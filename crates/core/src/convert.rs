//! Conversion orchestration and native/fallback lowering.

use std::collections::{BTreeMap, BTreeSet};

use base64::Engine as _;
use usvg::tiny_skia_path::{PathSegment, Point as SkiaPoint, Transform};

use crate::{
    ConversionOptions, ConversionProfile, ProvenanceMode,
    error::{ConversionError, LimitResource},
    identity::{element_identity, file_id},
    ingest::{NormalizedInput, normalize},
    report::{
        ConversionDiagnostic, ConversionReport, ConversionResult, DiagnosticCode,
        DiagnosticSeverity, sort_diagnostics,
    },
    resource::ResourceContext,
    target::{
        BinaryFile, ElementBase, ElementStyle, ExcalidrawColor, ExcalidrawDocument,
        ExcalidrawElement, FileId, Finite, LocalPoint, StrokeStyle, TextAlign,
    },
};

/// Converts SVG/SVGZ bytes without external I/O.
///
/// # Errors
///
/// Returns a typed [`ConversionError`] for invalid input, denied resources,
/// exhausted limits, strict fidelity violations, or invalid target output.
pub fn convert(
    svg: &[u8],
    options: &ConversionOptions,
) -> Result<ConversionResult, ConversionError> {
    convert_internal(svg, options, None)
}

/// Converts SVG/SVGZ bytes using an explicit bounded resource provider.
///
/// # Errors
///
/// Returns a typed [`ConversionError`] under the same contract as [`convert`],
/// plus provider/resource validation failures.
pub fn convert_with_resources(
    svg: &[u8],
    options: &ConversionOptions,
    resources: &ResourceContext<'_>,
) -> Result<ConversionResult, ConversionError> {
    convert_internal(svg, options, Some(resources))
}

fn convert_internal(
    svg: &[u8],
    options: &ConversionOptions,
    resources: Option<&ResourceContext<'_>>,
) -> Result<ConversionResult, ConversionError> {
    let normalized = normalize(svg, options, resources)?;
    let scene_scale = scene_scale(&normalized, options)?;
    let mut context = LoweringContext::new(options, &normalized, scene_scale);
    context.lower_group(normalized.tree.root(), 1.0)?;
    sort_diagnostics(&mut context.diagnostics);

    if options.profile == ConversionProfile::Strict
        && context
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity() != DiagnosticSeverity::Info)
    {
        return Err(ConversionError::StrictFidelityViolation {
            diagnostics: context.diagnostics,
        });
    }

    let document = ExcalidrawDocument::new(context.elements, context.files);
    document.validate(&options.limits)?;
    let _serialized = document.to_pretty_json_with_limits(&options.limits)?;
    let report = ConversionReport {
        profile: options.profile.into(),
        input_bytes: normalized.input_bytes,
        source_elements: normalized.census.elements,
        source_references: normalized.census.references,
        paint_nodes: normalized.paint_nodes,
        target_elements: document.elements().len(),
        target_points: context.target_points,
        embedded_bytes: context.embedded_bytes,
        fallback_pixels: context.fallback_pixels,
        diagnostics: context.diagnostics,
    };
    Ok(ConversionResult { document, report })
}

fn scene_scale(
    normalized: &NormalizedInput,
    options: &ConversionOptions,
) -> Result<f64, ConversionError> {
    let width = f64::from(normalized.tree.size().width());
    let height = f64::from(normalized.tree.size().height());
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err(ConversionError::GeometryOverflow);
    }
    let largest = width.max(height);
    if largest <= options.limits.max_element_extent() {
        return Ok(1.0);
    }
    Ok(options.limits.max_element_extent() / largest)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn transformed(point: SkiaPoint, transform: Transform, scene_scale: f64) -> Self {
        let x = f64::from(transform.sx) * f64::from(point.x)
            + f64::from(transform.kx) * f64::from(point.y)
            + f64::from(transform.tx);
        let y = f64::from(transform.ky) * f64::from(point.x)
            + f64::from(transform.sy) * f64::from(point.y)
            + f64::from(transform.ty);
        Self {
            x: x * scene_scale,
            y: y * scene_scale,
        }
    }

    fn midpoint(self, other: Self) -> Self {
        Self {
            x: (self.x + other.x) * 0.5,
            y: (self.y + other.y) * 0.5,
        }
    }
}

#[derive(Debug)]
struct Subpath {
    points: Vec<Point>,
    closed: bool,
    curved: bool,
}

#[derive(Debug, Clone, Copy)]
enum PaintRole {
    Combined,
    Fill,
    Stroke,
}

struct LoweringContext<'a> {
    options: &'a ConversionOptions,
    digest: &'a blake3::Hash,
    elements: Vec<ExcalidrawElement>,
    element_ids: BTreeSet<String>,
    files: BTreeMap<FileId, BinaryFile>,
    diagnostics: Vec<ConversionDiagnostic>,
    source_order: u32,
    total_path_segments: usize,
    target_points: usize,
    fallback_pixels: u64,
    embedded_bytes: usize,
    scene_scale: f64,
}

impl<'a> LoweringContext<'a> {
    fn new(
        options: &'a ConversionOptions,
        normalized: &'a NormalizedInput,
        scene_scale: f64,
    ) -> Self {
        let mut diagnostics = normalized.diagnostics.clone();
        if scene_scale < 1.0 {
            diagnostics.push(ConversionDiagnostic::new(
                DiagnosticCode::SceneScaledToTargetRange,
                DiagnosticSeverity::Approximation,
                0,
                "scene was uniformly scaled to fit Excalidraw restoration bounds",
            ));
        }
        Self {
            options,
            digest: &normalized.digest,
            elements: Vec::with_capacity(normalized.paint_nodes.min(4096)),
            element_ids: BTreeSet::new(),
            files: BTreeMap::new(),
            diagnostics,
            source_order: 0,
            total_path_segments: 0,
            target_points: 0,
            fallback_pixels: 0,
            embedded_bytes: 0,
            scene_scale,
        }
    }

    fn lower_group(
        &mut self,
        group: &usvg::Group,
        inherited_opacity: f64,
    ) -> Result<(), ConversionError> {
        let opacity = inherited_opacity * f64::from(group.opacity().get());
        for node in group.children() {
            self.source_order =
                self.source_order
                    .checked_add(1)
                    .ok_or(ConversionError::LimitExceeded {
                        resource: LimitResource::WorkUnits,
                        limit: u64::from(u32::MAX),
                    })?;
            match node {
                usvg::Node::Group(child) if child.should_isolate() => {
                    self.fallback_or_violation(node, "isolated SVG compositing group", None)?;
                }
                usvg::Node::Group(child) => self.lower_group(child, opacity)?,
                usvg::Node::Path(path) if path.is_visible() => {
                    self.lower_path(node, path, opacity)?;
                }
                usvg::Node::Path(_) => {}
                usvg::Node::Text(text) => self.lower_text(node, text, opacity)?,
                usvg::Node::Image(_) => {
                    self.fallback_or_violation(node, "source raster image", None)?;
                }
            }
        }
        Ok(())
    }

    fn lower_path(
        &mut self,
        node: &usvg::Node,
        path: &usvg::Path,
        opacity: f64,
    ) -> Result<(), ConversionError> {
        if path.fill().is_none() && path.stroke().is_none() {
            return Ok(());
        }
        if !path_has_only_solid_paint(path) || !stroke_transform_is_scalar(path) {
            return self.fallback_or_violation(
                node,
                "non-native path paint or stroke transform",
                None,
            );
        }
        if path.stroke().is_some() && !stroke_is_exact_native(path) {
            self.diagnostics.push(ConversionDiagnostic::new(
                DiagnosticCode::StrokeStyleApproximated,
                DiagnosticSeverity::Approximation,
                self.source_order,
                "SVG stroke dash, cap, join, or miter semantics were approximated",
            ));
        }
        let segment_count = path.data().segments().count();
        if segment_count > self.options.limits.max_path_segments_per_path() {
            return Err(ConversionError::LimitExceeded {
                resource: LimitResource::PathSegments,
                limit: usize_to_u64(self.options.limits.max_path_segments_per_path()),
            });
        }
        self.total_path_segments = self.total_path_segments.checked_add(segment_count).ok_or(
            ConversionError::LimitExceeded {
                resource: LimitResource::PathSegments,
                limit: usize_to_u64(self.options.limits.max_path_segments()),
            },
        )?;
        if self.total_path_segments > self.options.limits.max_path_segments() {
            return Err(ConversionError::LimitExceeded {
                resource: LimitResource::PathSegments,
                limit: usize_to_u64(self.options.limits.max_path_segments()),
            });
        }

        if looks_like_axis_aligned_ellipse(path) {
            return self.lower_ellipse(path, opacity);
        }
        let has_curves = path
            .data()
            .segments()
            .any(|segment| matches!(segment, PathSegment::QuadTo(..) | PathSegment::CubicTo(..)));
        if has_curves
            && matches!(
                self.options.profile,
                ConversionProfile::Fidelity | ConversionProfile::Strict
            )
        {
            return self.fallback_or_violation(
                node,
                "curved path",
                Some(DiagnosticCode::PathFlattened),
            );
        }

        let subpaths = flatten_path(
            path,
            self.options.geometry.curve_tolerance_px(),
            self.scene_scale,
            self.options.limits.max_target_points(),
        )?;
        if subpaths.is_empty() {
            return Ok(());
        }
        if subpaths.len() > 1 && path.fill().is_some() {
            return self.fallback_or_violation(node, "compound filled path", None);
        }
        if path.fill().is_some()
            && subpaths
                .iter()
                .any(|subpath| !is_simple_fill_boundary(subpath, self.options))
        {
            return self.fallback_or_violation(node, "non-simple filled path", None);
        }
        let flattened = subpaths.iter().any(|subpath| subpath.curved);
        for subpath in subpaths {
            self.lower_subpath(path, &subpath, opacity)?;
        }
        if flattened {
            self.diagnostics.push(ConversionDiagnostic::new(
                DiagnosticCode::PathFlattened,
                DiagnosticSeverity::Approximation,
                self.source_order,
                "curved path was flattened within the configured error bound",
            ));
        }
        Ok(())
    }

    fn lower_ellipse(&mut self, path: &usvg::Path, opacity: f64) -> Result<(), ConversionError> {
        let bbox = path.abs_bounding_box();
        let x = f64::from(bbox.x()) * self.scene_scale;
        let y = f64::from(bbox.y()) * self.scene_scale;
        let width = f64::from(bbox.width()) * self.scene_scale;
        let height = f64::from(bbox.height()) * self.scene_scale;
        for role in paint_roles(path, true) {
            let style = path_style(path, role, opacity, self.scene_scale);
            let base = self.make_base("ellipse", x, y, width, height, style)?;
            self.push_element(ExcalidrawElement::ellipse(base))?;
        }
        Ok(())
    }

    fn lower_subpath(
        &mut self,
        path: &usvg::Path,
        subpath: &Subpath,
        opacity: f64,
    ) -> Result<(), ConversionError> {
        if subpath.points.len() < 2 {
            return Ok(());
        }
        let has_painted_fill = path.fill().is_some()
            && subpath.points.len() >= 3
            && !points_are_collinear(&subpath.points);
        let open_fill_and_stroke = !subpath.closed && has_painted_fill && path.stroke().is_some();
        let roles = paint_roles(
            path,
            subpath.closed && has_painted_fill && !open_fill_and_stroke,
        );
        for role in roles {
            if matches!(role, PaintRole::Stroke) && path.stroke().is_none() {
                continue;
            }
            if matches!(role, PaintRole::Fill) && !has_painted_fill {
                continue;
            }
            let polygon = !matches!(role, PaintRole::Stroke) && path.fill().is_some();
            if polygon && subpath.points.len() < 3 {
                continue;
            }
            let style = path_style(path, role, opacity, self.scene_scale);
            if subpath.closed && is_axis_aligned_rectangle(&subpath.points) {
                let (x, y, width, height) = bounds(&subpath.points)?;
                let base = self.make_base("rectangle", x, y, width, height, style)?;
                self.push_element(ExcalidrawElement::rectangle(base))?;
            } else if subpath.closed && is_target_diamond(&subpath.points) {
                let (x, y, width, height) = bounds(&subpath.points)?;
                let base = self.make_base("diamond", x, y, width, height, style)?;
                self.push_element(ExcalidrawElement::diamond(base))?;
            } else {
                let close_stroke = subpath.closed && matches!(role, PaintRole::Stroke);
                self.emit_linear(&subpath.points, polygon, close_stroke, style)?;
            }
        }
        Ok(())
    }

    fn emit_linear(
        &mut self,
        points: &[Point],
        polygon: bool,
        close_stroke: bool,
        style: ElementStyle,
    ) -> Result<(), ConversionError> {
        let first = points.first().ok_or(ConversionError::GeometryOverflow)?;
        let (x, y, width, height) = bounds(points)?;
        let closes = polygon || close_stroke;
        let mut local = Vec::with_capacity(points.len().saturating_add(usize::from(closes)));
        for point in points {
            local.push(LocalPoint::new(
                point.x - first.x,
                point.y - first.y,
                &self.options.limits,
            )?);
        }
        if closes {
            local.push(LocalPoint::new(0.0, 0.0, &self.options.limits)?);
        }
        let added = local.len();
        self.reserve_points(added)?;
        let base = self.make_base("line", first.x, first.y, width, height, style)?;
        self.push_element(ExcalidrawElement::line(base, local, polygon))?;
        let _ = (x, y);
        Ok(())
    }

    fn lower_text(
        &mut self,
        node: &usvg::Node,
        text: &usvg::Text,
        opacity: f64,
    ) -> Result<(), ConversionError> {
        if text.writing_mode() != usvg::WritingMode::LeftToRight
            || text.dx().iter().any(|value| value.abs() > f32::EPSILON)
            || text.dy().iter().any(|value| value.abs() > f32::EPSILON)
            || text.rotate().iter().any(|value| value.abs() > f32::EPSILON)
        {
            return self.fallback_or_violation(node, "complex SVG text layout", None);
        }
        let mut content = String::new();
        let mut first_span = None;
        let mut align = TextAlign::Left;
        for chunk in text.chunks() {
            content.push_str(chunk.text());
            align = match chunk.anchor() {
                usvg::TextAnchor::Start => TextAlign::Left,
                usvg::TextAnchor::Middle => TextAlign::Center,
                usvg::TextAnchor::End => TextAlign::Right,
            };
            if first_span.is_none() {
                first_span = chunk.spans().first();
            }
            if chunk.spans().len() != 1 {
                return self.fallback_or_violation(node, "multi-style SVG text", None);
            }
        }
        if content.is_empty() {
            return Ok(());
        }
        let span = first_span.ok_or(ConversionError::NormalizationFailed {
            category: "text has no style span",
        })?;
        if !is_exact_target_font(span) {
            self.diagnostics.push(ConversionDiagnostic::new(
                DiagnosticCode::FontSubstituted,
                DiagnosticSeverity::Approximation,
                self.source_order,
                "source text style was mapped to target-compatible Liberation Sans",
            ));
        }
        let color = solid_fill_color(span.fill()).ok_or(ConversionError::NormalizationFailed {
            category: "text paint is not a solid color",
        })?;
        let bbox = text.abs_bounding_box();
        let transform = text.abs_transform();
        let x_scale = f64::from(transform.sx).hypot(f64::from(transform.ky));
        let font_size = f64::from(span.font_size().get()) * x_scale * self.scene_scale;
        let fill_opacity = span
            .fill()
            .map_or(1.0, |fill| f64::from(fill.opacity().get()));
        let style = ElementStyle {
            stroke_color: color,
            background_color: ExcalidrawColor::transparent(),
            stroke_width: 1.0,
            stroke_style: StrokeStyle::Solid,
            opacity: opacity_percent(opacity * fill_opacity),
            roundness: None,
        };
        let base = self.make_base(
            "text",
            f64::from(bbox.x()) * self.scene_scale,
            f64::from(bbox.y()) * self.scene_scale,
            f64::from(bbox.width()) * self.scene_scale,
            f64::from(bbox.height()) * self.scene_scale,
            style,
        )?;
        let font_size = Finite::length(font_size, self.options.limits.max_element_extent())?;
        let line_height = Finite::length(
            self.options.fonts.target_line_height(),
            self.options.limits.max_element_extent(),
        )?;
        self.push_element(ExcalidrawElement::text(
            base,
            font_size,
            content,
            align,
            line_height,
        ))
    }

    fn fallback_or_violation(
        &mut self,
        node: &usvg::Node,
        _reason: &'static str,
        strict_code: Option<DiagnosticCode>,
    ) -> Result<(), ConversionError> {
        if node.abs_layer_bounding_box().is_none() {
            return Ok(());
        }
        if self.options.profile == ConversionProfile::Strict {
            self.diagnostics.push(ConversionDiagnostic::new(
                strict_code.unwrap_or(DiagnosticCode::PaintIslandRasterized),
                DiagnosticSeverity::Fallback,
                self.source_order,
                "painted SVG content is not exactly representable by the strict target profile",
            ));
            return Ok(());
        }
        self.rasterize_node(node)
    }

    fn rasterize_node(&mut self, node: &usvg::Node) -> Result<(), ConversionError> {
        let bbox = node
            .abs_layer_bounding_box()
            .ok_or(ConversionError::RasterizationFailed {
                category: "empty fallback bounds",
            })?;
        let scale = self.options.raster.fallback_scale() * self.scene_scale;
        let width = f64::from(bbox.width()) * scale;
        let height = f64::from(bbox.height()) * scale;
        let pixel_width = checked_pixel_extent(width)?;
        let pixel_height = checked_pixel_extent(height)?;
        let pixels = u64::from(pixel_width)
            .checked_mul(u64::from(pixel_height))
            .ok_or(ConversionError::LimitExceeded {
                resource: LimitResource::RasterPixels,
                limit: self.options.limits.max_raster_pixels_per_island(),
            })?;
        let aggregate = self.reserve_fallback_pixels(pixels)?;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(pixel_width, pixel_height).ok_or(
            ConversionError::RasterizationFailed {
                category: "invalid fallback pixmap dimensions",
            },
        )?;
        let render_scale = raster_scale_to_f32(scale)?;
        let render_transform = resvg::tiny_skia::Transform::from_scale(render_scale, render_scale);
        resvg::render_node(node, render_transform, &mut pixmap.as_mut()).ok_or(
            ConversionError::RasterizationFailed {
                category: "fallback node is not renderable",
            },
        )?;
        let png = pixmap
            .encode_png()
            .map_err(|_| ConversionError::RasterizationFailed {
                category: "PNG encoding",
            })?;
        let file_id = FileId::new(file_id(&png)?);
        let data_url = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&png)
        );
        let additional_bytes = if self.files.contains_key(&file_id) {
            0
        } else {
            data_url.len()
        };
        let new_embedded = self.reserve_embedded_bytes(additional_bytes)?;
        let style = ElementStyle {
            stroke_color: ExcalidrawColor::transparent(),
            background_color: ExcalidrawColor::transparent(),
            stroke_width: 0.0,
            stroke_style: StrokeStyle::Solid,
            opacity: 100,
            roundness: None,
        };
        let base = self.make_base(
            "image",
            f64::from(bbox.x()) * self.scene_scale,
            f64::from(bbox.y()) * self.scene_scale,
            f64::from(bbox.width()) * self.scene_scale,
            f64::from(bbox.height()) * self.scene_scale,
            style,
        )?;
        let element_id = base.id().as_str().to_owned();
        self.files
            .entry(file_id.clone())
            .or_insert_with(|| BinaryFile::png(file_id.clone(), data_url));
        self.push_element(ExcalidrawElement::image(base, file_id))?;
        self.fallback_pixels = aggregate;
        self.embedded_bytes = new_embedded;
        self.diagnostics.push(
            ConversionDiagnostic::new(
                DiagnosticCode::PaintIslandRasterized,
                DiagnosticSeverity::Fallback,
                self.source_order,
                "smallest complete unsupported paint island was rasterized",
            )
            .with_target(&element_id),
        );
        Ok(())
    }

    fn reserve_fallback_pixels(&self, pixels: u64) -> Result<u64, ConversionError> {
        if pixels > self.options.limits.max_raster_pixels_per_island() {
            return Err(ConversionError::LimitExceeded {
                resource: LimitResource::RasterPixels,
                limit: self.options.limits.max_raster_pixels_per_island(),
            });
        }
        let aggregate =
            self.fallback_pixels
                .checked_add(pixels)
                .ok_or(ConversionError::LimitExceeded {
                    resource: LimitResource::RasterPixels,
                    limit: self.options.limits.max_raster_pixels(),
                })?;
        if aggregate > self.options.limits.max_raster_pixels() {
            return Err(ConversionError::LimitExceeded {
                resource: LimitResource::RasterPixels,
                limit: self.options.limits.max_raster_pixels(),
            });
        }
        Ok(aggregate)
    }

    fn reserve_embedded_bytes(&self, bytes: usize) -> Result<usize, ConversionError> {
        let limit_value = self.options.limits.max_embedded_output_bytes();
        let aggregate =
            self.embedded_bytes
                .checked_add(bytes)
                .ok_or(ConversionError::LimitExceeded {
                    resource: LimitResource::EmbeddedBytes,
                    limit: usize_to_u64(limit_value),
                })?;
        if aggregate > limit_value {
            return Err(ConversionError::LimitExceeded {
                resource: LimitResource::EmbeddedBytes,
                limit: usize_to_u64(limit_value),
            });
        }
        Ok(aggregate)
    }

    fn make_base(
        &self,
        role: &str,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        style: ElementStyle,
    ) -> Result<ElementBase, ConversionError> {
        let occurrence =
            u32::try_from(self.elements.len()).map_err(|_| ConversionError::LimitExceeded {
                resource: LimitResource::TargetElements,
                limit: usize_to_u64(self.options.limits.max_target_elements()),
            })?;
        let (id, identity) = element_identity(self.digest, self.source_order, occurrence, role)?;
        let mut base = ElementBase::new(
            id,
            identity,
            x,
            y,
            width,
            height,
            0.0,
            style,
            &self.options.limits,
        )?;
        if self.options.provenance == ProvenanceMode::Compact {
            let (mapping, diagnostic_codes) = if role == "image" {
                ("fallback", vec!["paint-island-rasterized".to_owned()])
            } else {
                ("native", Vec::new())
            };
            base.set_provenance(self.source_order, role, mapping, diagnostic_codes);
        }
        Ok(base)
    }

    fn push_element(&mut self, element: ExcalidrawElement) -> Result<(), ConversionError> {
        if self.elements.len() >= self.options.limits.max_target_elements() {
            return Err(ConversionError::LimitExceeded {
                resource: LimitResource::TargetElements,
                limit: usize_to_u64(self.options.limits.max_target_elements()),
            });
        }
        if !self.element_ids.insert(element.id().to_owned()) {
            return Err(ConversionError::InvalidGeneratedDocument {
                category: "deterministic element ID collision",
            });
        }
        self.elements.push(element);
        Ok(())
    }

    fn reserve_points(&mut self, count: usize) -> Result<(), ConversionError> {
        self.target_points =
            self.target_points
                .checked_add(count)
                .ok_or(ConversionError::LimitExceeded {
                    resource: LimitResource::TargetPoints,
                    limit: usize_to_u64(self.options.limits.max_target_points()),
                })?;
        if self.target_points > self.options.limits.max_target_points() {
            return Err(ConversionError::LimitExceeded {
                resource: LimitResource::TargetPoints,
                limit: usize_to_u64(self.options.limits.max_target_points()),
            });
        }
        Ok(())
    }
}

fn path_has_only_solid_paint(path: &usvg::Path) -> bool {
    path.fill()
        .is_none_or(|fill| matches!(fill.paint(), usvg::Paint::Color(_)))
        && path
            .stroke()
            .is_none_or(|stroke| matches!(stroke.paint(), usvg::Paint::Color(_)))
}

fn stroke_transform_is_scalar(path: &usvg::Path) -> bool {
    if path.stroke().is_none() {
        return true;
    }
    let transform = path.abs_transform();
    let x_scale = f64::from(transform.sx).hypot(f64::from(transform.ky));
    let y_scale = f64::from(transform.kx).hypot(f64::from(transform.sy));
    (x_scale - y_scale).abs() <= 1.0e-6 && x_scale.is_finite() && x_scale > 0.0
}

fn stroke_is_exact_native(path: &usvg::Path) -> bool {
    path.stroke().is_none_or(|stroke| {
        stroke.dasharray().is_none()
            && stroke.dashoffset() == 0.0
            && stroke.linecap() == usvg::LineCap::Round
            && stroke.linejoin() == usvg::LineJoin::Round
    })
}

fn is_exact_target_font(span: &usvg::TextSpan) -> bool {
    matches!(
        span.font().families().first(),
        Some(usvg::FontFamily::Named(name)) if name.eq_ignore_ascii_case("Liberation Sans")
    ) && span.font().style() == usvg::FontStyle::Normal
        && span.font().stretch() == usvg::FontStretch::Normal
        && span.font().weight() == 400
        && !span.small_caps()
        && span.letter_spacing() == 0.0
        && span.word_spacing() == 0.0
}

fn paint_roles(path: &usvg::Path, can_combine: bool) -> Vec<PaintRole> {
    match (path.fill(), path.stroke()) {
        (Some(fill), Some(stroke))
            if can_combine
                && path.paint_order() == usvg::PaintOrder::FillAndStroke
                && (f64::from(fill.opacity().get()) - f64::from(stroke.opacity().get())).abs()
                    <= 0.005 =>
        {
            vec![PaintRole::Combined]
        }
        (Some(_), Some(_)) => match path.paint_order() {
            usvg::PaintOrder::FillAndStroke => vec![PaintRole::Fill, PaintRole::Stroke],
            usvg::PaintOrder::StrokeAndFill => vec![PaintRole::Stroke, PaintRole::Fill],
        },
        (Some(_), None) => vec![PaintRole::Fill],
        (None, Some(_)) => vec![PaintRole::Stroke],
        (None, None) => Vec::new(),
    }
}

fn is_simple_fill_boundary(subpath: &Subpath, options: &ConversionOptions) -> bool {
    const INTERSECTION_WORK_CAP: usize = 4_000_000;

    let points = &subpath.points;
    if points.len() < 3 || points_are_collinear(points) {
        return true;
    }
    let segment_count = points.len();
    if segment_count
        .checked_mul(segment_count)
        .is_none_or(|work| work > INTERSECTION_WORK_CAP)
        || segment_count > options.limits.max_decomposition_elements()
    {
        return false;
    }
    let Some(first) = points.first().copied() else {
        return false;
    };
    let Some(last) = points.last().copied() else {
        return false;
    };
    let edges = points
        .windows(2)
        .filter_map(|pair| Some((*pair.first()?, *pair.get(1)?)))
        .chain(std::iter::once((last, first)))
        .collect::<Vec<_>>();
    for (left, (left_start, left_end)) in edges.iter().copied().enumerate() {
        for right in (left + 1)..edges.len() {
            if right == left + 1 || (left == 0 && right + 1 == segment_count) {
                continue;
            }
            let Some((right_start, right_end)) = edges.get(right).copied() else {
                return false;
            };
            if segments_intersect(left_start, left_end, right_start, right_end) {
                return false;
            }
        }
    }
    true
}

fn points_are_collinear(points: &[Point]) -> bool {
    let Some(first) = points.first().copied() else {
        return true;
    };
    let Some(second) = points
        .iter()
        .copied()
        .find(|point| !approximately_same(first, *point))
    else {
        return true;
    };
    points
        .iter()
        .copied()
        .all(|point| orientation(first, second, point).abs() <= 1.0e-9)
}

fn segments_intersect(a: Point, b: Point, c: Point, d: Point) -> bool {
    let ab_c = orientation(a, b, c);
    let ab_d = orientation(a, b, d);
    let cd_a = orientation(c, d, a);
    let cd_b = orientation(c, d, b);
    let epsilon = 1.0e-9;

    if ab_c.abs() <= epsilon && point_on_segment(c, a, b, epsilon)
        || ab_d.abs() <= epsilon && point_on_segment(d, a, b, epsilon)
        || cd_a.abs() <= epsilon && point_on_segment(a, c, d, epsilon)
        || cd_b.abs() <= epsilon && point_on_segment(b, c, d, epsilon)
    {
        return true;
    }
    (ab_c > epsilon) != (ab_d > epsilon) && (cd_a > epsilon) != (cd_b > epsilon)
}

fn orientation(a: Point, b: Point, c: Point) -> f64 {
    (b.x - a.x).mul_add(c.y - a.y, -((b.y - a.y) * (c.x - a.x)))
}

fn point_on_segment(point: Point, start: Point, end: Point, epsilon: f64) -> bool {
    point.x >= start.x.min(end.x) - epsilon
        && point.x <= start.x.max(end.x) + epsilon
        && point.y >= start.y.min(end.y) - epsilon
        && point.y <= start.y.max(end.y) + epsilon
}

fn path_style(
    path: &usvg::Path,
    role: PaintRole,
    inherited_opacity: f64,
    scene_scale: f64,
) -> ElementStyle {
    let fill = if matches!(role, PaintRole::Combined | PaintRole::Fill) {
        path.fill()
    } else {
        None
    };
    let stroke = if matches!(role, PaintRole::Combined | PaintRole::Stroke) {
        path.stroke()
    } else {
        None
    };
    let background_color = fill
        .and_then(|paint| solid_paint_color(paint.paint()))
        .unwrap_or_else(ExcalidrawColor::transparent);
    let stroke_color = stroke
        .and_then(|paint| solid_paint_color(paint.paint()))
        .unwrap_or_else(ExcalidrawColor::transparent);
    let transform = path.abs_transform();
    let transform_scale = f64::from(transform.sx).hypot(f64::from(transform.ky));
    let stroke_width = stroke.map_or(0.0, |paint| {
        f64::from(paint.width().get()) * transform_scale * scene_scale
    });
    let alpha = fill
        .map(|paint| f64::from(paint.opacity().get()))
        .or_else(|| stroke.map(|paint| f64::from(paint.opacity().get())))
        .unwrap_or(1.0);
    ElementStyle {
        stroke_color,
        background_color,
        stroke_width,
        stroke_style: classify_dash(stroke),
        opacity: opacity_percent(inherited_opacity * alpha),
        roundness: None,
    }
}

fn classify_dash(stroke: Option<&usvg::Stroke>) -> StrokeStyle {
    let Some(stroke) = stroke else {
        return StrokeStyle::Solid;
    };
    let Some(dashes) = stroke.dasharray() else {
        return StrokeStyle::Solid;
    };
    if stroke.linecap() == usvg::LineCap::Round
        && dashes
            .iter()
            .step_by(2)
            .all(|dash| *dash <= stroke.width().get())
    {
        StrokeStyle::Dotted
    } else {
        StrokeStyle::Dashed
    }
}

fn solid_paint_color(paint: &usvg::Paint) -> Option<ExcalidrawColor> {
    match paint {
        usvg::Paint::Color(color) => Some(ExcalidrawColor::rgb(color.red, color.green, color.blue)),
        _ => None,
    }
}

fn solid_fill_color(fill: Option<&usvg::Fill>) -> Option<ExcalidrawColor> {
    fill.and_then(|paint| solid_paint_color(paint.paint()))
}

fn opacity_percent(opacity: f64) -> u8 {
    let percent = (opacity.clamp(0.0, 1.0) * 100.0).round_ties_even();
    if percent <= 0.0 {
        0
    } else if percent >= 100.0 {
        100
    } else {
        bounded_percent_to_u8(percent)
    }
}

fn looks_like_axis_aligned_ellipse(path: &usvg::Path) -> bool {
    let mut moves = 0_usize;
    let mut cubics = 0_usize;
    let mut closes = 0_usize;
    let mut others = 0_usize;
    for segment in path.data().segments() {
        match segment {
            PathSegment::MoveTo(_) => moves = moves.saturating_add(1),
            PathSegment::CubicTo(..) => cubics = cubics.saturating_add(1),
            PathSegment::Close => closes = closes.saturating_add(1),
            PathSegment::LineTo(_) | PathSegment::QuadTo(..) => {
                others = others.saturating_add(1);
            }
        }
    }
    moves == 1 && cubics == 4 && closes == 1 && others == 0 && !path.abs_transform().has_skew()
}

fn flatten_path(
    path: &usvg::Path,
    tolerance: f64,
    scene_scale: f64,
    max_points: usize,
) -> Result<Vec<Subpath>, ConversionError> {
    let transform = path.abs_transform();
    let mut output = Vec::new();
    let mut current: Option<Subpath> = None;
    for segment in path.data().segments() {
        match segment {
            PathSegment::MoveTo(point) => {
                finish_subpath(&mut current, &mut output);
                current = Some(Subpath {
                    points: vec![Point::transformed(point, transform, scene_scale)],
                    closed: false,
                    curved: false,
                });
            }
            PathSegment::LineTo(point) => {
                let point = Point::transformed(point, transform, scene_scale);
                push_distinct(current.as_mut(), point, max_points)?;
            }
            PathSegment::QuadTo(control, end) => {
                let subpath = current.as_mut().ok_or(ConversionError::GeometryOverflow)?;
                let start = *subpath
                    .points
                    .last()
                    .ok_or(ConversionError::GeometryOverflow)?;
                let control = Point::transformed(control, transform, scene_scale);
                let end = Point::transformed(end, transform, scene_scale);
                flatten_quad(
                    start,
                    control,
                    end,
                    tolerance,
                    0,
                    &mut subpath.points,
                    max_points,
                )?;
                subpath.curved = true;
            }
            PathSegment::CubicTo(control1, control2, end) => {
                let subpath = current.as_mut().ok_or(ConversionError::GeometryOverflow)?;
                let start = *subpath
                    .points
                    .last()
                    .ok_or(ConversionError::GeometryOverflow)?;
                let control1 = Point::transformed(control1, transform, scene_scale);
                let control2 = Point::transformed(control2, transform, scene_scale);
                let end = Point::transformed(end, transform, scene_scale);
                flatten_cubic(
                    start,
                    control1,
                    control2,
                    end,
                    tolerance,
                    0,
                    &mut subpath.points,
                    max_points,
                )?;
                subpath.curved = true;
            }
            PathSegment::Close => {
                if let Some(subpath) = current.as_mut() {
                    subpath.closed = true;
                }
            }
        }
    }
    finish_subpath(&mut current, &mut output);
    Ok(output)
}

fn finish_subpath(current: &mut Option<Subpath>, output: &mut Vec<Subpath>) {
    if let Some(mut subpath) = current.take() {
        deduplicate_points(&mut subpath.points);
        if subpath.closed
            && let (Some(first), Some(last)) = (subpath.points.first(), subpath.points.last())
            && approximately_same(*first, *last)
        {
            subpath.points.pop();
        }
        if !subpath.points.is_empty() {
            output.push(subpath);
        }
    }
}

fn push_distinct(
    subpath: Option<&mut Subpath>,
    point: Point,
    max_points: usize,
) -> Result<(), ConversionError> {
    let subpath = subpath.ok_or(ConversionError::GeometryOverflow)?;
    if subpath
        .points
        .last()
        .is_none_or(|last| !approximately_same(*last, point))
    {
        push_bounded(&mut subpath.points, point, max_points)?;
    }
    Ok(())
}

fn flatten_quad(
    start: Point,
    control: Point,
    end: Point,
    tolerance: f64,
    depth: u8,
    output: &mut Vec<Point>,
    max_points: usize,
) -> Result<(), ConversionError> {
    if distance_to_line(control, start, end) <= tolerance || depth >= 20 {
        return push_bounded(output, end, max_points);
    }
    let start_control = start.midpoint(control);
    let control_end = control.midpoint(end);
    let middle = start_control.midpoint(control_end);
    flatten_quad(
        start,
        start_control,
        middle,
        tolerance,
        depth + 1,
        output,
        max_points,
    )?;
    flatten_quad(
        middle,
        control_end,
        end,
        tolerance,
        depth + 1,
        output,
        max_points,
    )
}

#[allow(clippy::too_many_arguments)]
fn flatten_cubic(
    start: Point,
    control1: Point,
    control2: Point,
    end: Point,
    tolerance: f64,
    depth: u8,
    output: &mut Vec<Point>,
    max_points: usize,
) -> Result<(), ConversionError> {
    let flatness =
        distance_to_line(control1, start, end).max(distance_to_line(control2, start, end));
    if flatness <= tolerance || depth >= 20 {
        return push_bounded(output, end, max_points);
    }
    let left_edge = start.midpoint(control1);
    let center_edge = control1.midpoint(control2);
    let right_edge = control2.midpoint(end);
    let left_half = left_edge.midpoint(center_edge);
    let right_half = center_edge.midpoint(right_edge);
    let middle = left_half.midpoint(right_half);
    flatten_cubic(
        start,
        left_edge,
        left_half,
        middle,
        tolerance,
        depth + 1,
        output,
        max_points,
    )?;
    flatten_cubic(
        middle,
        right_half,
        right_edge,
        end,
        tolerance,
        depth + 1,
        output,
        max_points,
    )
}

fn push_bounded(
    output: &mut Vec<Point>,
    point: Point,
    max_points: usize,
) -> Result<(), ConversionError> {
    if !point.x.is_finite() || !point.y.is_finite() {
        return Err(ConversionError::GeometryOverflow);
    }
    if output.len() >= max_points {
        return Err(ConversionError::LimitExceeded {
            resource: LimitResource::TargetPoints,
            limit: usize_to_u64(max_points),
        });
    }
    output.push(point);
    Ok(())
}

fn distance_to_line(point: Point, start: Point, end: Point) -> f64 {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let denominator = dx.hypot(dy);
    if denominator <= f64::EPSILON {
        return (point.x - start.x).hypot(point.y - start.y);
    }
    ((dy * point.x - dx * point.y + end.x * start.y - end.y * start.x).abs()) / denominator
}

fn deduplicate_points(points: &mut Vec<Point>) {
    points.dedup_by(|left, right| approximately_same(*left, *right));
}

fn approximately_same(left: Point, right: Point) -> bool {
    (left.x - right.x).abs() <= 1.0e-9 && (left.y - right.y).abs() <= 1.0e-9
}

fn bounds(points: &[Point]) -> Result<(f64, f64, f64, f64), ConversionError> {
    let first = points.first().ok_or(ConversionError::GeometryOverflow)?;
    let mut min_x = first.x;
    let mut min_y = first.y;
    let mut max_x = first.x;
    let mut max_y = first.y;
    for point in points.iter().skip(1) {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }
    if [min_x, min_y, max_x, max_y]
        .iter()
        .any(|value| !value.is_finite())
    {
        return Err(ConversionError::GeometryOverflow);
    }
    Ok((min_x, min_y, max_x - min_x, max_y - min_y))
}

fn is_axis_aligned_rectangle(points: &[Point]) -> bool {
    if points.len() != 4 {
        return false;
    }
    let Ok((min_x, min_y, width, height)) = bounds(points) else {
        return false;
    };
    if width <= 0.0 || height <= 0.0 {
        return false;
    }
    let corners = [
        (min_x.to_bits(), min_y.to_bits()),
        ((min_x + width).to_bits(), min_y.to_bits()),
        ((min_x + width).to_bits(), (min_y + height).to_bits()),
        (min_x.to_bits(), (min_y + height).to_bits()),
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    points
        .iter()
        .map(|point| (point.x.to_bits(), point.y.to_bits()))
        .collect::<BTreeSet<_>>()
        == corners
}

fn is_target_diamond(points: &[Point]) -> bool {
    if points.len() != 4 {
        return false;
    }
    let Ok((min_x, min_y, width, height)) = bounds(points) else {
        return false;
    };
    let expected = [
        ((min_x + width * 0.5).to_bits(), min_y.to_bits()),
        ((min_x + width).to_bits(), (min_y + height * 0.5).to_bits()),
        ((min_x + width * 0.5).to_bits(), (min_y + height).to_bits()),
        (min_x.to_bits(), (min_y + height * 0.5).to_bits()),
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    points
        .iter()
        .map(|point| (point.x.to_bits(), point.y.to_bits()))
        .collect::<BTreeSet<_>>()
        == expected
}

fn checked_pixel_extent(value: f64) -> Result<u32, ConversionError> {
    let value = value.ceil().max(1.0);
    if !value.is_finite() || value > f64::from(u32::MAX) {
        return Err(ConversionError::GeometryOverflow);
    }
    Ok(bounded_pixel_extent_to_u32(value))
}

#[allow(clippy::cast_possible_truncation)]
fn raster_scale_to_f32(value: f64) -> Result<f32, ConversionError> {
    if !value.is_finite() || !(0.0..=4.0).contains(&value) {
        return Err(ConversionError::GeometryOverflow);
    }
    // resvg's transform is intentionally f32; policy has already bounded this value to 0..=4.
    Ok(value as f32)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn bounded_percent_to_u8(value: f64) -> u8 {
    debug_assert!((0.0..=100.0).contains(&value));
    value as u8
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn bounded_pixel_extent_to_u32(value: f64) -> u32 {
    debug_assert!((1.0..=f64::from(u32::MAX)).contains(&value));
    value as u32
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use crate::{ConversionOptions, convert};

    #[test]
    fn test_should_convert_basic_shapes_to_stable_json() -> Result<(), Box<dyn std::error::Error>> {
        let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50">
          <rect x="1" y="2" width="20" height="10" fill="#ff0000"/>
          <circle cx="50" cy="20" r="10" fill="#00ff00"/>
          <line x1="70" y1="5" x2="90" y2="25" stroke="#0000ff"/>
        </svg>"##;
        let first = convert(input, &ConversionOptions::default())?;
        let second = convert(input, &ConversionOptions::default())?;
        assert_eq!(
            first.document.to_pretty_json()?,
            second.document.to_pretty_json()?
        );
        assert_eq!(first.document.elements().len(), 3);
        Ok(())
    }
}
