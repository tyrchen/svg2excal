//! Feature and canonical-fixture integration tests.

use base64::Engine as _;
use serde_json::Value;
use svg2excal_core::{
    ConversionError, ConversionOptions, ConversionProfile, ExcalidrawDocument, ProvenanceMode,
    convert,
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
fn test_should_convert_arch_fixture_deterministically() -> Result<(), Box<dyn std::error::Error>> {
    let input = include_bytes!("../../../fixtures/arch.svg");
    let first = convert(input, &ConversionOptions::default())?;
    let second = convert(input, &ConversionOptions::default())?;
    assert_eq!(
        first.document.to_pretty_json()?,
        second.document.to_pretty_json()?
    );
    assert_eq!(
        first
            .document
            .elements()
            .iter()
            .filter(|element| element.element_type() == "text")
            .count(),
        86
    );
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
    assert!(matches!(
        convert(input, &options),
        Err(ConversionError::StrictFidelityViolation { .. })
    ));
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
