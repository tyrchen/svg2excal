//! Bounded SVG/SVGZ preflight and deterministic `usvg` normalization.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    sync::Arc,
};

use crate::{
    ConversionOptions,
    error::{ConversionError, InputRejection, LimitResource},
    identity::document_digest,
    report::{ConversionDiagnostic, DiagnosticCode, DiagnosticSeverity},
    resource::{ProvidedResourcePolicy, ResourceContext, ResourceRequest},
    source::{SourceMetadata, prepare_source},
};

const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";
const LIBERATION_SANS: &[u8] = include_bytes!("../assets/fonts/LiberationSans-Regular.ttf");
const NOTO_EMOJI: &[u8] = include_bytes!("../assets/fonts/NotoEmoji-Regular.ttf");
const XIAOLAI_BASIC_CJK: &[u8] = include_bytes!("../assets/fonts/Xiaolai-Regular-Basic-CJK.ttf");

pub(crate) const fn bundled_target_font() -> &'static [u8] {
    LIBERATION_SANS
}

#[derive(Debug)]
pub(crate) struct SourceCensus {
    pub(crate) elements: usize,
    pub(crate) references: usize,
    pub(crate) active_content: usize,
    pub(crate) external_references: Vec<String>,
    pub(crate) nested_svg_data_images: usize,
}

#[derive(Clone)]
enum ResolvedImage {
    Raster(Box<usvg::ImageKind>),
    NestedSvg(Box<usvg::Tree>),
}

#[derive(Clone, Default)]
struct ResolvedImages {
    external: BTreeMap<String, ResolvedImage>,
    data: BTreeMap<String, ResolvedImage>,
}

#[derive(Default)]
struct ResourceBudget {
    decoded_bytes: usize,
    xml_elements: usize,
    references: usize,
}

#[derive(Debug)]
pub(crate) struct NormalizedInput {
    pub(crate) tree: usvg::Tree,
    pub(crate) census: SourceCensus,
    pub(crate) diagnostics: Vec<ConversionDiagnostic>,
    pub(crate) input_bytes: usize,
    pub(crate) paint_nodes: usize,
    pub(crate) digest: blake3::Hash,
    pub(crate) source: SourceMetadata,
}

pub(crate) fn normalize(
    input: &[u8],
    options: &ConversionOptions,
    resources: Option<&ResourceContext<'_>>,
) -> Result<NormalizedInput, ConversionError> {
    options.check_cancelled()?;
    if !options.is_valid() {
        return Err(ConversionError::InputRejected(
            InputRejection::InvalidOptions,
        ));
    }
    if input.is_empty() {
        return Err(ConversionError::InputRejected(InputRejection::Empty));
    }
    if input.len() > options.limits.max_input_bytes() {
        return Err(limit(
            LimitResource::InputBytes,
            options.limits.max_input_bytes(),
        ));
    }

    let decompressed = if input.starts_with(&[0x1f, 0x8b]) {
        decompress_svgz(input, options)?
    } else {
        input.to_vec()
    };
    let text = std::str::from_utf8(&decompressed)
        .map_err(|_| ConversionError::InputRejected(InputRejection::InvalidUtf8))?;
    reject_prohibited_declarations(text)?;
    reject_illegal_controls(text)?;

    let parsing_options = usvg::roxmltree::ParsingOptions {
        allow_dtd: false,
        ..Default::default()
    };
    let document = usvg::roxmltree::Document::parse_with_options(text, parsing_options)
        .map_err(|error| sanitize_xml_error(&error))?;
    validate_root(&document)?;
    let census = census(&document, options)?;
    if census.nested_svg_data_images > 0 && !options.raster.allow_nested_svg() {
        return Err(ConversionError::ResourceDenied { kind: "nested-svg" });
    }
    let prepared = prepare_source(text, &document, &options.limits)?;
    if prepared.metadata.len() > options.limits.max_correlation_candidates() {
        return Err(limit(
            LimitResource::WorkUnits,
            options.limits.max_correlation_candidates(),
        ));
    }
    let mut resource_budget = ResourceBudget {
        decoded_bytes: 0,
        xml_elements: census.elements,
        references: census.references,
    };
    let resolved = resolve_images(
        &document,
        &census.external_references,
        options,
        resources,
        &mut resource_budget,
    )?;
    let usvg_options = deterministic_usvg_options(options, &resolved);
    let instrumented = usvg::roxmltree::Document::parse_with_options(
        &prepared.xml,
        usvg::roxmltree::ParsingOptions {
            allow_dtd: false,
            ..Default::default()
        },
    )
    .map_err(|error| sanitize_xml_error(&error))?;
    let tree = usvg::Tree::from_xmltree(&instrumented, &usvg_options)
        .map_err(|error| sanitize_normalization_error(&error))?;

    let paint_nodes = count_paint_nodes(tree.root(), options)?;
    let mut diagnostics = Vec::new();
    if census.active_content > 0 {
        diagnostics.push(ConversionDiagnostic::new(
            DiagnosticCode::ActiveContentIgnored,
            DiagnosticSeverity::Info,
            0,
            "active SVG content was ignored and never executed",
        ));
    }
    let digest = document_digest(&decompressed);
    Ok(NormalizedInput {
        tree,
        census,
        diagnostics,
        input_bytes: input.len(),
        paint_nodes,
        digest,
        source: prepared.metadata,
    })
}

fn decompress_svgz(input: &[u8], options: &ConversionOptions) -> Result<Vec<u8>, ConversionError> {
    let ratio_limit = input
        .len()
        .saturating_mul(options.limits.max_svgz_expansion_ratio());
    let byte_limit = options.limits.max_decompressed_bytes().min(ratio_limit);
    let mut decoder = flate2::read::GzDecoder::new(input);
    let initial_capacity = input.len().saturating_mul(2).min(byte_limit);
    let mut output = Vec::with_capacity(initial_capacity);
    let mut buffer = [0_u8; 8192];
    loop {
        let read = decoder
            .read(&mut buffer)
            .map_err(|_| ConversionError::NormalizationFailed {
                category: "malformed SVGZ stream",
            })?;
        if read == 0 {
            break;
        }
        let new_len = output.len().checked_add(read).ok_or_else(|| {
            limit(
                LimitResource::DecompressedBytes,
                options.limits.max_decompressed_bytes(),
            )
        })?;
        if new_len > byte_limit {
            return Err(limit(LimitResource::DecompressedBytes, byte_limit));
        }
        let bytes = buffer
            .get(..read)
            .ok_or(ConversionError::GeometryOverflow)?;
        output.extend_from_slice(bytes);
    }
    Ok(output)
}

fn reject_prohibited_declarations(text: &str) -> Result<(), ConversionError> {
    if text.contains("<!DOCTYPE") || text.contains("<!ENTITY") {
        return Err(ConversionError::InputRejected(InputRejection::DtdOrEntity));
    }
    Ok(())
}

fn reject_illegal_controls(text: &str) -> Result<(), ConversionError> {
    if text
        .chars()
        .any(|character| character < '\u{20}' && !matches!(character, '\t' | '\n' | '\r'))
    {
        return Err(ConversionError::InputRejected(
            InputRejection::IllegalControlCharacter,
        ));
    }
    Ok(())
}

fn validate_root(document: &usvg::roxmltree::Document<'_>) -> Result<(), ConversionError> {
    let root = document
        .root()
        .children()
        .find(usvg::roxmltree::Node::is_element)
        .ok_or(ConversionError::UnsupportedRoot)?;
    if root.tag_name().name() != "svg" || root.tag_name().namespace() != Some(SVG_NAMESPACE) {
        return Err(ConversionError::UnsupportedRoot);
    }
    Ok(())
}

fn census(
    document: &usvg::roxmltree::Document<'_>,
    options: &ConversionOptions,
) -> Result<SourceCensus, ConversionError> {
    let limits = &options.limits;
    let mut elements = 0_usize;
    let mut total_attributes = 0_usize;
    let mut total_text_bytes = 0_usize;
    let mut references = 0_usize;
    let mut active_content = 0_usize;
    let mut nested_svg_data_images = 0_usize;
    let mut input_images = 0_usize;
    let mut external_references = Vec::new();
    let mut external_seen = BTreeSet::new();

    for node in document.descendants() {
        options.check_cancelled()?;
        if node.is_element() {
            elements = elements
                .checked_add(1)
                .ok_or_else(|| limit(LimitResource::XmlElements, limits.max_xml_elements()))?;
            if elements > limits.max_xml_elements() {
                return Err(limit(LimitResource::XmlElements, limits.max_xml_elements()));
            }
            let depth = node
                .ancestors()
                .filter(usvg::roxmltree::Node::is_element)
                .count();
            if depth > limits.max_xml_depth() {
                return Err(limit(LimitResource::XmlDepth, limits.max_xml_depth()));
            }
            let attribute_count = node.attributes().len();
            if attribute_count > limits.max_attributes_per_element() {
                return Err(limit(
                    LimitResource::XmlAttributes,
                    limits.max_attributes_per_element(),
                ));
            }
            total_attributes = total_attributes
                .checked_add(attribute_count)
                .ok_or_else(|| {
                    limit(LimitResource::XmlAttributes, limits.max_total_attributes())
                })?;
            if total_attributes > limits.max_total_attributes() {
                return Err(limit(
                    LimitResource::XmlAttributes,
                    limits.max_total_attributes(),
                ));
            }
            if is_active_element(node.tag_name().name()) {
                active_content = active_content.saturating_add(1);
            }
            reserve_input_image(node, limits, &mut input_images)?;
            for attribute in node.attributes() {
                let value = attribute.value();
                validate_lexical_value(value, limits, &mut total_text_bytes)?;
                if attribute.name().starts_with("on") {
                    active_content = active_content.saturating_add(1);
                }
                if is_reference_attribute(node.tag_name().name(), attribute.name()) {
                    if is_nested_svg_data_image(node.tag_name().name(), attribute.value()) {
                        nested_svg_data_images = nested_svg_data_images.saturating_add(1);
                    }
                    classify_reference(
                        value,
                        limits,
                        &mut references,
                        &mut external_references,
                        &mut external_seen,
                    )?;
                }
                for url in css_urls(value) {
                    classify_reference(
                        url,
                        limits,
                        &mut references,
                        &mut external_references,
                        &mut external_seen,
                    )?;
                }
            }
        } else if node.is_text() {
            census_text_node(
                node,
                limits,
                &mut total_text_bytes,
                &mut references,
                &mut external_references,
                &mut external_seen,
            )?;
        }
    }
    if references > limits.max_references() {
        return Err(limit(LimitResource::References, limits.max_references()));
    }
    validate_reference_depth(document, limits)?;

    Ok(SourceCensus {
        elements,
        references,
        active_content,
        external_references,
        nested_svg_data_images,
    })
}

fn reserve_input_image(
    node: usvg::roxmltree::Node<'_, '_>,
    limits: &crate::ConversionLimits,
    input_images: &mut usize,
) -> Result<(), ConversionError> {
    if node.tag_name().name() != "image" {
        return Ok(());
    }
    *input_images = input_images
        .checked_add(1)
        .ok_or_else(|| limit(LimitResource::Images, limits.max_input_images()))?;
    if *input_images > limits.max_input_images() {
        return Err(limit(LimitResource::Images, limits.max_input_images()));
    }
    Ok(())
}

fn census_text_node(
    node: usvg::roxmltree::Node<'_, '_>,
    limits: &crate::ConversionLimits,
    total_text_bytes: &mut usize,
    references: &mut usize,
    external_references: &mut Vec<String>,
    external_seen: &mut BTreeSet<String>,
) -> Result<(), ConversionError> {
    let text = node.text().unwrap_or_default();
    validate_lexical_value(text, limits, total_text_bytes)?;
    if node
        .parent_element()
        .is_some_and(|parent| parent.tag_name().name() == "style")
    {
        for reference in css_urls(text).chain(css_imports(text)) {
            classify_reference(
                reference,
                limits,
                references,
                external_references,
                external_seen,
            )?;
        }
    }
    Ok(())
}

fn is_nested_svg_data_image(element_name: &str, reference: &str) -> bool {
    element_name == "image"
        && reference
            .trim()
            .to_ascii_lowercase()
            .starts_with("data:image/svg+xml")
}

fn validate_lexical_value(
    value: &str,
    limits: &crate::ConversionLimits,
    aggregate: &mut usize,
) -> Result<(), ConversionError> {
    if value.len() > limits.max_single_text_bytes() {
        return Err(limit(
            LimitResource::XmlText,
            limits.max_single_text_bytes(),
        ));
    }
    *aggregate = aggregate
        .checked_add(value.len())
        .ok_or_else(|| limit(LimitResource::XmlText, limits.max_total_text_bytes()))?;
    if *aggregate > limits.max_total_text_bytes() {
        return Err(limit(LimitResource::XmlText, limits.max_total_text_bytes()));
    }
    Ok(())
}

fn is_active_element(name: &str) -> bool {
    matches!(
        name,
        "script"
            | "foreignObject"
            | "animate"
            | "animateMotion"
            | "animateTransform"
            | "set"
            | "discard"
            | "a"
    )
}

fn is_reference_attribute(element_name: &str, attribute_name: &str) -> bool {
    element_name != "a" && matches!(attribute_name, "href" | "src")
}

fn css_urls(value: &str) -> impl Iterator<Item = &str> {
    value.split("url(").skip(1).filter_map(|suffix| {
        let end = suffix.find(')')?;
        let raw = suffix.get(..end)?.trim();
        Some(raw.trim_matches(|character| matches!(character, '\'' | '"')))
    })
}

fn css_imports(value: &str) -> impl Iterator<Item = &str> {
    value.split("@import").skip(1).filter_map(|suffix| {
        let statement = suffix.split(';').next()?.trim();
        if statement.starts_with("url(") {
            return None;
        }
        let reference = statement.split_ascii_whitespace().next()?;
        Some(reference.trim_matches(|character| matches!(character, '\'' | '"')))
    })
}

fn classify_reference(
    value: &str,
    limits: &crate::ConversionLimits,
    count: &mut usize,
    external: &mut Vec<String>,
    external_seen: &mut BTreeSet<String>,
) -> Result<(), ConversionError> {
    *count = count
        .checked_add(1)
        .ok_or_else(|| limit(LimitResource::References, limits.max_references()))?;
    let value = value.trim();
    if value.starts_with('#') || value.is_empty() {
        return Ok(());
    }
    if value.starts_with("data:") {
        if value.len() > limits.max_data_url_bytes() {
            return Err(limit(
                LimitResource::EmbeddedBytes,
                limits.max_data_url_bytes(),
            ));
        }
        return Ok(());
    }
    if external_seen.insert(value.to_owned()) {
        external.push(value.to_owned());
    }
    Ok(())
}

fn validate_reference_depth(
    document: &usvg::roxmltree::Document<'_>,
    limits: &crate::ConversionLimits,
) -> Result<(), ConversionError> {
    let mut graph = BTreeMap::<String, Vec<String>>::new();
    for node in document
        .descendants()
        .filter(usvg::roxmltree::Node::is_element)
    {
        if let Some(id) = node.attribute("id") {
            graph.entry(id.to_owned()).or_default();
        }
        let Some(owner_id) = node.attribute("id").or_else(|| {
            node.ancestors()
                .filter(usvg::roxmltree::Node::is_element)
                .find_map(|ancestor| ancestor.attribute("id"))
        }) else {
            continue;
        };
        let references = graph.entry(owner_id.to_owned()).or_default();
        for attribute in node.attributes() {
            if is_reference_attribute(node.tag_name().name(), attribute.name())
                && let Some(target) = attribute.value().trim().strip_prefix('#')
            {
                references.push(target.to_owned());
            }
            references.extend(
                css_urls(attribute.value())
                    .filter_map(|value| value.strip_prefix('#'))
                    .map(str::to_owned),
            );
        }
    }

    let mut state = BTreeMap::<String, u8>::new();
    let mut depths = BTreeMap::<String, usize>::new();
    for root in graph.keys() {
        if state.get(root).copied() == Some(2) {
            continue;
        }
        let mut stack = vec![(root.clone(), 0_usize, 1_usize)];
        state.insert(root.clone(), 1);
        while !stack.is_empty() {
            let Some((current, next_index, _)) = stack.last_mut() else {
                break;
            };
            let next = graph
                .get(current)
                .and_then(|targets| targets.get(*next_index))
                .cloned();
            if let Some(target) = next {
                *next_index = next_index.saturating_add(1);
                match state.get(&target).copied().unwrap_or_default() {
                    0 if graph.contains_key(&target) => {
                        state.insert(target.clone(), 1);
                        stack.push((target, 0, 1));
                    }
                    1 => {
                        return Err(limit(
                            LimitResource::References,
                            limits.max_reference_depth(),
                        ));
                    }
                    2 => {
                        let child_depth = depths.get(&target).copied().unwrap_or(1);
                        let Some((_, _, current_depth)) = stack.last_mut() else {
                            break;
                        };
                        *current_depth = (*current_depth).max(child_depth.saturating_add(1));
                        if *current_depth > limits.max_reference_depth() {
                            return Err(limit(
                                LimitResource::References,
                                limits.max_reference_depth(),
                            ));
                        }
                    }
                    _ => {}
                }
            } else {
                let Some((completed, _, completed_depth)) = stack.pop() else {
                    break;
                };
                if completed_depth > limits.max_reference_depth() {
                    return Err(limit(
                        LimitResource::References,
                        limits.max_reference_depth(),
                    ));
                }
                state.insert(completed.clone(), 2);
                depths.insert(completed, completed_depth);
                if let Some((_, _, parent_depth)) = stack.last_mut() {
                    *parent_depth = (*parent_depth).max(completed_depth.saturating_add(1));
                }
            }
        }
    }
    Ok(())
}

fn resolve_images(
    document: &usvg::roxmltree::Document<'_>,
    references: &[String],
    options: &ConversionOptions,
    resources: Option<&ResourceContext<'_>>,
    budget: &mut ResourceBudget,
) -> Result<Arc<ResolvedImages>, ConversionError> {
    let mut resolved = ResolvedImages::default();
    prevalidate_data_images(document, options, 1, budget, &mut resolved.data)?;
    if references.is_empty() {
        return Ok(Arc::new(resolved));
    }
    let context = resources.ok_or(ConversionError::ResourceDenied {
        kind: "path-or-url",
    })?;
    let ProvidedResourcePolicy::RelativeFiles(policy) = &context.policy;
    for reference in references {
        options.check_cancelled()?;
        let request = ResourceRequest::parse(reference, policy).map_err(|_| {
            ConversionError::ResourceDenied {
                kind: "invalid-relative-path",
            }
        })?;
        let resource =
            context
                .provider
                .load(&request)
                .map_err(|_| ConversionError::NormalizationFailed {
                    category: "resource provider failed",
                })?;
        reserve_resource_bytes(resource.bytes().len(), options, budget)?;
        let kind = if resource.mime_type() == "image/svg+xml" {
            if !options.raster.allow_nested_svg() {
                return Err(ConversionError::ResourceDenied { kind: "nested-svg" });
            }
            ResolvedImage::NestedSvg(Box::new(parse_nested_svg(
                resource.bytes(),
                options,
                1,
                budget,
            )?))
        } else {
            ResolvedImage::Raster(Box::new(checked_image_kind(
                resource.mime_type(),
                resource.bytes(),
                options,
            )?))
        };
        resolved.external.insert(reference.clone(), kind);
    }
    Ok(Arc::new(resolved))
}

fn parse_nested_svg(
    bytes: &[u8],
    options: &ConversionOptions,
    depth: usize,
    budget: &mut ResourceBudget,
) -> Result<usvg::Tree, ConversionError> {
    if depth > options.limits.max_nested_svg_depth() {
        return Err(limit(
            LimitResource::References,
            options.limits.max_nested_svg_depth(),
        ));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| ConversionError::InputRejected(InputRejection::InvalidUtf8))?;
    reject_prohibited_declarations(text)?;
    reject_illegal_controls(text)?;
    let document = usvg::roxmltree::Document::parse_with_options(
        text,
        usvg::roxmltree::ParsingOptions {
            allow_dtd: false,
            ..Default::default()
        },
    )
    .map_err(|error| sanitize_xml_error(&error))?;
    validate_root(&document)?;
    let nested_census = census(&document, options)?;
    if !nested_census.external_references.is_empty() {
        return Err(ConversionError::ResourceDenied {
            kind: "nested-external-resource",
        });
    }
    budget.xml_elements = budget
        .xml_elements
        .checked_add(nested_census.elements)
        .ok_or_else(|| {
            limit(
                LimitResource::XmlElements,
                options.limits.max_xml_elements(),
            )
        })?;
    if budget.xml_elements > options.limits.max_xml_elements() {
        return Err(limit(
            LimitResource::XmlElements,
            options.limits.max_xml_elements(),
        ));
    }
    budget.references = budget
        .references
        .checked_add(nested_census.references)
        .ok_or_else(|| limit(LimitResource::References, options.limits.max_references()))?;
    if budget.references > options.limits.max_references() {
        return Err(limit(
            LimitResource::References,
            options.limits.max_references(),
        ));
    }
    let mut resolved = ResolvedImages::default();
    prevalidate_data_images(
        &document,
        options,
        depth.saturating_add(1),
        budget,
        &mut resolved.data,
    )?;
    let usvg_options = deterministic_usvg_options(options, &Arc::new(resolved));
    usvg::Tree::from_xmltree(&document, &usvg_options)
        .map_err(|error| sanitize_normalization_error(&error))
}

fn prevalidate_data_images(
    document: &usvg::roxmltree::Document<'_>,
    options: &ConversionOptions,
    nested_depth: usize,
    budget: &mut ResourceBudget,
    output: &mut BTreeMap<String, ResolvedImage>,
) -> Result<(), ConversionError> {
    for node in document
        .descendants()
        .filter(usvg::roxmltree::Node::is_element)
    {
        options.check_cancelled()?;
        if node.tag_name().name() != "image" {
            continue;
        }
        for attribute in node
            .attributes()
            .filter(|attribute| attribute.name() == "href")
        {
            let value = attribute.value().trim();
            if !value.to_ascii_lowercase().starts_with("data:") {
                continue;
            }
            let data_url = data_url::DataUrl::process(value).map_err(|_| {
                ConversionError::NormalizationFailed {
                    category: "malformed image data URL",
                }
            })?;
            let (bytes, _) =
                data_url
                    .decode_to_vec()
                    .map_err(|_| ConversionError::NormalizationFailed {
                        category: "malformed image data URL body",
                    })?;
            reserve_resource_bytes(bytes.len(), options, budget)?;
            let image = if data_url.mime_type().matches("image", "svg+xml") {
                if !options.raster.allow_nested_svg() {
                    return Err(ConversionError::ResourceDenied { kind: "nested-svg" });
                }
                ResolvedImage::NestedSvg(Box::new(parse_nested_svg(
                    &bytes,
                    options,
                    nested_depth,
                    budget,
                )?))
            } else {
                let mime = if data_url.mime_type().matches("image", "png") {
                    "image/png"
                } else if data_url.mime_type().matches("image", "jpeg") {
                    "image/jpeg"
                } else if data_url.mime_type().matches("image", "gif") {
                    "image/gif"
                } else if data_url.mime_type().matches("image", "webp") {
                    "image/webp"
                } else {
                    return Err(ConversionError::ResourceDenied {
                        kind: "image-data-mime",
                    });
                };
                ResolvedImage::Raster(Box::new(checked_image_kind(mime, &bytes, options)?))
            };
            output.insert(image_digest(&bytes), image);
        }
    }
    Ok(())
}

fn reserve_resource_bytes(
    bytes: usize,
    options: &ConversionOptions,
    budget: &mut ResourceBudget,
) -> Result<(), ConversionError> {
    if bytes > options.limits.max_resource_bytes() {
        return Err(limit(
            LimitResource::EmbeddedBytes,
            options.limits.max_resource_bytes(),
        ));
    }
    budget.decoded_bytes = budget.decoded_bytes.checked_add(bytes).ok_or_else(|| {
        limit(
            LimitResource::EmbeddedBytes,
            options.limits.max_resource_bytes_total(),
        )
    })?;
    if budget.decoded_bytes > options.limits.max_resource_bytes_total() {
        return Err(limit(
            LimitResource::EmbeddedBytes,
            options.limits.max_resource_bytes_total(),
        ));
    }
    Ok(())
}

fn checked_image_kind(
    mime: &str,
    bytes: &[u8],
    options: &ConversionOptions,
) -> Result<usvg::ImageKind, ConversionError> {
    let dimensions =
        imagesize::blob_size(bytes).map_err(|_| ConversionError::NormalizationFailed {
            category: "invalid raster image header",
        })?;
    let pixels = u64::try_from(dimensions.width)
        .ok()
        .and_then(|width| {
            u64::try_from(dimensions.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or(ConversionError::GeometryOverflow)?;
    if pixels > options.limits.max_raster_pixels_per_island() {
        return Err(ConversionError::LimitExceeded {
            resource: LimitResource::RasterPixels,
            limit: options.limits.max_raster_pixels_per_island(),
        });
    }
    let data = Arc::new(bytes.to_vec());
    match mime {
        "image/png" if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => Ok(usvg::ImageKind::PNG(data)),
        "image/jpeg" if bytes.starts_with(&[0xff, 0xd8, 0xff]) => Ok(usvg::ImageKind::JPEG(data)),
        "image/gif" if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") => {
            Ok(usvg::ImageKind::GIF(data))
        }
        "image/webp"
            if bytes.starts_with(b"RIFF")
                && bytes.get(8..12).is_some_and(|magic| magic == b"WEBP") =>
        {
            Ok(usvg::ImageKind::WEBP(data))
        }
        _ => Err(ConversionError::NormalizationFailed {
            category: "resource MIME or magic mismatch",
        }),
    }
}

fn deterministic_usvg_options(
    options: &ConversionOptions,
    resolved: &Arc<ResolvedImages>,
) -> usvg::Options<'static> {
    let max_resource_bytes = options.limits.max_resource_bytes();
    let data_store = Arc::clone(resolved);
    let string_store = Arc::clone(resolved);
    let mut usvg_options = usvg::Options {
        resources_dir: None,
        font_family: "Liberation Sans".to_owned(),
        ..Default::default()
    };
    if options.fonts.substitute_with_liberation_sans() {
        let fontdb = usvg_options.fontdb_mut();
        fontdb.load_font_source(usvg::fontdb::Source::Binary(Arc::new(LIBERATION_SANS)));
        fontdb.load_font_source(usvg::fontdb::Source::Binary(Arc::new(NOTO_EMOJI)));
        fontdb.load_font_source(usvg::fontdb::Source::Binary(Arc::new(XIAOLAI_BASIC_CJK)));
        fontdb.set_serif_family("Liberation Sans");
        fontdb.set_sans_serif_family("Liberation Sans");
    }
    usvg_options.image_href_resolver = usvg::ImageHrefResolver {
        resolve_data: Box::new(move |_mime, data, _nested_options| {
            if data.len() > max_resource_bytes {
                return None;
            }
            match data_store.data.get(&image_digest(data.as_slice())) {
                Some(ResolvedImage::Raster(image)) => Some((**image).clone()),
                Some(ResolvedImage::NestedSvg(tree)) => {
                    Some(usvg::ImageKind::SVG((**tree).clone()))
                }
                None => None,
            }
        }),
        resolve_string: Box::new(move |href, _nested_options| {
            match string_store.external.get(href) {
                Some(ResolvedImage::Raster(image)) => Some((**image).clone()),
                Some(ResolvedImage::NestedSvg(tree)) => {
                    Some(usvg::ImageKind::SVG((**tree).clone()))
                }
                None => None,
            }
        }),
    };
    usvg_options
}

fn image_digest(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn count_paint_nodes(
    group: &usvg::Group,
    options: &ConversionOptions,
) -> Result<usize, ConversionError> {
    let limit_value = options.limits.max_paint_nodes();
    let mut count = 1_usize;
    let mut stack = vec![group];
    while let Some(current) = stack.pop() {
        options.check_cancelled()?;
        for node in current.children() {
            count = count
                .checked_add(1)
                .ok_or_else(|| limit(LimitResource::PaintNodes, limit_value))?;
            if count > limit_value {
                return Err(limit(LimitResource::PaintNodes, limit_value));
            }
            if let usvg::Node::Group(child) = node {
                stack.push(child);
            } else if let usvg::Node::Image(image) = node
                && let usvg::ImageKind::SVG(tree) = image.kind()
            {
                stack.push(tree.root());
            }
        }
    }
    Ok(count)
}

fn sanitize_xml_error(error: &usvg::roxmltree::Error) -> ConversionError {
    let position = error.pos();
    ConversionError::MalformedXml {
        category: "XML syntax",
        line: position.row,
        column: position.col,
    }
}

fn sanitize_normalization_error(error: &usvg::Error) -> ConversionError {
    let category = match error {
        usvg::Error::NotAnUtf8Str => "non-UTF-8 input",
        usvg::Error::SvgzFeatureNotEnabled => "unexpected compressed input",
        usvg::Error::MalformedGZip => "malformed compressed input",
        usvg::Error::ElementsLimitReached => "upstream element limit",
        usvg::Error::InvalidSize => "invalid SVG viewport size",
        usvg::Error::ParsingFailed(_) => "validated XML rejected during normalization",
    };
    ConversionError::NormalizationFailed { category }
}

fn limit(resource: LimitResource, value: usize) -> ConversionError {
    ConversionError::LimitExceeded {
        resource,
        limit: u64::try_from(value).unwrap_or(u64::MAX),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::{Compression, write::GzEncoder};

    use crate::{ConversionError, ConversionOptions, InputRejection, convert};

    #[test]
    fn test_should_reject_dtd_before_normalization() {
        let input = br#"<!DOCTYPE svg><svg xmlns="http://www.w3.org/2000/svg"/>"#;
        let result = convert(input, &ConversionOptions::default());
        assert!(matches!(
            result,
            Err(ConversionError::InputRejected(InputRejection::DtdOrEntity))
        ));
    }

    #[test]
    fn test_should_parse_bounded_svgz() -> Result<(), Box<dyn std::error::Error>> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>"#,
        )?;
        let bytes = encoder.finish()?;
        let result = convert(&bytes, &ConversionOptions::default());
        assert!(result.is_ok());
        Ok(())
    }
}
