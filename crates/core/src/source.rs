//! Bounded source-semantic anchors for correlating the normalized paint tree.

use std::collections::{BTreeMap, BTreeSet};

use svgtypes::{PathParser, PathSegment};

use crate::{ConversionError, ConversionLimits, target::Arrowhead};

const SYNTHETIC_ID_PREFIX: &str = "s2e-anchor";

#[derive(Debug, Clone, Copy)]
pub(crate) struct DropShadow {
    pub(crate) balanced_omittable: bool,
    pub(crate) editable_omittable: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct SourceNodeMetadata {
    pub(crate) order: u32,
    pub(crate) tag: String,
    pub(crate) explicit_group: bool,
    pub(crate) marker_declared: bool,
    pub(crate) marker_fully_recognized: bool,
    pub(crate) marker_start: Option<Arrowhead>,
    pub(crate) marker_end: Option<Arrowhead>,
    pub(crate) marker_start_color: Option<[u8; 3]>,
    pub(crate) marker_end_color: Option<[u8; 3]>,
    pub(crate) rect_radius: Option<(f64, f64)>,
    pub(crate) drop_shadow: Option<DropShadow>,
}

#[derive(Debug, Default)]
pub(crate) struct SourceMetadata {
    nodes: BTreeMap<String, SourceNodeMetadata>,
}

impl SourceMetadata {
    pub(crate) fn len(&self) -> usize {
        self.nodes.len()
    }

    pub(crate) fn node(&self, id: &str) -> Option<&SourceNodeMetadata> {
        self.nodes.get(id)
    }

    pub(crate) fn has_marker(&self, id: &str) -> bool {
        self.node(id)
            .is_some_and(|node| node.marker_declared && node.marker_fully_recognized)
    }
}

#[derive(Debug)]
pub(crate) struct PreparedSource {
    pub(crate) xml: String,
    pub(crate) metadata: SourceMetadata,
}

#[derive(Debug, Clone, Copy)]
struct MarkerDefinition {
    arrowhead: Arrowhead,
    color: [u8; 3],
    supports_start: bool,
    supports_end: bool,
}

pub(crate) fn prepare_source(
    text: &str,
    document: &usvg::roxmltree::Document<'_>,
    limits: &ConversionLimits,
) -> Result<PreparedSource, ConversionError> {
    let id_counts = source_id_counts(document);
    let marker_definitions = marker_definitions(document, &id_counts);
    let shadow_definitions = shadow_definitions(document, &id_counts);
    let injection_safe = !document.descendants().any(|node| {
        node.is_text()
            && node
                .parent_element()
                .is_some_and(|parent| parent.tag_name().name() == "style")
            && stylesheet_observes_id_presence(node.text().unwrap_or_default())
    });
    let mut reserved_ids = id_counts.keys().cloned().collect::<BTreeSet<_>>();
    let mut insertions = Vec::<(usize, String)>::new();
    let mut nodes = BTreeMap::new();

    for (index, node) in document
        .descendants()
        .filter(usvg::roxmltree::Node::is_element)
        .enumerate()
    {
        let order = u32::try_from(index).map_err(|_| ConversionError::LimitExceeded {
            resource: crate::LimitResource::XmlElements,
            limit: usize_to_u64(limits.max_xml_elements()),
        })?;
        let existing_id = node.attribute("id");
        let anchor = existing_id
            .filter(|id| id_counts.get(*id).copied() == Some(1))
            .map(str::to_owned)
            .or_else(|| {
                if !injection_safe || existing_id.is_some() {
                    return None;
                }
                let candidate = synthetic_id(order);
                if reserved_ids.insert(candidate.clone()) {
                    Some(candidate)
                } else {
                    None
                }
            });
        let Some(anchor) = anchor else {
            continue;
        };
        if existing_id.is_none() {
            let position = start_tag_name_end(text, node.range().start)?;
            insertions.push((position, format!(" id=\"{anchor}\"")));
        }
        let tag = node.tag_name().name();
        let marker_start = node
            .attribute("marker-start")
            .and_then(fragment_url)
            .and_then(|id| marker_definitions.get(id).copied())
            .filter(|marker| marker.supports_start);
        let marker_end = node
            .attribute("marker-end")
            .and_then(fragment_url)
            .and_then(|id| marker_definitions.get(id).copied())
            .filter(|marker| marker.supports_end);
        let marker_start_declared = node.attribute("marker-start").is_some();
        let marker_end_declared = node.attribute("marker-end").is_some();
        let drop_shadow = node
            .attribute("filter")
            .and_then(fragment_url)
            .and_then(|id| shadow_definitions.get(id).copied());
        nodes.insert(
            anchor,
            SourceNodeMetadata {
                order,
                tag: tag.to_owned(),
                explicit_group: matches!(tag, "g" | "use"),
                marker_declared: marker_start_declared || marker_end_declared,
                marker_fully_recognized: (!marker_start_declared || marker_start.is_some())
                    && (!marker_end_declared || marker_end.is_some()),
                marker_start: marker_start.map(|marker| marker.arrowhead),
                marker_end: marker_end.map(|marker| marker.arrowhead),
                marker_start_color: marker_start.map(|marker| marker.color),
                marker_end_color: marker_end.map(|marker| marker.color),
                rect_radius: (tag == "rect").then(|| rect_radius(node)).flatten(),
                drop_shadow,
            },
        );
    }

    Ok(PreparedSource {
        xml: apply_insertions(text, &insertions)?,
        metadata: SourceMetadata { nodes },
    })
}

fn source_id_counts(document: &usvg::roxmltree::Document<'_>) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for id in document
        .descendants()
        .filter(usvg::roxmltree::Node::is_element)
        .filter_map(|node| node.attribute("id"))
    {
        *counts.entry(id.to_owned()).or_default() += 1;
    }
    counts
}

fn synthetic_id(order: u32) -> String {
    format!("{SYNTHETIC_ID_PREFIX}-{order:08x}")
}

fn start_tag_name_end(text: &str, start: usize) -> Result<usize, ConversionError> {
    let bytes = text.as_bytes();
    let mut position = start
        .checked_add(1)
        .ok_or(ConversionError::GeometryOverflow)?;
    while let Some(byte) = bytes.get(position) {
        if byte.is_ascii_whitespace() || matches!(*byte, b'/' | b'>') {
            return Ok(position);
        }
        position = position
            .checked_add(1)
            .ok_or(ConversionError::GeometryOverflow)?;
    }
    Err(ConversionError::MalformedXml {
        category: "unterminated start tag",
        line: 1,
        column: 1,
    })
}

fn apply_insertions(text: &str, insertions: &[(usize, String)]) -> Result<String, ConversionError> {
    let added = insertions
        .iter()
        .try_fold(0_usize, |total, (_, insertion)| {
            total
                .checked_add(insertion.len())
                .ok_or(ConversionError::GeometryOverflow)
        })?;
    let capacity = text
        .len()
        .checked_add(added)
        .ok_or(ConversionError::GeometryOverflow)?;
    let mut output = String::with_capacity(capacity);
    let mut cursor = 0_usize;
    for (position, insertion) in insertions {
        let chunk = text
            .get(cursor..*position)
            .ok_or(ConversionError::GeometryOverflow)?;
        output.push_str(chunk);
        output.push_str(insertion);
        cursor = *position;
    }
    output.push_str(
        text.get(cursor..)
            .ok_or(ConversionError::GeometryOverflow)?,
    );
    Ok(output)
}

fn stylesheet_observes_id_presence(stylesheet: &str) -> bool {
    let lowercase = stylesheet.to_ascii_lowercase();
    lowercase.contains(SYNTHETIC_ID_PREFIX) || lowercase.contains('[')
}

fn fragment_url(value: &str) -> Option<&str> {
    value
        .trim()
        .strip_prefix("url(")?
        .strip_suffix(')')?
        .trim()
        .trim_matches(|character| matches!(character, '\'' | '"'))
        .strip_prefix('#')
}

fn rect_radius(node: usvg::roxmltree::Node<'_, '_>) -> Option<(f64, f64)> {
    let rx = node.attribute("rx").and_then(parse_length);
    let ry = node.attribute("ry").and_then(parse_length);
    match (rx, ry) {
        (Some(x), Some(y)) => Some((x, y)),
        (Some(value), None) | (None, Some(value)) => Some((value, value)),
        (None, None) => None,
    }
}

fn parse_length(value: &str) -> Option<f64> {
    let parsed = parse_number(value)?;
    (parsed >= 0.0).then_some(parsed)
}

fn parse_number(value: &str) -> Option<f64> {
    let value = value.trim().strip_suffix("px").unwrap_or(value.trim());
    value
        .parse::<f64>()
        .ok()
        .filter(|parsed| parsed.is_finite())
}

fn marker_definitions(
    document: &usvg::roxmltree::Document<'_>,
    id_counts: &BTreeMap<String, usize>,
) -> BTreeMap<String, MarkerDefinition> {
    document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "marker")
        .filter_map(|marker| {
            let id = marker.attribute("id")?;
            (id_counts.get(id).copied() == Some(1))
                .then(|| classify_marker(marker).map(|definition| (id.to_owned(), definition)))?
        })
        .collect()
}

fn classify_marker(marker: usvg::roxmltree::Node<'_, '_>) -> Option<MarkerDefinition> {
    let (supports_start, supports_end) = match marker.attribute("orient").unwrap_or("0") {
        "auto-start-reverse" => (true, true),
        "auto" => (false, true),
        _ => return None,
    };
    if has_unexpected_attributes(
        marker,
        &[
            "id",
            "viewBox",
            "refX",
            "refY",
            "markerWidth",
            "markerHeight",
            "orient",
            "markerUnits",
        ],
    ) || marker
        .attribute("markerUnits")
        .is_some_and(|value| value != "strokeWidth")
        || !number_list_matches(marker.attribute("viewBox")?, &[0.0, 0.0, 10.0, 10.0])
        || !number_matches(marker.attribute("refX")?, 8.5)
        || !number_matches(marker.attribute("refY")?, 5.0)
        || !number_matches(marker.attribute("markerWidth")?, 6.5)
        || !number_matches(marker.attribute("markerHeight")?, 6.5)
        || marker.attribute("preserveAspectRatio").is_some()
        || marker.attribute("class").is_some()
        || marker.attribute("style").is_some()
    {
        return None;
    }
    let mut children = marker.children().filter(usvg::roxmltree::Node::is_element);
    let child = children.next()?;
    if children.next().is_some() {
        return None;
    }
    if child.tag_name().name() != "path"
        || has_unexpected_attributes(child, &["d", "fill"])
        || child.attribute("class").is_some()
        || child.attribute("style").is_some()
        || child.attribute("transform").is_some()
        || child.attribute("stroke").is_some()
        || child.attribute("filter").is_some()
        || child.attribute("mask").is_some()
        || child.attribute("clip-path").is_some()
        || child.attribute("opacity").is_some()
        || child.attribute("fill-opacity").is_some()
    {
        return None;
    }
    let color = child
        .attribute("fill")
        .map_or(Some([0, 0, 0]), parse_hex_color)?;
    let (points, closed) = linear_path_points(child.attribute("d")?)?;
    if !closed || !triangle_marker_matches(&points) {
        return None;
    }
    Some(MarkerDefinition {
        arrowhead: Arrowhead::Triangle,
        color,
        supports_start,
        supports_end,
    })
}

fn triangle_marker_matches(points: &[(f64, f64)]) -> bool {
    const EXPECTED: [(f64, f64); 3] = [(0.0, 0.6), (9.5, 5.0), (0.0, 9.4)];
    points.len() == EXPECTED.len()
        && points.iter().zip(EXPECTED).all(|(actual, expected)| {
            (actual.0 - expected.0).abs() <= 0.05 && (actual.1 - expected.1).abs() <= 0.05
        })
}

fn number_matches(value: &str, expected: f64) -> bool {
    parse_number(value).is_some_and(|actual| (actual - expected).abs() <= 1.0e-6)
}

fn number_list_matches(value: &str, expected: &[f64]) -> bool {
    let actual = value
        .split(|character: char| character.is_ascii_whitespace() || character == ',')
        .filter(|part| !part.is_empty())
        .map(parse_number)
        .collect::<Option<Vec<_>>>();
    actual.is_some_and(|actual| {
        actual.len() == expected.len()
            && actual
                .iter()
                .zip(expected)
                .all(|(left, right)| (left - right).abs() <= 1.0e-6)
    })
}

fn parse_hex_color(value: &str) -> Option<[u8; 3]> {
    let digits = value.trim().strip_prefix('#')?;
    match digits.len() {
        3 => {
            let mut bytes = digits.bytes();
            let red = hex_digit(bytes.next()?)?;
            let green = hex_digit(bytes.next()?)?;
            let blue = hex_digit(bytes.next()?)?;
            Some([red * 17, green * 17, blue * 17])
        }
        6 => {
            let bytes = digits.as_bytes();
            Some([
                hex_digit(*bytes.first()?)? * 16 + hex_digit(*bytes.get(1)?)?,
                hex_digit(*bytes.get(2)?)? * 16 + hex_digit(*bytes.get(3)?)?,
                hex_digit(*bytes.get(4)?)? * 16 + hex_digit(*bytes.get(5)?)?,
            ])
        }
        _ => None,
    }
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn has_unexpected_attributes(node: usvg::roxmltree::Node<'_, '_>, allowed: &[&str]) -> bool {
    node.attributes()
        .any(|attribute| !allowed.contains(&attribute.name()))
}

fn linear_path_points(data: &str) -> Option<(Vec<(f64, f64)>, bool)> {
    let mut points = Vec::new();
    let mut current = (0.0_f64, 0.0_f64);
    let mut closed = false;
    for segment in PathParser::from(data) {
        match segment.ok()? {
            PathSegment::MoveTo { abs, x, y } | PathSegment::LineTo { abs, x, y } => {
                current = resolve_point(current, abs, x, y);
                points.push(current);
            }
            PathSegment::HorizontalLineTo { abs, x } => {
                current.0 = if abs { x } else { current.0 + x };
                points.push(current);
            }
            PathSegment::VerticalLineTo { abs, y } => {
                current.1 = if abs { y } else { current.1 + y };
                points.push(current);
            }
            PathSegment::ClosePath { .. } => closed = true,
            PathSegment::CurveTo { .. }
            | PathSegment::SmoothCurveTo { .. }
            | PathSegment::Quadratic { .. }
            | PathSegment::SmoothQuadratic { .. }
            | PathSegment::EllipticalArc { .. } => return None,
        }
        if !current.0.is_finite() || !current.1.is_finite() {
            return None;
        }
    }
    points.dedup();
    Some((points, closed))
}

fn resolve_point(current: (f64, f64), absolute: bool, x: f64, y: f64) -> (f64, f64) {
    if absolute {
        (x, y)
    } else {
        (current.0 + x, current.1 + y)
    }
}

fn shadow_definitions(
    document: &usvg::roxmltree::Document<'_>,
    id_counts: &BTreeMap<String, usize>,
) -> BTreeMap<String, DropShadow> {
    document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "filter")
        .filter_map(|filter| {
            let id = filter.attribute("id")?;
            (id_counts.get(id).copied() == Some(1))
                .then(|| classify_shadow(filter).map(|shadow| (id.to_owned(), shadow)))?
        })
        .collect()
}

fn classify_shadow(filter: usvg::roxmltree::Node<'_, '_>) -> Option<DropShadow> {
    if has_unexpected_attributes(
        filter,
        &[
            "id",
            "x",
            "y",
            "width",
            "height",
            "filterUnits",
            "primitiveUnits",
            "color-interpolation-filters",
        ],
    ) || filter.attribute("class").is_some()
        || filter.attribute("style").is_some()
        || filter
            .attribute("color-interpolation-filters")
            .is_some_and(|value| value != "sRGB")
        || filter
            .attribute("filterUnits")
            .is_some_and(|value| value != "objectBoundingBox")
        || filter
            .attribute("primitiveUnits")
            .is_some_and(|value| value != "userSpaceOnUse")
    {
        return None;
    }
    let mut children = filter.children().filter(usvg::roxmltree::Node::is_element);
    let shadow = children.next()?;
    if shadow.tag_name().name() != "feDropShadow"
        || has_unexpected_attributes(
            shadow,
            &[
                "in",
                "dx",
                "dy",
                "stdDeviation",
                "flood-color",
                "flood-opacity",
            ],
        )
        || children.next().is_some()
        || shadow.attribute("class").is_some()
        || shadow.attribute("style").is_some()
        || shadow
            .attribute("in")
            .is_some_and(|value| value != "SourceGraphic")
        || shadow.attribute("result").is_some()
        || shadow.attribute("color-interpolation-filters").is_some()
        || shadow
            .attribute("flood-color")
            .and_then(parse_hex_color)
            .is_none()
    {
        return None;
    }
    let dx = shadow.attribute("dx").and_then(parse_number).unwrap_or(0.0);
    let dy = shadow.attribute("dy").and_then(parse_number).unwrap_or(0.0);
    let deviations = shadow
        .attribute("stdDeviation")
        .unwrap_or("0")
        .split_ascii_whitespace()
        .filter_map(parse_length)
        .collect::<Vec<_>>();
    let deviation_x = deviations.first().copied()?;
    let deviation_y = deviations.get(1).copied().unwrap_or(deviation_x);
    let opacity = shadow
        .attribute("flood-opacity")
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(1.0);
    if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
        return None;
    }
    Some(DropShadow {
        balanced_omittable: opacity <= 0.15
            && deviation_x <= 4.0
            && deviation_y <= 4.0
            && dx.abs() <= 4.0
            && dy.abs() <= 4.0,
        editable_omittable: opacity <= 0.25
            && deviation_x <= 8.0
            && deviation_y <= 8.0
            && dx.abs() <= 8.0
            && dy.abs() <= 8.0,
    })
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::{linear_path_points, stylesheet_observes_id_presence};

    #[test]
    fn test_should_recognize_linear_closed_marker_path() {
        let parsed = linear_path_points("M0 0.6 L9.5 5 L0 9.4 z");
        assert!(matches!(parsed, Some((points, true)) if points.len() == 3));
    }

    #[test]
    fn test_should_detect_id_attribute_selectors() {
        assert!(stylesheet_observes_id_presence(
            "g[ id ^= 'source' ] { fill: red }"
        ));
        assert!(!stylesheet_observes_id_presence("#id { fill: red }"));
        assert!(stylesheet_observes_id_presence("[i\\64] { fill: red }"));
    }
}
