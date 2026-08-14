//! Feature and canonical-fixture integration tests.

use std::collections::BTreeSet;

use base64::Engine as _;
use serde_json::Value;
use svg2excal_core::{
    ConversionError, ConversionOptions, ConversionProfile, ConversionResult, DiagnosticCode,
    ExcalidrawDocument, ProvenanceMode, RasterOptions, convert,
};

#[test]
fn test_should_round_trip_stable_typed_json() -> Result<(), Box<dyn std::error::Error>> {
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="60">
      <rect x="2" y="3" width="30" height="20" fill="#74c0fc" stroke="#1971c2"/>
      <ellipse cx="70" cy="25" rx="20" ry="12" fill="#b2f2bb"/>
      <text x="5" y="55" font-size="12">stable</text>
    </svg>"##;
    let options = ConversionOptions::default();
    let result = convert(input, &options)?;
    let json = result
        .document
        .to_pretty_json_with_limits(&options.limits)?;
    let restored = ExcalidrawDocument::from_json(json.as_bytes(), &options.limits)?;
    assert_eq!(json, restored.to_pretty_json_with_limits(&options.limits)?);
    Ok(())
}

#[test]
fn test_should_reject_curve_in_strict_profile() {
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="30">
      <path d="M1 20 C5 1 25 1 29 20" fill="none" stroke="#000000"/>
    </svg>"##;
    let options = ConversionOptions::builder()
        .profile(ConversionProfile::Strict)
        .build();
    let result = convert(input, &options);
    assert!(matches!(
        result,
        Err(ConversionError::StrictFidelityViolation { .. })
    ));
}

#[test]
fn test_should_deny_external_resource_by_default() {
    let input = br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">
      <image href="relative.png" width="20" height="20"/>
    </svg>"#;
    let result = convert(input, &ConversionOptions::default());
    assert!(matches!(
        result,
        Err(ConversionError::ResourceDenied { .. })
    ));
}

#[test]
fn test_should_convert_rfc_fixture_deterministically() -> Result<(), Box<dyn std::error::Error>> {
    let input = include_bytes!("../fixtures/rfc.svg");
    let first = convert(input, &ConversionOptions::default())?;
    let second = convert(input, &ConversionOptions::default())?;
    assert_eq!(
        first.document.to_pretty_json()?,
        second.document.to_pretty_json()?
    );
    assert_rfc_counts(&first);
    assert_rfc_target_json(&first)?;
    Ok(())
}

fn assert_rfc_counts(result: &ConversionResult) {
    assert_eq!(
        result
            .document
            .elements()
            .iter()
            .filter(|element| element.element_type() == "text")
            .count(),
        131
    );
    assert_eq!(
        result
            .document
            .elements()
            .iter()
            .filter(|element| element.element_type() == "line")
            .count(),
        47
    );
    assert_eq!(
        result
            .document
            .elements()
            .iter()
            .filter(|element| element.element_type() == "image")
            .count(),
        12
    );
    assert_eq!(
        result
            .report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == DiagnosticCode::FilterOmitted)
            .count(),
        12
    );
    assert_eq!(
        result
            .report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == DiagnosticCode::ClipRasterized)
            .count(),
        12
    );
    assert_eq!(
        result
            .report
            .diagnostics
            .iter()
            .filter(|diagnostic| { diagnostic.code() == DiagnosticCode::MarkerPreservedAsGeometry })
            .count(),
        12
    );
}

fn assert_rfc_target_json(result: &ConversionResult) -> Result<(), Box<dyn std::error::Error>> {
    let json: Value = serde_json::from_str(&result.document.to_pretty_json()?)?;
    let elements = json
        .get("elements")
        .and_then(Value::as_array)
        .ok_or_else(|| std::io::Error::other("missing RFC elements"))?;
    assert_eq!(
        elements
            .first()
            .and_then(|element| element.get("type"))
            .and_then(Value::as_str),
        Some("rectangle")
    );
    assert!(
        elements
            .iter()
            .filter_map(|element| element.get("roundness"))
            .filter(|roundness| !roundness.is_null())
            .count()
            >= 45
    );
    assert!(elements.iter().all(|element| {
        element.get("frameId").is_some_and(Value::is_null)
            && element.get("boundElements").is_some_and(Value::is_null)
    }));
    let lines = elements
        .iter()
        .filter(|element| element.get("type").and_then(Value::as_str) == Some("line"))
        .collect::<Vec<_>>();
    assert!(lines.iter().all(|line| {
        line.get("startArrowhead").is_some_and(Value::is_null)
            && line.get("endArrowhead").is_some_and(Value::is_null)
            && line.get("startBinding").is_some_and(Value::is_null)
            && line.get("endBinding").is_some_and(Value::is_null)
    }));
    let group_ids = elements
        .iter()
        .filter_map(|element| element.get("groupIds").and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    assert!(group_ids.len() >= 13);
    Ok(())
}

#[test]
fn test_should_preserve_closed_stroke_without_polygon_fill()
-> Result<(), Box<dyn std::error::Error>> {
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40">
      <path d="M5 5 L35 5 L20 35 Z" fill="none" stroke="#000000" stroke-linecap="round" stroke-linejoin="round"/>
    </svg>"##;
    let result = convert(input, &ConversionOptions::default())?;
    let json: Value = serde_json::from_str(&result.document.to_pretty_json()?)?;
    assert_eq!(
        json.pointer("/elements/0/polygon"),
        Some(&Value::Bool(false))
    );
    assert_eq!(
        json.pointer("/elements/0/points/0"),
        json.pointer("/elements/0/points/3")
    );
    Ok(())
}

#[test]
fn test_should_accept_exact_target_font_in_strict_profile() {
    let input = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30">
      <text x="2" y="20" font-family="Liberation Sans" font-size="16">exact</text>
    </svg>"#;
    let options = ConversionOptions::builder()
        .profile(ConversionProfile::Strict)
        .build();
    assert!(convert(input, &options).is_ok());
}

#[test]
fn test_should_reject_non_native_stroke_semantics_in_strict_profile() {
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30">
      <path d="M2 15 L98 15" fill="none" stroke="#000" stroke-dasharray="7 3"/>
    </svg>"##;
    let options = ConversionOptions::builder()
        .profile(ConversionProfile::Strict)
        .build();
    let result = convert(input, &options);
    assert!(
        matches!(result, Err(ConversionError::StrictFidelityViolation { .. })),
        "unexpected strict result: {result:?}"
    );
}

#[test]
fn test_should_preserve_stroke_then_fill_paint_order() -> Result<(), Box<dyn std::error::Error>> {
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40">
      <rect x="5" y="5" width="30" height="30" fill="#ff0000" stroke="#0000ff" paint-order="stroke fill"/>
    </svg>"##;
    let result = convert(input, &ConversionOptions::default())?;
    let json: Value = serde_json::from_str(&result.document.to_pretty_json()?)?;
    assert_eq!(
        json.pointer("/elements/0/backgroundColor")
            .and_then(Value::as_str),
        Some("transparent")
    );
    assert_eq!(
        json.pointer("/elements/1/backgroundColor")
            .and_then(Value::as_str),
        Some("#ff0000")
    );
    Ok(())
}

#[test]
fn test_should_fallback_for_self_intersecting_fill() -> Result<(), Box<dyn std::error::Error>> {
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="50" height="50">
      <path d="M5 5 L45 45 L45 5 L5 45 Z" fill="#ff0000"/>
    </svg>"##;
    let result = convert(input, &ConversionOptions::default())?;
    assert_eq!(result.document.elements().len(), 1);
    assert_eq!(
        result
            .document
            .elements()
            .first()
            .map(svg2excal_core::ExcalidrawElement::element_type),
        Some("image")
    );
    Ok(())
}

#[test]
fn test_should_render_off_origin_fallback_pixels() -> Result<(), Box<dyn std::error::Error>> {
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="220" height="120">
      <defs><linearGradient id="g"><stop stop-color="#ff0000"/><stop offset="1" stop-color="#0000ff"/></linearGradient></defs>
      <rect x="150" y="70" width="40" height="30" fill="url(#g)"/>
    </svg>"##;
    let result = convert(input, &ConversionOptions::default())?;
    let file = result
        .document
        .files()
        .values()
        .next()
        .ok_or_else(|| std::io::Error::other("missing fallback file"))?;
    let encoded = file
        .data_url()
        .strip_prefix("data:image/png;base64,")
        .ok_or_else(|| std::io::Error::other("invalid fallback data URL"))?;
    let png = base64::engine::general_purpose::STANDARD.decode(encoded)?;
    let pixmap = resvg::tiny_skia::Pixmap::decode_png(&png)?;
    assert!(pixmap.pixels().iter().any(|pixel| pixel.alpha() > 0));
    Ok(())
}

#[test]
fn test_should_round_trip_compact_provenance() -> Result<(), Box<dyn std::error::Error>> {
    let input = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>"#;
    let options = ConversionOptions::builder()
        .provenance(ProvenanceMode::Compact)
        .build();
    let result = convert(input, &options)?;
    let json = result
        .document
        .to_pretty_json_with_limits(&options.limits)?;
    assert!(json.contains("\"svg2excal\""));
    ExcalidrawDocument::from_json(json.as_bytes(), &options.limits)?;
    Ok(())
}

#[test]
fn test_should_group_multiple_elements_from_explicit_source_group()
-> Result<(), Box<dyn std::error::Error>> {
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40">
      <g><rect width="30" height="30" fill="#f00"/><circle cx="55" cy="15" r="15" fill="#00f"/></g>
    </svg>"##;
    let result = convert(input, &ConversionOptions::default())?;
    let json: Value = serde_json::from_str(&result.document.to_pretty_json()?)?;
    let first = json
        .pointer("/elements/0/groupIds/0")
        .and_then(Value::as_str);
    let second = json
        .pointer("/elements/1/groupIds/0")
        .and_then(Value::as_str);
    assert!(first.is_some());
    assert_eq!(first, second);
    Ok(())
}

#[test]
fn test_should_not_change_id_attribute_selector_semantics() -> Result<(), Box<dyn std::error::Error>>
{
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="50" height="20">
      <style>[id] { fill: #ff0000; }</style>
      <rect id="authored" width="20" height="20" fill="#0000ff"/>
      <rect x="30" width="20" height="20" fill="#0000ff"/>
    </svg>"##;
    let result = convert(input, &ConversionOptions::default())?;
    let json: Value = serde_json::from_str(&result.document.to_pretty_json()?)?;
    assert_eq!(
        json.pointer("/elements/0/backgroundColor")
            .and_then(Value::as_str),
        Some("#ff0000")
    );
    assert_eq!(
        json.pointer("/elements/1/backgroundColor")
            .and_then(Value::as_str),
        Some("#0000ff")
    );
    Ok(())
}

#[test]
fn test_should_promote_recognized_endpoint_marker_to_arrow()
-> Result<(), Box<dyn std::error::Error>> {
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30">
      <defs><marker id="head" viewBox="0 0 10 10" refX="8.5" refY="5" markerWidth="6.5" markerHeight="6.5" orient="auto-start-reverse"><path d="M0 0.6 L9.5 5 L0 9.4 Z" fill="#000000"/></marker></defs>
      <line x1="5" y1="15" x2="95" y2="15" stroke="#000" marker-end="url(#head)"/>
    </svg>"##;
    let result = convert(input, &ConversionOptions::default())?;
    let json: Value = serde_json::from_str(&result.document.to_pretty_json()?)?;
    assert_eq!(
        json.pointer("/elements/0/type").and_then(Value::as_str),
        Some("arrow")
    );
    assert_eq!(
        json.pointer("/elements/0/endArrowhead")
            .and_then(Value::as_str),
        Some("triangle")
    );
    assert!(
        json.pointer("/elements/0/startArrowhead")
            .is_some_and(Value::is_null)
    );
    Ok(())
}

#[test]
fn test_should_emit_correlated_rounded_rectangle() -> Result<(), Box<dyn std::error::Error>> {
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50">
      <rect x="5" y="5" width="90" height="40" rx="8" fill="#fff" stroke="#000"/>
    </svg>"##;
    let result = convert(input, &ConversionOptions::default())?;
    let json: Value = serde_json::from_str(&result.document.to_pretty_json()?)?;
    assert_eq!(
        json.pointer("/elements/0/type").and_then(Value::as_str),
        Some("rectangle")
    );
    assert_eq!(
        json.pointer("/elements/0/roundness/type")
            .and_then(Value::as_u64),
        Some(3)
    );
    assert_eq!(
        json.pointer("/elements/0/roundness/value")
            .and_then(Value::as_f64),
        Some(8.0)
    );
    Ok(())
}

#[test]
fn test_should_rasterize_text_outside_target_font_coverage()
-> Result<(), Box<dyn std::error::Error>> {
    let input = r#"<svg xmlns="http://www.w3.org/2000/svg" width="50" height="40">
      <text x="5" y="30" font-size="24">漢</text>
    </svg>"#;
    let result = convert(input.as_bytes(), &ConversionOptions::default())?;
    assert_eq!(
        result
            .document
            .elements()
            .first()
            .map(svg2excal_core::ExcalidrawElement::element_type),
        Some("image")
    );
    let strict = ConversionOptions::builder()
        .profile(ConversionProfile::Strict)
        .build();
    assert!(matches!(
        convert(input.as_bytes(), &strict),
        Err(ConversionError::StrictFidelityViolation { .. })
    ));
    Ok(())
}

#[test]
fn test_should_preserve_noncanonical_marker_as_geometry() -> Result<(), Box<dyn std::error::Error>>
{
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30">
      <defs><marker id="head" orient="auto"><path d="M0 0 L10 5 L0 10 Z" fill="#000000"/></marker></defs>
      <line x1="5" y1="15" x2="95" y2="15" stroke="#000" marker-end="url(#head)"/>
    </svg>"##;
    let result = convert(input, &ConversionOptions::default())?;
    assert!(
        result
            .document
            .elements()
            .iter()
            .all(|element| element.element_type() != "arrow")
    );
    assert!(result.document.elements().len() >= 2);
    assert!(
        result
            .report
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code() == DiagnosticCode::MarkerPreservedAsGeometry })
    );
    Ok(())
}

#[test]
fn test_should_not_promote_marker_when_connector_has_painted_open_fill()
-> Result<(), Box<dyn std::error::Error>> {
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="40">
      <defs><marker id="head" viewBox="0 0 10 10" refX="8.5" refY="5" markerWidth="6.5" markerHeight="6.5" orient="auto"><path d="M0 0.6 L9.5 5 L0 9.4 Z" fill="#000000"/></marker></defs>
      <path d="M5 30 L50 5 L95 30" fill="#ff0000" stroke="#000000" marker-end="url(#head)"/>
    </svg>"##;
    let result = convert(input, &ConversionOptions::default())?;
    let json: Value = serde_json::from_str(&result.document.to_pretty_json()?)?;
    let elements = json
        .get("elements")
        .and_then(Value::as_array)
        .ok_or_else(|| std::io::Error::other("missing elements"))?;
    assert!(
        elements
            .iter()
            .all(|element| { element.get("type").and_then(Value::as_str) != Some("arrow") })
    );
    assert!(elements.iter().any(|element| {
        element.get("backgroundColor").and_then(Value::as_str) == Some("#ff0000")
    }));
    Ok(())
}

#[test]
fn test_should_rasterize_radius_outside_excalidraw_adaptive_model()
-> Result<(), Box<dyn std::error::Error>> {
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="40">
      <rect x="5" y="3" width="110" height="34" rx="17" fill="#ffffff" stroke="#000000"/>
    </svg>"##;
    let result = convert(input, &ConversionOptions::default())?;
    assert_eq!(
        result
            .document
            .elements()
            .first()
            .map(svg2excal_core::ExcalidrawElement::element_type),
        Some("image")
    );
    assert!(
        result
            .report
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code() == DiagnosticCode::PaintIslandRasterized })
    );
    Ok(())
}

#[test]
fn test_should_rasterize_multi_chunk_text_as_one_paint_island()
-> Result<(), Box<dyn std::error::Error>> {
    let input = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30">
      <text y="20" font-family="Liberation Sans" font-size="16"><tspan x="2">left</tspan><tspan x="50">right</tspan></text>
    </svg>"#;
    let result = convert(input, &ConversionOptions::default())?;
    assert_eq!(result.document.elements().len(), 1);
    assert_eq!(
        result
            .document
            .elements()
            .first()
            .map(svg2excal_core::ExcalidrawElement::element_type),
        Some("image")
    );
    Ok(())
}

#[test]
fn test_should_not_omit_shadow_from_partially_transparent_group()
-> Result<(), Box<dyn std::error::Error>> {
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="40">
      <defs><filter id="shadow"><feDropShadow dx="1" dy="1" stdDeviation="2" flood-color="#000000" flood-opacity="0.1"/></filter></defs>
      <g opacity="0.5" filter="url(#shadow)"><rect x="5" y="5" width="50" height="30" fill="#ff0000"/></g>
    </svg>"##;
    let result = convert(input, &ConversionOptions::default())?;
    assert_eq!(result.document.elements().len(), 1);
    assert_eq!(
        result
            .document
            .elements()
            .first()
            .map(svg2excal_core::ExcalidrawElement::element_type),
        Some("image")
    );
    assert!(
        result
            .report
            .diagnostics
            .iter()
            .all(|diagnostic| { diagnostic.code() != DiagnosticCode::FilterOmitted })
    );
    Ok(())
}

#[test]
fn test_should_assign_distinct_group_ids_to_reused_definition_instances()
-> Result<(), Box<dyn std::error::Error>> {
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="40">
      <defs><g id="icon"><rect width="10" height="10"/><rect x="12" width="10" height="10"/></g></defs>
      <use href="#icon" x="5" y="5"/><use href="#icon" x="60" y="5"/>
    </svg>"##;
    let result = convert(input, &ConversionOptions::default())?;
    let json: Value = serde_json::from_str(&result.document.to_pretty_json()?)?;
    let elements = json
        .get("elements")
        .and_then(Value::as_array)
        .ok_or_else(|| std::io::Error::other("missing elements"))?;
    assert_eq!(elements.len(), 4);
    let first_ids = elements
        .first()
        .and_then(|element| element.get("groupIds"))
        .and_then(Value::as_array)
        .ok_or_else(|| std::io::Error::other("missing first group IDs"))?;
    let second_ids = elements
        .get(1)
        .and_then(|element| element.get("groupIds"))
        .and_then(Value::as_array)
        .ok_or_else(|| std::io::Error::other("missing second group IDs"))?;
    let third_ids = elements
        .get(2)
        .and_then(|element| element.get("groupIds"))
        .and_then(Value::as_array)
        .ok_or_else(|| std::io::Error::other("missing third group IDs"))?;
    let fourth_ids = elements
        .get(3)
        .and_then(|element| element.get("groupIds"))
        .and_then(Value::as_array)
        .ok_or_else(|| std::io::Error::other("missing fourth group IDs"))?;
    assert_eq!(first_ids, second_ids);
    assert_eq!(third_ids, fourth_ids);
    assert!(first_ids.iter().all(|id| !third_ids.contains(id)));
    Ok(())
}

#[test]
fn test_should_decompose_disjoint_compound_fill_only_in_editable_profile()
-> Result<(), Box<dyn std::error::Error>> {
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="25">
      <path d="M2 2 H22 V22 H2 Z M38 2 H58 V22 H38 Z" fill="#ff0000"/>
    </svg>"##;
    let balanced = convert(input, &ConversionOptions::default())?;
    assert_eq!(
        balanced
            .document
            .elements()
            .first()
            .map(svg2excal_core::ExcalidrawElement::element_type),
        Some("image")
    );

    let editable_options = ConversionOptions::builder()
        .profile(ConversionProfile::Editable)
        .build();
    let editable = convert(input, &editable_options)?;
    assert_eq!(editable.document.elements().len(), 2);
    assert!(
        editable.document.elements().iter().all(|element| {
            element.element_type() != "image" && !element.group_ids().is_empty()
        })
    );
    assert!(
        editable
            .report
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code() == DiagnosticCode::CompoundPathDecomposed })
    );
    Ok(())
}

#[test]
fn test_should_report_specific_fallback_island_kind() -> Result<(), Box<dyn std::error::Error>> {
    let gradient = br##"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="30">
      <defs><linearGradient id="g"><stop stop-color="#f00"/><stop offset="1" stop-color="#00f"/></linearGradient></defs>
      <rect width="30" height="30" fill="url(#g)"/>
    </svg>"##;
    let result = convert(gradient, &ConversionOptions::default())?;
    assert!(
        result
            .report
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code() == DiagnosticCode::GradientRasterized })
    );

    let clipped = br##"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="30">
      <defs><clipPath id="c"><circle cx="15" cy="15" r="10"/></clipPath></defs>
      <g clip-path="url(#c)"><rect width="30" height="30" fill="#f00"/></g>
    </svg>"##;
    let result = convert(clipped, &ConversionOptions::default())?;
    assert!(
        result
            .report
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code() == DiagnosticCode::ClipRasterized })
    );
    Ok(())
}

#[test]
fn test_should_make_fidelity_profile_rasterize_visible_approximations()
-> Result<(), Box<dyn std::error::Error>> {
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="140" height="40">
      <text x="2" y="20" font-family="Inter" font-size="16">substitute</text>
      <path d="M2 32 H138" stroke="#000" stroke-dasharray="7 3"/>
    </svg>"##;
    let options = ConversionOptions::builder()
        .profile(ConversionProfile::Fidelity)
        .build();
    let result = convert(input, &options)?;
    assert_eq!(
        result
            .document
            .elements()
            .iter()
            .filter(|element| element.element_type() == "image")
            .count(),
        2
    );
    assert!(
        result.document.elements().iter().all(|element| {
            element.element_type() != "text" && element.element_type() != "line"
        })
    );
    Ok(())
}

#[test]
fn test_should_gate_nested_svg_data_images_by_explicit_option()
-> Result<(), Box<dyn std::error::Error>> {
    let nested = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><circle cx="10" cy="10" r="8" fill="#ff0000"/></svg>"##;
    let encoded = base64::engine::general_purpose::STANDARD.encode(nested);
    let outer = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="30"><image x="5" y="5" width="20" height="20" href="data:image/svg+xml;base64,{encoded}"/></svg>"#
    );
    assert!(matches!(
        convert(outer.as_bytes(), &ConversionOptions::default()),
        Err(ConversionError::ResourceDenied { .. })
    ));

    let raster = RasterOptions::try_new(2.0, true)?;
    let options = ConversionOptions::builder().raster(raster).build();
    let result = convert(outer.as_bytes(), &options)?;
    assert_eq!(
        result
            .document
            .elements()
            .first()
            .map(svg2excal_core::ExcalidrawElement::element_type),
        Some("image")
    );
    Ok(())
}

#[test]
fn test_should_render_transformed_zero_area_stroke_fallback()
-> Result<(), Box<dyn std::error::Error>> {
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="100">
      <path d="M0 0 H30" transform="translate(42 35) rotate(20) scale(2)" fill="none" stroke="#e11d48" stroke-width="3" stroke-dasharray="7 3"/>
    </svg>"##;
    let options = ConversionOptions::builder()
        .profile(ConversionProfile::Fidelity)
        .build();
    let result = convert(input, &options)?;
    let file = result
        .document
        .files()
        .values()
        .next()
        .ok_or_else(|| std::io::Error::other("missing fallback image"))?;
    let encoded = file
        .data_url()
        .strip_prefix("data:image/png;base64,")
        .ok_or_else(|| std::io::Error::other("invalid fallback URL"))?;
    let png = base64::engine::general_purpose::STANDARD.decode(encoded)?;
    let pixmap = resvg::tiny_skia::Pixmap::decode_png(&png)?;
    assert!(pixmap.pixels().iter().any(|pixel| pixel.alpha() != 0));
    Ok(())
}

#[test]
fn test_should_accept_static_raster_image_in_strict_profile()
-> Result<(), Box<dyn std::error::Error>> {
    let mut pixmap = resvg::tiny_skia::Pixmap::new(2, 2)
        .ok_or_else(|| std::io::Error::other("invalid test pixmap"))?;
    pixmap.fill(resvg::tiny_skia::Color::from_rgba8(255, 0, 0, 255));
    let encoded = base64::engine::general_purpose::STANDARD.encode(pixmap.encode_png()?);
    let input = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><image x="2" y="2" width="16" height="16" href="data:image/png;base64,{encoded}"/></svg>"#
    );
    let options = ConversionOptions::builder()
        .profile(ConversionProfile::Strict)
        .build();
    let result = convert(input.as_bytes(), &options)?;
    assert_eq!(
        result
            .document
            .elements()
            .first()
            .map(svg2excal_core::ExcalidrawElement::element_type),
        Some("image")
    );
    assert!(result.report.diagnostics.is_empty());
    Ok(())
}
