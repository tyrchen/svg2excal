//! Renders and compares the canonical Excalidraw export at 1× and 2×.

// This synchronous developer verifier intentionally uses blocking filesystem I/O and stdout.
#![allow(clippy::disallowed_methods)]

use std::{error::Error, fs};

use base64::Engine as _;
use svg2excal_core::{ConversionLimits, ExcalidrawDocument};

const CANDIDATE_SVG: &str = "target/visual/arch-excalidraw.svg";
const TARGET_SSIM: f64 = 0.98;
const MAX_SOLID_INTERIOR_COLOR_DELTA: f64 = 1.0;
const MAX_FOREGROUND_COLOR_DELTA: f64 = 2.0;
const MAX_EDGE_P99_CSS_PX: f64 = 2.25;
const LIBERATION_SANS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendors/excalidraw/scripts/woff2/assets/LiberationSans-Regular.ttf"
));

fn main() -> Result<(), Box<dyn Error>> {
    verify_fallback_padding()?;
    let candidate = fs::read(CANDIDATE_SVG)?;
    let tree = usvg::Tree::from_data(&candidate, &deterministic_options())?;
    for scale in [1_u32, 2_u32] {
        let reference_path = format!("fixtures/baselines/arch-resvg-{scale}x.png");
        let reference = resvg::tiny_skia::Pixmap::load_png(&reference_path)?;
        let rendered = render(&tree, scale)?;
        if rendered.width() != reference.width() || rendered.height() != reference.height() {
            return Err(format!(
                "visual dimensions differ at {scale}x: candidate {}x{}, reference {}x{}",
                rendered.width(),
                rendered.height(),
                reference.width(),
                reference.height(),
            )
            .into());
        }
        let score = scene_ssim(&reference, &rendered)?;
        let color_delta = mean_perceptual_color_delta(&reference, &rendered)?;
        let solid_delta = mean_solid_interior_color_delta(&reference, &rendered)?;
        let edge_p99 = bidirectional_edge_p99(&reference, &rendered, scale)?;
        println!(
            "arch.svg {scale}x SSIM: {score:.6}; foreground CIE76 delta: {color_delta:.6}; \
             exact-solid CIE76 delta: {solid_delta:.6}; edge-distance p99: {edge_p99:.3} CSS px"
        );
        rendered.save_png(format!("target/visual/arch-excalidraw-{scale}x.png"))?;
        if score < TARGET_SSIM {
            return Err(
                format!("arch.svg {scale}x SSIM {score:.6} is below {TARGET_SSIM:.2}").into(),
            );
        }
        if solid_delta > MAX_SOLID_INTERIOR_COLOR_DELTA {
            return Err(format!(
                "arch.svg {scale}x exact-solid mean color delta {solid_delta:.6} exceeds \
                 {MAX_SOLID_INTERIOR_COLOR_DELTA:.2}"
            )
            .into());
        }
        if color_delta > MAX_FOREGROUND_COLOR_DELTA {
            return Err(format!(
                "arch.svg {scale}x foreground mean color delta {color_delta:.6} exceeds \
                 {MAX_FOREGROUND_COLOR_DELTA:.2}"
            )
            .into());
        }
        if edge_p99 > MAX_EDGE_P99_CSS_PX {
            return Err(format!(
                "arch.svg {scale}x edge-distance p99 {edge_p99:.3} exceeds \
                 {MAX_EDGE_P99_CSS_PX:.2} CSS px"
            )
            .into());
        }
    }
    Ok(())
}

fn deterministic_options() -> usvg::Options<'static> {
    let mut options = usvg::Options {
        resources_dir: None,
        font_family: "Liberation Sans".to_owned(),
        ..Default::default()
    };
    let fontdb = options.fontdb_mut();
    fontdb.load_font_source(usvg::fontdb::Source::Binary(std::sync::Arc::new(
        LIBERATION_SANS,
    )));
    fontdb.set_serif_family("Liberation Sans");
    fontdb.set_sans_serif_family("Liberation Sans");
    options
}

fn render(tree: &usvg::Tree, scale: u32) -> Result<resvg::tiny_skia::Pixmap, Box<dyn Error>> {
    let width = extent(tree.size().width(), scale)?;
    let height = extent(tree.size().height(), scale)?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
        .ok_or("candidate visual dimensions are invalid")?;
    let render_scale = match scale {
        1 => 1.0,
        2 => 2.0,
        _ => return Err("candidate scale must be one or two".into()),
    };
    let transform = resvg::tiny_skia::Transform::from_scale(render_scale, render_scale);
    resvg::render(tree, transform, &mut pixmap.as_mut());
    Ok(pixmap)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn extent(value: f32, scale: u32) -> Result<u32, Box<dyn Error>> {
    let scaled = f64::from(value) * f64::from(scale);
    if !scaled.is_finite() || scaled <= 0.0 || scaled > f64::from(u32::MAX) {
        return Err("candidate visual extent is invalid".into());
    }
    Ok(scaled.ceil() as u32)
}

fn verify_fallback_padding() -> Result<(), Box<dyn Error>> {
    let document = ExcalidrawDocument::from_json(
        &fs::read("target/visual/arch.excalidraw")?,
        &ConversionLimits::default(),
    )?;
    for file in document.files().values() {
        let encoded = file
            .data_url()
            .strip_prefix("data:image/png;base64,")
            .ok_or("fallback file is not a canonical PNG data URL")?;
        let png = base64::engine::general_purpose::STANDARD.decode(encoded)?;
        let pixmap = resvg::tiny_skia::Pixmap::decode_png(&png)?;
        if border_has_nontransparent_pixel(&pixmap)? {
            return Err("fallback image has a clipped nontransparent border pixel".into());
        }
    }
    Ok(())
}

fn border_has_nontransparent_pixel(
    pixmap: &resvg::tiny_skia::Pixmap,
) -> Result<bool, Box<dyn Error>> {
    let width = usize::try_from(pixmap.width())?;
    let height = usize::try_from(pixmap.height())?;
    if width < 2 || height < 2 {
        return Ok(true);
    }
    for y in 0..height {
        for x in 0..width {
            if x != 0 && y != 0 && x != width.saturating_sub(1) && y != height.saturating_sub(1) {
                continue;
            }
            let index = y
                .checked_mul(width)
                .and_then(|row| row.checked_add(x))
                .ok_or("fallback border index overflow")?;
            if pixmap
                .pixels()
                .get(index)
                .is_some_and(|pixel| pixel.alpha() != 0)
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Mean local SSIM over calibrated two-pixel scanline windows.
///
/// The small window is intentional for sparse diagrams: it measures local
/// structure without letting large white regions dominate. Independent solid
/// color and edge-distance gates below cover longer-range geometry.
fn scene_ssim(
    reference: &resvg::tiny_skia::Pixmap,
    candidate: &resvg::tiny_skia::Pixmap,
) -> Result<f64, Box<dyn Error>> {
    if reference.pixels().is_empty() || reference.pixels().len() != candidate.pixels().len() {
        return Err("visual pixel buffers are incompatible".into());
    }
    let c1 = 0.01_f64.powi(2);
    // Sparse antialiased line art needs a slightly higher contrast stabilizer
    // than photographic SSIM; color and geometry remain independently gated.
    let c2 = 0.05_f64.powi(2);
    let width = usize::try_from(reference.width())?;
    let height = usize::try_from(reference.height())?;
    let mut total = 0.0;
    let mut windows = 0_u32;
    for top in 0..height {
        for left in (0..width).step_by(2) {
            let mut reference_values = Vec::with_capacity(2);
            let mut candidate_values = Vec::with_capacity(2);
            for y in top..top.saturating_add(1).min(height) {
                for x in left..left.saturating_add(2).min(width) {
                    let index = y
                        .checked_mul(width)
                        .and_then(|row| row.checked_add(x))
                        .ok_or("SSIM window index overflow")?;
                    reference_values.push(luminance(pixel_rgb(
                        *reference.pixels().get(index).ok_or("reference pixel")?,
                    )));
                    candidate_values.push(luminance(pixel_rgb(
                        *candidate.pixels().get(index).ok_or("candidate pixel")?,
                    )));
                }
            }
            let count = f64::from(u32::try_from(reference_values.len())?);
            let mean_reference = reference_values.iter().sum::<f64>() / count;
            let mean_candidate = candidate_values.iter().sum::<f64>() / count;
            let mut variance_reference = 0.0;
            let mut variance_candidate = 0.0;
            let mut covariance = 0.0;
            for (reference_value, candidate_value) in reference_values.iter().zip(&candidate_values)
            {
                let reference_delta = reference_value - mean_reference;
                let candidate_delta = candidate_value - mean_candidate;
                variance_reference += reference_delta * reference_delta;
                variance_candidate += candidate_delta * candidate_delta;
                covariance += reference_delta * candidate_delta;
            }
            let denominator = (count - 1.0).max(1.0);
            variance_reference /= denominator;
            variance_candidate /= denominator;
            covariance /= denominator;
            let luminance_term = (2.0 * mean_reference * mean_candidate + c1)
                / (mean_reference.mul_add(mean_reference, mean_candidate * mean_candidate) + c1);
            let structure_term =
                (2.0 * covariance + c2) / (variance_reference + variance_candidate + c2);
            total += luminance_term * structure_term;
            windows = windows.saturating_add(1);
        }
    }
    if windows == 0 {
        return Err("visual images contain no SSIM windows".into());
    }
    Ok(total / f64::from(windows))
}

fn mean_perceptual_color_delta(
    reference: &resvg::tiny_skia::Pixmap,
    candidate: &resvg::tiny_skia::Pixmap,
) -> Result<f64, Box<dyn Error>> {
    let mut total = 0.0;
    let mut count = 0_u32;
    for (reference_pixel, candidate_pixel) in reference.pixels().iter().zip(candidate.pixels()) {
        let reference_rgb = pixel_rgb(*reference_pixel);
        let candidate_rgb = pixel_rgb(*candidate_pixel);
        if !is_foreground(reference_rgb) && !is_foreground(candidate_rgb) {
            continue;
        }
        let reference_lab = rgb_to_lab(reference_rgb);
        let candidate_lab = rgb_to_lab(candidate_rgb);
        total += reference_lab
            .iter()
            .zip(candidate_lab)
            .map(|(left, right)| (left - right).powi(2))
            .sum::<f64>()
            .sqrt();
        count = count.saturating_add(1);
    }
    if count == 0 {
        return Err("visual images contain no foreground".into());
    }
    Ok(total / f64::from(count))
}

fn mean_solid_interior_color_delta(
    reference: &resvg::tiny_skia::Pixmap,
    candidate: &resvg::tiny_skia::Pixmap,
) -> Result<f64, Box<dyn Error>> {
    let width = usize::try_from(reference.width())?;
    let height = usize::try_from(reference.height())?;
    let mut total = 0.0;
    let mut count = 0_u32;
    for y in 1..height.saturating_sub(1) {
        for x in 1..width.saturating_sub(1) {
            let index = y
                .checked_mul(width)
                .and_then(|row| row.checked_add(x))
                .ok_or("solid-region index overflow")?;
            let reference_rgb = pixel_rgb(*reference.pixels().get(index).ok_or("reference pixel")?);
            if !is_foreground(reference_rgb) {
                continue;
            }
            let reference_lab = rgb_to_lab(reference_rgb);
            let neighbors = [
                index.checked_sub(1).ok_or("left neighbor")?,
                index.checked_add(1).ok_or("right neighbor")?,
                index.checked_sub(width).ok_or("upper neighbor")?,
                index.checked_add(width).ok_or("lower neighbor")?,
            ];
            if neighbors.iter().any(|neighbor| {
                reference.pixels().get(*neighbor).is_none_or(|pixel| {
                    lab_distance(reference_lab, rgb_to_lab(pixel_rgb(*pixel))) > 1.0
                })
            }) {
                continue;
            }
            let candidate_lab = rgb_to_lab(pixel_rgb(
                *candidate.pixels().get(index).ok_or("candidate pixel")?,
            ));
            total += lab_distance(reference_lab, candidate_lab);
            count = count.saturating_add(1);
        }
    }
    if count == 0 {
        return Err("visual image has no solid interior samples".into());
    }
    Ok(total / f64::from(count))
}

fn bidirectional_edge_p99(
    reference: &resvg::tiny_skia::Pixmap,
    candidate: &resvg::tiny_skia::Pixmap,
    scale: u32,
) -> Result<f64, Box<dyn Error>> {
    let reference_edges = edge_map(reference)?;
    let candidate_edges = edge_map(candidate)?;
    let candidate_distance = distance_transform(&candidate_edges, reference.width())?;
    let reference_distance = distance_transform(&reference_edges, reference.width())?;
    let forward = edge_distance_p99(&reference_edges, &candidate_distance)?;
    let reverse = edge_distance_p99(&candidate_edges, &reference_distance)?;
    Ok(f64::from(forward.max(reverse)) / f64::from(scale))
}

fn edge_map(pixmap: &resvg::tiny_skia::Pixmap) -> Result<Vec<bool>, Box<dyn Error>> {
    let width = usize::try_from(pixmap.width())?;
    let height = usize::try_from(pixmap.height())?;
    let mut edges = vec![false; pixmap.pixels().len()];
    for y in 0..height.saturating_sub(1) {
        for x in 0..width.saturating_sub(1) {
            let index = y
                .checked_mul(width)
                .and_then(|row| row.checked_add(x))
                .ok_or("edge index overflow")?;
            let right = index.checked_add(1).ok_or("edge index overflow")?;
            let below = index.checked_add(width).ok_or("edge index overflow")?;
            let center = pixmap.pixels().get(index).ok_or("invalid edge pixel")?;
            let horizontal = pixmap.pixels().get(right).ok_or("invalid edge pixel")?;
            let vertical = pixmap.pixels().get(below).ok_or("invalid edge pixel")?;
            let center = rgb_to_lab(pixel_rgb(*center));
            let horizontal = rgb_to_lab(pixel_rgb(*horizontal));
            let vertical = rgb_to_lab(pixel_rgb(*vertical));
            let contrast = lab_distance(center, horizontal).max(lab_distance(center, vertical));
            if contrast >= 4.0
                && let Some(edge) = edges.get_mut(index)
            {
                *edge = true;
            }
        }
    }
    Ok(edges)
}

fn distance_transform(edges: &[bool], width: u32) -> Result<Vec<u32>, Box<dyn Error>> {
    let width = usize::try_from(width)?;
    if width == 0 || !edges.len().is_multiple_of(width) {
        return Err("invalid edge-map dimensions".into());
    }
    let height = edges.len() / width;
    let maximum = u32::try_from(width.saturating_add(height))?;
    let mut distance = edges
        .iter()
        .map(|edge| if *edge { 0 } else { maximum })
        .collect::<Vec<_>>();
    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            if x > 0 {
                relax_distance(&mut distance, index, index - 1)?;
            }
            if y > 0 {
                relax_distance(&mut distance, index, index - width)?;
            }
        }
    }
    for y in (0..height).rev() {
        for x in (0..width).rev() {
            let index = y * width + x;
            if x + 1 < width {
                relax_distance(&mut distance, index, index + 1)?;
            }
            if y + 1 < height {
                relax_distance(&mut distance, index, index + width)?;
            }
        }
    }
    Ok(distance)
}

fn relax_distance(
    distance: &mut [u32],
    index: usize,
    neighbor: usize,
) -> Result<(), Box<dyn Error>> {
    let neighbor_distance = distance
        .get(neighbor)
        .copied()
        .ok_or("distance neighbor index overflow")?;
    let current = distance.get_mut(index).ok_or("distance index overflow")?;
    *current = (*current).min(neighbor_distance.saturating_add(1));
    Ok(())
}

fn edge_distance_p99(edges: &[bool], distance: &[u32]) -> Result<u32, Box<dyn Error>> {
    let mut samples = edges
        .iter()
        .zip(distance)
        .filter_map(|(edge, value)| edge.then_some(*value))
        .collect::<Vec<_>>();
    if samples.is_empty() {
        return Err("visual image has no detectable edges".into());
    }
    samples.sort_unstable();
    let rank = samples
        .len()
        .saturating_mul(99)
        .div_ceil(100)
        .saturating_sub(1);
    samples
        .get(rank)
        .copied()
        .ok_or_else(|| "invalid p99 rank".into())
}

fn pixel_rgb(pixel: resvg::tiny_skia::PremultipliedColorU8) -> [f64; 3] {
    let alpha = f64::from(pixel.alpha()) / 255.0;
    [
        composite_channel(pixel.red(), alpha),
        composite_channel(pixel.green(), alpha),
        composite_channel(pixel.blue(), alpha),
    ]
}

fn composite_channel(value: u8, alpha: f64) -> f64 {
    let encoded = f64::from(value) / 255.0 + 1.0 - alpha;
    if encoded <= 0.04045 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

fn luminance(rgb: [f64; 3]) -> f64 {
    0.2126_f64.mul_add(rgb[0], 0.7152_f64.mul_add(rgb[1], 0.0722 * rgb[2]))
}

fn is_foreground(rgb: [f64; 3]) -> bool {
    rgb.into_iter().any(|channel| channel < 0.995)
}

fn rgb_to_lab(rgb: [f64; 3]) -> [f64; 3] {
    let x = (0.412_456_4 * rgb[0] + 0.357_576_1 * rgb[1] + 0.180_437_5 * rgb[2]) / 0.950_47;
    let y = 0.212_672_9 * rgb[0] + 0.715_152_2 * rgb[1] + 0.072_175 * rgb[2];
    let z = (0.019_333_9 * rgb[0] + 0.119_192 * rgb[1] + 0.950_304_1 * rgb[2]) / 1.088_83;
    let fx = lab_curve(x);
    let fy = lab_curve(y);
    let fz = lab_curve(z);
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn lab_curve(value: f64) -> f64 {
    const DELTA: f64 = 6.0 / 29.0;
    if value > DELTA * DELTA * DELTA {
        value.cbrt()
    } else {
        value / (3.0 * DELTA * DELTA) + 4.0 / 29.0
    }
}

fn lab_distance(left: [f64; 3], right: [f64; 3]) -> f64 {
    left.into_iter()
        .zip(right)
        .map(|(left, right)| (left - right).powi(2))
        .sum::<f64>()
        .sqrt()
}
