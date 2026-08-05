//! Reproducible Phase 0 characterization for the canonical architecture fixture.

// This synchronous developer probe intentionally uses blocking filesystem I/O.
#![allow(clippy::disallowed_methods)]

use std::{collections::BTreeMap, error::Error, fs, path::Path};

use serde::Serialize;
use usvg::tiny_skia_path::PathSegment;

const INPUT: &str = "fixtures/arch.svg";
const OUTPUT: &str = "fixtures/baselines/arch-usvg-characterization.json";
const PNG_1X: &str = "fixtures/baselines/arch-resvg-1x.png";
const PNG_2X: &str = "fixtures/baselines/arch-resvg-2x.png";
const LIBERATION_SANS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendors/excalidraw/scripts/woff2/assets/LiberationSans-Regular.ttf"
));

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct NormalizedCounts {
    groups: u64,
    filtered_groups: u64,
    paths: u64,
    line_only_paths: u64,
    curved_paths: u64,
    texts: u64,
    images: u64,
    solid_fills: u64,
    solid_strokes: u64,
    paint_servers: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Characterization {
    fixture_bytes: usize,
    width: f32,
    height: f32,
    source_elements: BTreeMap<String, u64>,
    source_max_depth: usize,
    normalized: NormalizedCounts,
    css_presentation_conflict_resolved_fill: String,
    bundled_font: &'static str,
    usvg_version: &'static str,
    resvg_version: &'static str,
}

fn main() -> Result<(), Box<dyn Error>> {
    let input = fs::read(INPUT)?;
    let text = std::str::from_utf8(&input)?;
    let xml_options = usvg::roxmltree::ParsingOptions {
        allow_dtd: false,
        ..Default::default()
    };
    let document = usvg::roxmltree::Document::parse_with_options(text, xml_options)?;

    let mut source_elements = BTreeMap::new();
    let mut source_max_depth = 0_usize;
    for node in document
        .descendants()
        .filter(usvg::roxmltree::Node::is_element)
    {
        *source_elements
            .entry(node.tag_name().name().to_owned())
            .or_default() += 1;
        source_max_depth = source_max_depth.max(node.ancestors().count());
    }

    let options = deterministic_options();
    let tree = usvg::Tree::from_xmltree(&document, &options)?;
    let mut normalized = NormalizedCounts::default();
    count_group(tree.root(), &mut normalized);

    let characterization = Characterization {
        fixture_bytes: input.len(),
        width: tree.size().width(),
        height: tree.size().height(),
        source_elements,
        source_max_depth,
        normalized,
        css_presentation_conflict_resolved_fill: css_conflict_probe()?,
        bundled_font: "Liberation Sans Regular (Excalidraw-pinned TTF)",
        usvg_version: "0.48.1",
        resvg_version: "0.48.1",
    };

    if let Some(parent) = Path::new(OUTPUT).parent() {
        fs::create_dir_all(parent)?;
    }
    let mut json = serde_json::to_string_pretty(&characterization)?;
    json.push('\n');
    fs::write(OUTPUT, json)?;
    render_baseline(&tree, 1, PNG_1X)?;
    render_baseline(&tree, 2, PNG_2X)?;
    Ok(())
}

fn deterministic_options() -> usvg::Options<'static> {
    let mut options = usvg::Options {
        resources_dir: None,
        font_family: "Liberation Sans".to_owned(),
        ..Default::default()
    };
    let fontdb = options.fontdb_mut();
    fontdb.load_font_data(LIBERATION_SANS.to_vec());
    fontdb.set_serif_family("Liberation Sans");
    fontdb.set_sans_serif_family("Liberation Sans");
    options.image_href_resolver = usvg::ImageHrefResolver {
        resolve_data: Box::new(|_, _, _| None),
        resolve_string: Box::new(|_, _| None),
    };
    options
}

fn count_group(group: &usvg::Group, counts: &mut NormalizedCounts) {
    counts.groups = counts.groups.saturating_add(1);
    if !group.filters().is_empty() {
        counts.filtered_groups = counts.filtered_groups.saturating_add(1);
    }
    for node in group.children() {
        match node {
            usvg::Node::Group(child) => count_group(child, counts),
            usvg::Node::Path(path) => {
                counts.paths = counts.paths.saturating_add(1);
                let curved = path.data().segments().any(|segment| {
                    matches!(segment, PathSegment::QuadTo(..) | PathSegment::CubicTo(..))
                });
                if curved {
                    counts.curved_paths = counts.curved_paths.saturating_add(1);
                } else {
                    counts.line_only_paths = counts.line_only_paths.saturating_add(1);
                }
                count_paint(path.fill().map(usvg::Fill::paint), true, counts);
                count_paint(path.stroke().map(usvg::Stroke::paint), false, counts);
            }
            usvg::Node::Text(_) => counts.texts = counts.texts.saturating_add(1),
            usvg::Node::Image(_) => counts.images = counts.images.saturating_add(1),
        }
    }
}

fn count_paint(paint: Option<&usvg::Paint>, fill: bool, counts: &mut NormalizedCounts) {
    match paint {
        Some(usvg::Paint::Color(_)) if fill => {
            counts.solid_fills = counts.solid_fills.saturating_add(1);
        }
        Some(usvg::Paint::Color(_)) => {
            counts.solid_strokes = counts.solid_strokes.saturating_add(1);
        }
        Some(_) => counts.paint_servers = counts.paint_servers.saturating_add(1),
        None => {}
    }
}

fn css_conflict_probe() -> Result<String, Box<dyn Error>> {
    let input = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
      <style>.card { fill: #123456; }</style>
      <rect class="card" fill="#abcdef" width="10" height="10"/>
    </svg>"##;
    let xml_options = usvg::roxmltree::ParsingOptions {
        allow_dtd: false,
        ..Default::default()
    };
    let document = usvg::roxmltree::Document::parse_with_options(input, xml_options)?;
    let tree = usvg::Tree::from_xmltree(&document, &deterministic_options())?;
    let path = tree.root().children().iter().find_map(|node| match node {
        usvg::Node::Path(path) => Some(path.as_ref()),
        _ => None,
    });
    let color = path
        .and_then(usvg::Path::fill)
        .map(usvg::Fill::paint)
        .and_then(|paint| match paint {
            usvg::Paint::Color(color) => Some(color),
            _ => None,
        })
        .ok_or("CSS cascade probe did not resolve to a solid fill")?;
    Ok(format!(
        "#{:02x}{:02x}{:02x}",
        color.red, color.green, color.blue
    ))
}

fn render_baseline(tree: &usvg::Tree, scale: u32, output: &str) -> Result<(), Box<dyn Error>> {
    let width = bounded_extent_to_u32(tree.size().width())?;
    let height = bounded_extent_to_u32(tree.size().height())?;
    let scaled_width = width.checked_mul(scale).ok_or("baseline width overflow")?;
    let scaled_height = height
        .checked_mul(scale)
        .ok_or("baseline height overflow")?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(scaled_width, scaled_height)
        .ok_or("baseline pixmap dimensions are invalid")?;
    let render_scale = match scale {
        1 => 1.0,
        2 => 2.0,
        _ => return Err("baseline scale must be one or two".into()),
    };
    let transform = resvg::tiny_skia::Transform::from_scale(render_scale, render_scale);
    resvg::render(tree, transform, &mut pixmap.as_mut());
    pixmap.save_png(output)?;
    Ok(())
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn bounded_extent_to_u32(value: f32) -> Result<u32, Box<dyn Error>> {
    let rounded = value.ceil();
    if !rounded.is_finite() || rounded <= 0.0 || f64::from(rounded) > f64::from(u32::MAX) {
        return Err("baseline extent is outside the PNG range".into());
    }
    Ok(rounded as u32)
}
