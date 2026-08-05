//! Hostile-input boundary tests.

use std::{fmt::Write as _, io::Write as _};

use base64::Engine as _;
use flate2::{Compression, write::GzEncoder};
use rstest::rstest;
use svg2excal_core::{
    CancellationFlag, ConversionError, ConversionLimits, ConversionOptions, ExcalidrawDocument,
    InputRejection, LimitResource, ProvenanceMode, RasterOptions, convert,
};

#[test]
fn test_should_reject_empty_input() {
    let result = convert(&[], &ConversionOptions::default());
    assert!(matches!(
        result,
        Err(ConversionError::InputRejected(InputRejection::Empty))
    ));
}

#[test]
fn test_should_honor_cancellation_before_conversion() {
    let cancellation = CancellationFlag::default();
    cancellation.cancel();
    let options = ConversionOptions::builder()
        .cancellation(cancellation)
        .build();
    assert!(matches!(
        convert(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>", &options),
        Err(ConversionError::Cancelled)
    ));
}

#[test]
fn test_should_reject_input_one_byte_over_limit() -> Result<(), Box<dyn std::error::Error>> {
    let limits = ConversionLimits::builder()
        .max_input_bytes(8)
        .max_decompressed_bytes(8)
        .build()?;
    let options = ConversionOptions::builder().limits(limits).build();
    let result = convert(b"123456789", &options);
    assert!(matches!(
        result,
        Err(ConversionError::LimitExceeded {
            resource: LimitResource::InputBytes,
            limit: 8
        })
    ));
    Ok(())
}

#[rstest]
#[case(-1, true)]
#[case(0, false)]
#[case(1, false)]
fn test_should_enforce_input_byte_limit_at_boundary(
    #[case] delta: isize,
    #[case] rejected: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>";
    let input_len = isize::try_from(input.len())?;
    let limit = usize::try_from(input_len.saturating_add(delta))?;
    let limits = ConversionLimits::builder().max_input_bytes(limit).build()?;
    let result = convert(input, &ConversionOptions::builder().limits(limits).build());
    assert_eq!(
        matches!(
            result,
            Err(ConversionError::LimitExceeded {
                resource: LimitResource::InputBytes,
                ..
            })
        ),
        rejected
    );
    Ok(())
}

#[rstest]
#[case(1, false)]
#[case(2, false)]
#[case(3, true)]
fn test_should_enforce_xml_element_limit_at_boundary(
    #[case] rectangles: usize,
    #[case] rejected: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::from(r#"<svg xmlns="http://www.w3.org/2000/svg">"#);
    for _ in 0..rectangles {
        input.push_str("<rect/>");
    }
    input.push_str("</svg>");
    let limits = ConversionLimits::builder().max_xml_elements(3).build()?;
    let result = convert(
        input.as_bytes(),
        &ConversionOptions::builder().limits(limits).build(),
    );
    assert_eq!(
        matches!(
            result,
            Err(ConversionError::LimitExceeded {
                resource: LimitResource::XmlElements,
                limit: 3
            })
        ),
        rejected
    );
    Ok(())
}

#[rstest]
#[case(2, true)]
#[case(3, false)]
#[case(4, false)]
fn test_should_enforce_xml_depth_limit_at_boundary(
    #[case] limit: usize,
    #[case] rejected: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = br#"<svg xmlns="http://www.w3.org/2000/svg"><g><rect/></g></svg>"#;
    let limits = ConversionLimits::builder().max_xml_depth(limit).build()?;
    let result = convert(input, &ConversionOptions::builder().limits(limits).build());
    assert_eq!(
        matches!(
            result,
            Err(ConversionError::LimitExceeded {
                resource: LimitResource::XmlDepth,
                ..
            })
        ),
        rejected
    );
    Ok(())
}

#[rstest]
#[case(2, true)]
#[case(3, false)]
#[case(4, false)]
fn test_should_enforce_per_element_attribute_limit_at_boundary(
    #[case] limit: usize,
    #[case] rejected: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = br#"<svg xmlns="http://www.w3.org/2000/svg"><rect x="1" y="1" width="1"/></svg>"#;
    let limits = ConversionLimits::builder()
        .max_attributes_per_element(limit)
        .build()?;
    let result = convert(input, &ConversionOptions::builder().limits(limits).build());
    assert_eq!(
        matches!(
            result,
            Err(ConversionError::LimitExceeded {
                resource: LimitResource::XmlAttributes,
                ..
            })
        ),
        rejected
    );
    Ok(())
}

#[rstest]
#[case(2, true)]
#[case(3, false)]
#[case(4, false)]
fn test_should_enforce_total_attribute_limit_at_boundary(
    #[case] limit: usize,
    #[case] rejected: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = br#"<svg xmlns="http://www.w3.org/2000/svg"><rect x="1"/><rect y="1"/><rect width="1"/></svg>"#;
    let limits = ConversionLimits::builder()
        .max_attributes_per_element(1)
        .max_total_attributes(limit)
        .build()?;
    let result = convert(input, &ConversionOptions::builder().limits(limits).build());
    assert_eq!(
        matches!(
            result,
            Err(ConversionError::LimitExceeded {
                resource: LimitResource::XmlAttributes,
                ..
            })
        ),
        rejected
    );
    Ok(())
}

#[rstest]
#[case("abc", false)]
#[case("abcd", false)]
#[case("abcde", true)]
fn test_should_enforce_single_text_limit_at_boundary(
    #[case] text: &str,
    #[case] rejected: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = format!(r#"<svg xmlns="http://www.w3.org/2000/svg"><text>{text}</text></svg>"#);
    let limits = ConversionLimits::builder()
        .max_single_text_bytes(4)
        .build()?;
    let result = convert(
        input.as_bytes(),
        &ConversionOptions::builder().limits(limits).build(),
    );
    assert_eq!(
        matches!(
            result,
            Err(ConversionError::LimitExceeded {
                resource: LimitResource::XmlText,
                limit: 4
            })
        ),
        rejected
    );
    Ok(())
}

#[rstest]
#[case(2, true)]
#[case(3, false)]
#[case(4, false)]
fn test_should_enforce_total_text_limit_at_boundary(
    #[case] limit: usize,
    #[case] rejected: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = br#"<svg xmlns="http://www.w3.org/2000/svg"><text>a</text><text>b</text><text>c</text></svg>"#;
    let limits = ConversionLimits::builder()
        .max_single_text_bytes(1)
        .max_total_text_bytes(limit)
        .build()?;
    let result = convert(input, &ConversionOptions::builder().limits(limits).build());
    assert_eq!(
        matches!(
            result,
            Err(ConversionError::LimitExceeded {
                resource: LimitResource::XmlText,
                ..
            })
        ),
        rejected
    );
    Ok(())
}

#[rstest]
#[case(-1, true)]
#[case(0, false)]
#[case(1, false)]
fn test_should_enforce_decompressed_byte_limit_at_boundary(
    #[case] delta: isize,
    #[case] rejected: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><!--{}--></svg>",
        "x".repeat(1_000)
    );
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(input.as_bytes())?;
    let compressed = encoder.finish()?;
    let limit = usize::try_from(isize::try_from(input.len())?.saturating_add(delta))?;
    let limits = ConversionLimits::builder()
        .max_input_bytes(compressed.len())
        .max_decompressed_bytes(limit)
        .max_svgz_expansion_ratio(100)
        .build()?;
    let result = convert(
        &compressed,
        &ConversionOptions::builder().limits(limits).build(),
    );
    assert_eq!(
        matches!(
            result,
            Err(ConversionError::LimitExceeded {
                resource: LimitResource::DecompressedBytes,
                ..
            })
        ),
        rejected
    );
    Ok(())
}

#[rstest]
#[case(-1, true)]
#[case(0, false)]
#[case(1, false)]
fn test_should_enforce_data_url_lexical_limit_at_boundary(
    #[case] delta: isize,
    #[case] rejected: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let href = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
    let input = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg"><image width="1" height="1" href="{href}"/></svg>"#
    );
    let limit = usize::try_from(isize::try_from(href.len())?.saturating_add(delta))?;
    let limits = ConversionLimits::builder()
        .max_data_url_bytes(limit)
        .build()?;
    let result = convert(
        input.as_bytes(),
        &ConversionOptions::builder().limits(limits).build(),
    );
    if rejected {
        assert!(matches!(
            result,
            Err(ConversionError::LimitExceeded {
                resource: LimitResource::EmbeddedBytes,
                ..
            })
        ));
    } else {
        assert!(
            result.is_ok(),
            "unexpected accepted-boundary result: {result:?}"
        );
    }
    Ok(())
}

#[rstest]
#[case(1, false)]
#[case(2, false)]
#[case(3, true)]
fn test_should_enforce_reference_count_limit_at_boundary(
    #[case] references: usize,
    #[case] rejected: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::from(
        r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><rect id="a" width="1" height="1"/></defs>"#,
    );
    for _ in 0..references {
        input.push_str(r##"<use href="#a"/>"##);
    }
    input.push_str("</svg>");
    let limits = ConversionLimits::builder().max_references(2).build()?;
    let result = convert(
        input.as_bytes(),
        &ConversionOptions::builder().limits(limits).build(),
    );
    assert_eq!(
        matches!(
            result,
            Err(ConversionError::LimitExceeded {
                resource: LimitResource::References,
                limit: 2
            })
        ),
        rejected
    );
    Ok(())
}

#[rstest]
#[case(1, false)]
#[case(2, false)]
#[case(3, true)]
fn test_should_enforce_paint_node_limit_at_boundary(
    #[case] rectangles: usize,
    #[case] rejected: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::from(r#"<svg xmlns="http://www.w3.org/2000/svg">"#);
    for _ in 0..rectangles {
        input.push_str("<rect width=\"1\" height=\"1\"/>");
    }
    input.push_str("</svg>");
    let limits = ConversionLimits::builder().max_paint_nodes(3).build()?;
    let result = convert(
        input.as_bytes(),
        &ConversionOptions::builder().limits(limits).build(),
    );
    assert_eq!(
        matches!(
            result,
            Err(ConversionError::LimitExceeded {
                resource: LimitResource::PaintNodes,
                limit: 3
            })
        ),
        rejected
    );
    Ok(())
}

#[rstest]
#[case(1, false)]
#[case(2, false)]
#[case(3, true)]
fn test_should_enforce_input_image_limit_at_boundary(
    #[case] images: usize,
    #[case] rejected: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::from(r#"<svg xmlns="http://www.w3.org/2000/svg">"#);
    for _ in 0..images {
        input.push_str(r##"<image href="#missing" width="1" height="1"/>"##);
    }
    input.push_str("</svg>");
    let limits = ConversionLimits::builder().max_input_images(2).build()?;
    let result = convert(
        input.as_bytes(),
        &ConversionOptions::builder().limits(limits).build(),
    );
    if rejected {
        assert!(matches!(
            result,
            Err(ConversionError::LimitExceeded {
                resource: LimitResource::Images,
                limit: 2
            })
        ));
    } else {
        assert!(!matches!(
            result,
            Err(ConversionError::LimitExceeded {
                resource: LimitResource::Images,
                ..
            })
        ));
    }
    Ok(())
}

#[test]
fn test_should_reject_illegal_xml_control_character() {
    let input = b"<svg xmlns=\"http://www.w3.org/2000/svg\">\0</svg>";
    let result = convert(input, &ConversionOptions::default());
    assert!(matches!(
        result,
        Err(ConversionError::InputRejected(
            InputRejection::IllegalControlCharacter
        ))
    ));
}

#[test]
fn test_should_sanitize_malformed_xml_error() {
    let input = br#"<svg xmlns="http://www.w3.org/2000/svg"><path secret="credential">"#;
    let result = convert(input, &ConversionOptions::default());
    let rendered = format!("{result:?}");
    assert!(!rendered.contains("credential"));
    assert!(matches!(result, Err(ConversionError::MalformedXml { .. })));
}

#[test]
fn test_should_reject_raster_image_dimension_bomb_before_normalization()
-> Result<(), Box<dyn std::error::Error>> {
    let mut pixmap = resvg::tiny_skia::Pixmap::new(2, 2)
        .ok_or_else(|| std::io::Error::other("invalid test pixmap"))?;
    pixmap.fill(resvg::tiny_skia::Color::from_rgba8(0, 0, 0, 255));
    let mut png = pixmap.encode_png()?;
    png.get_mut(16..20)
        .ok_or_else(|| std::io::Error::other("missing PNG width"))?
        .copy_from_slice(&100_000_u32.to_be_bytes());
    png.get_mut(20..24)
        .ok_or_else(|| std::io::Error::other("missing PNG height"))?
        .copy_from_slice(&100_000_u32.to_be_bytes());
    let encoded = base64::engine::general_purpose::STANDARD.encode(png);
    let input = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><image width="20" height="20" href="data:image/png;base64,{encoded}"/></svg>"#
    );
    assert!(matches!(
        convert(input.as_bytes(), &ConversionOptions::default()),
        Err(ConversionError::LimitExceeded {
            resource: LimitResource::RasterPixels,
            ..
        })
    ));
    Ok(())
}

#[test]
fn test_should_reject_nested_svg_past_recursive_depth_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let mut nested =
        br##"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><rect width="1" height="1" fill="#000"/></svg>"##.to_vec();
    for _ in 0..9 {
        let encoded = base64::engine::general_purpose::STANDARD.encode(&nested);
        nested = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><image width="1" height="1" href="data:image/svg+xml;base64,{encoded}"/></svg>"#
        )
        .into_bytes();
    }
    let raster = RasterOptions::try_new(2.0, true)?;
    let options = ConversionOptions::builder().raster(raster).build();
    assert!(matches!(
        convert(&nested, &options),
        Err(ConversionError::LimitExceeded {
            resource: LimitResource::References,
            limit: 8
        })
    ));
    Ok(())
}

#[test]
fn test_should_reject_invalid_limit_relationships_at_build_time() {
    let result = ConversionLimits::builder().max_xml_elements(0).build();
    assert!(matches!(
        result,
        Err(ConversionError::InputRejected(
            InputRejection::InvalidOptions
        ))
    ));
}

#[test]
fn test_should_enforce_selected_json_limit_during_conversion()
-> Result<(), Box<dyn std::error::Error>> {
    let limits = ConversionLimits::builder()
        .max_serialized_json_bytes(64)
        .build()?;
    let options = ConversionOptions::builder().limits(limits).build();
    let input = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>"#;
    assert!(matches!(
        convert(input, &options),
        Err(ConversionError::LimitExceeded {
            resource: LimitResource::SerializedJson,
            limit: 64
        })
    ));
    Ok(())
}

#[test]
fn test_should_deny_external_urls_in_stylesheet_text() {
    let input = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
      <style>rect { fill: url(relative.png); }</style><rect width="10" height="10"/>
    </svg>"#;
    assert!(matches!(
        convert(input, &ConversionOptions::default()),
        Err(ConversionError::ResourceDenied { .. })
    ));
}

#[test]
fn test_should_deny_external_stylesheet_import() {
    let input = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
      <style>@import "theme.css"; rect { fill: red; }</style><rect width="10" height="10"/>
    </svg>"#;
    assert!(matches!(
        convert(input, &ConversionOptions::default()),
        Err(ConversionError::ResourceDenied { .. })
    ));
}

#[test]
fn test_should_not_treat_hyperlink_as_rendering_resource() {
    let input = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
      <a href="https://example.invalid"><rect width="10" height="10"/></a>
    </svg>"#;
    assert!(convert(input, &ConversionOptions::default()).is_ok());
}

#[test]
fn test_should_enforce_local_reference_depth() -> Result<(), Box<dyn std::error::Error>> {
    let limits = ConversionLimits::builder().max_reference_depth(2).build()?;
    let options = ConversionOptions::builder().limits(limits).build();
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
      <defs><g id="a"><use href="#b"/></g><g id="b"><use href="#c"/></g><g id="c"><rect width="1" height="1"/></g></defs>
      <use href="#a"/>
    </svg>"##;
    let conversion = convert(input, &options);
    assert!(
        matches!(
            conversion,
            Err(ConversionError::LimitExceeded {
                resource: LimitResource::References,
                limit: 2
            })
        ),
        "unexpected result: {conversion:?}"
    );
    Ok(())
}

#[rstest]
#[case(2, true)]
#[case(3, false)]
#[case(4, false)]
fn test_should_enforce_reference_depth_at_boundary(
    #[case] limit: usize,
    #[case] rejected: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let limits = ConversionLimits::builder()
        .max_reference_depth(limit)
        .build()?;
    let options = ConversionOptions::builder().limits(limits).build();
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
      <defs><g id="a"><use href="#b"/></g><g id="b"><use href="#c"/></g><g id="c"><rect width="1" height="1"/></g></defs>
      <use href="#a"/>
    </svg>"##;
    let result = convert(input, &options);
    assert_eq!(
        matches!(
            result,
            Err(ConversionError::LimitExceeded {
                resource: LimitResource::References,
                ..
            })
        ),
        rejected
    );
    Ok(())
}

#[rstest]
#[case(1, true)]
#[case(2, false)]
#[case(3, false)]
fn test_should_enforce_segments_per_path_at_boundary(
    #[case] limit: usize,
    #[case] rejected: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let limits = ConversionLimits::builder()
        .max_path_segments_per_path(limit)
        .build()?;
    let options = ConversionOptions::builder().limits(limits).build();
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0 L10 10" fill="none" stroke="#000"/></svg>"##;
    let result = convert(input, &options);
    assert_eq!(
        matches!(
            result,
            Err(ConversionError::LimitExceeded {
                resource: LimitResource::PathSegments,
                ..
            })
        ),
        rejected
    );
    Ok(())
}

#[rstest]
#[case(3, true)]
#[case(4, false)]
#[case(5, false)]
fn test_should_enforce_total_path_segments_at_boundary(
    #[case] limit: usize,
    #[case] rejected: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let limits = ConversionLimits::builder()
        .max_path_segments_per_path(2)
        .max_path_segments(limit)
        .build()?;
    let options = ConversionOptions::builder().limits(limits).build();
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0 L10 10" fill="none" stroke="#000"/><path d="M10 0 L0 10" fill="none" stroke="#000"/></svg>"##;
    let result = convert(input, &options);
    assert_eq!(
        matches!(
            result,
            Err(ConversionError::LimitExceeded {
                resource: LimitResource::PathSegments,
                ..
            })
        ),
        rejected
    );
    Ok(())
}

#[rstest]
#[case(1, true)]
#[case(2, false)]
#[case(3, false)]
fn test_should_enforce_target_point_limit_at_boundary(
    #[case] limit: usize,
    #[case] rejected: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let limits = ConversionLimits::builder()
        .max_target_points(limit)
        .build()?;
    let options = ConversionOptions::builder().limits(limits).build();
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0 L10 10" fill="none" stroke="#000"/></svg>"##;
    let result = convert(input, &options);
    assert_eq!(
        matches!(
            result,
            Err(ConversionError::LimitExceeded {
                resource: LimitResource::TargetPoints,
                ..
            })
        ),
        rejected
    );
    Ok(())
}

#[rstest]
#[case(1, false)]
#[case(2, false)]
#[case(3, true)]
fn test_should_enforce_target_element_limit_at_boundary(
    #[case] rectangles: usize,
    #[case] rejected: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::from(r#"<svg xmlns="http://www.w3.org/2000/svg">"#);
    for index in 0..rectangles {
        if write!(
            &mut input,
            r#"<rect x="{index}" width="1" height="1" fill="red"/>"#
        )
        .is_err()
        {
            return Err(std::io::Error::other("test input construction failed").into());
        }
    }
    input.push_str("</svg>");
    let limits = ConversionLimits::builder()
        .max_target_elements(2)
        .max_decomposition_elements(2)
        .build()?;
    let result = convert(
        input.as_bytes(),
        &ConversionOptions::builder().limits(limits).build(),
    );
    assert_eq!(
        matches!(
            result,
            Err(ConversionError::LimitExceeded {
                resource: LimitResource::TargetElements,
                limit: 2
            })
        ),
        rejected
    );
    Ok(())
}

#[rstest]
#[case(-1, true)]
#[case(0, false)]
#[case(1, false)]
fn test_should_enforce_embedded_output_limit_at_boundary(
    #[case] delta: isize,
    #[case] rejected: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><defs><linearGradient id="g"><stop stop-color="#f00"/><stop offset="1" stop-color="#00f"/></linearGradient></defs><rect width="20" height="20" fill="url(#g)"/></svg>"##;
    let baseline = convert(input, &ConversionOptions::default())?;
    let limit =
        usize::try_from(isize::try_from(baseline.report.embedded_bytes)?.saturating_add(delta))?;
    let limits = ConversionLimits::builder()
        .max_embedded_output_bytes(limit)
        .build()?;
    let result = convert(input, &ConversionOptions::builder().limits(limits).build());
    assert_eq!(
        matches!(
            result,
            Err(ConversionError::LimitExceeded {
                resource: LimitResource::EmbeddedBytes,
                ..
            })
        ),
        rejected
    );
    Ok(())
}

#[rstest]
#[case(-1, true)]
#[case(0, false)]
#[case(1, false)]
fn test_should_enforce_serialized_json_limit_at_boundary(
    #[case] delta: isize,
    #[case] rejected: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>"#;
    let baseline = convert(input, &ConversionOptions::default())?;
    let serialized = baseline.document.to_pretty_json()?;
    let limit = usize::try_from(isize::try_from(serialized.len())?.saturating_add(delta))?;
    let limits = ConversionLimits::builder()
        .max_serialized_json_bytes(limit)
        .build()?;
    let result = convert(input, &ConversionOptions::builder().limits(limits).build());
    assert_eq!(
        matches!(
            result,
            Err(ConversionError::LimitExceeded {
                resource: LimitResource::SerializedJson,
                ..
            })
        ),
        rejected
    );
    Ok(())
}

#[test]
fn test_should_reject_invalid_element_version_on_import() -> Result<(), Box<dyn std::error::Error>>
{
    let input = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>"#;
    let result = convert(input, &ConversionOptions::default())?;
    let mut value: serde_json::Value = serde_json::from_str(&result.document.to_pretty_json()?)?;
    let version = value
        .pointer_mut("/elements/0/version")
        .ok_or_else(|| std::io::Error::other("missing element version"))?;
    *version = serde_json::Value::from(2);
    let bytes = serde_json::to_vec(&value)?;
    assert!(matches!(
        ExcalidrawDocument::from_json(&bytes, &ConversionLimits::default()),
        Err(ConversionError::InvalidGeneratedDocument { .. })
    ));
    Ok(())
}

#[test]
fn test_should_reject_line_bindings_on_import() -> Result<(), Box<dyn std::error::Error>> {
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><line x1="1" y1="1" x2="19" y2="19" stroke="#000"/></svg>"##;
    let result = convert(input, &ConversionOptions::default())?;
    let mut value: serde_json::Value = serde_json::from_str(&result.document.to_pretty_json()?)?;
    let snapshot = value.to_string();
    let binding = value
        .pointer_mut("/elements/0/startBinding")
        .ok_or_else(|| std::io::Error::other(format!("missing binding field: {snapshot}")))?;
    *binding = serde_json::json!({
        "elementId": "bound-id",
        "focus": 0,
        "gap": 0,
        "fixedPoint": null
    });
    let bytes = serde_json::to_vec(&value)?;
    assert!(matches!(
        ExcalidrawDocument::from_json(&bytes, &ConversionLimits::default()),
        Err(ConversionError::InvalidGeneratedDocument { .. })
    ));
    Ok(())
}

#[test]
fn test_should_reject_singleton_group_on_import() -> Result<(), Box<dyn std::error::Error>> {
    let input = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>"#;
    let result = convert(input, &ConversionOptions::default())?;
    let mut value: serde_json::Value = serde_json::from_str(&result.document.to_pretty_json()?)?;
    let groups = value
        .pointer_mut("/elements/0/groupIds")
        .ok_or_else(|| std::io::Error::other("missing group field"))?;
    *groups = serde_json::json!(["one-member"]);
    let bytes = serde_json::to_vec(&value)?;
    assert!(matches!(
        ExcalidrawDocument::from_json(&bytes, &ConversionLimits::default()),
        Err(ConversionError::InvalidGeneratedDocument { .. })
    ));
    Ok(())
}

#[test]
fn test_should_reject_linear_bounds_mismatch_on_import() -> Result<(), Box<dyn std::error::Error>> {
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><line x1="1" y1="1" x2="19" y2="19" stroke="#000"/></svg>"##;
    let result = convert(input, &ConversionOptions::default())?;
    let mut value: serde_json::Value = serde_json::from_str(&result.document.to_pretty_json()?)?;
    let width = value
        .pointer_mut("/elements/0/width")
        .ok_or_else(|| std::io::Error::other("missing width"))?;
    *width = serde_json::Value::from(17.0);
    assert_invalid_document(&value)
}

#[test]
fn test_should_reject_invalid_app_state_on_import() -> Result<(), Box<dyn std::error::Error>> {
    let input = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>"#;
    let result = convert(input, &ConversionOptions::default())?;
    let mut value: serde_json::Value = serde_json::from_str(&result.document.to_pretty_json()?)?;
    let grid = value
        .pointer_mut("/appState/gridModeEnabled")
        .ok_or_else(|| std::io::Error::other("missing grid setting"))?;
    *grid = serde_json::Value::Bool(true);
    assert_invalid_document(&value)
}

#[test]
fn test_should_reject_malformed_provenance_on_import() -> Result<(), Box<dyn std::error::Error>> {
    let input = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>"#;
    let options = ConversionOptions::builder()
        .provenance(ProvenanceMode::Compact)
        .build();
    let result = convert(input, &options)?;
    let mut value: serde_json::Value = serde_json::from_str(&result.document.to_pretty_json()?)?;
    let source_key = value
        .pointer_mut("/elements/0/customData/svg2excal/sourceKey")
        .ok_or_else(|| std::io::Error::other("missing provenance"))?;
    *source_key = serde_json::Value::from("../../private");
    assert_invalid_document(&value)
}

#[test]
fn test_should_reject_non_content_addressed_png_on_import() -> Result<(), Box<dyn std::error::Error>>
{
    let input = br##"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="30">
      <defs><linearGradient id="g"><stop stop-color="#f00"/><stop offset="1" stop-color="#00f"/></linearGradient></defs>
      <rect width="30" height="30" fill="url(#g)"/>
    </svg>"##;
    let result = convert(input, &ConversionOptions::default())?;
    let mut value: serde_json::Value = serde_json::from_str(&result.document.to_pretty_json()?)?;
    let files = value
        .get_mut("files")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| std::io::Error::other("missing files"))?;
    let file = files
        .values_mut()
        .next()
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| std::io::Error::other("missing file"))?;
    let data_url = file
        .get("dataURL")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| std::io::Error::other("missing data URL"))?;
    let encoded = data_url
        .strip_prefix("data:image/png;base64,")
        .ok_or_else(|| std::io::Error::other("bad data URL"))?;
    let mut bytes = base64::engine::general_purpose::STANDARD.decode(encoded)?;
    bytes.push(0);
    file.insert(
        "dataURL".to_owned(),
        serde_json::Value::from(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )),
    );
    assert_invalid_document(&value)
}

fn assert_invalid_document(value: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    let bytes = serde_json::to_vec(value)?;
    assert!(matches!(
        ExcalidrawDocument::from_json(&bytes, &ConversionLimits::default()),
        Err(ConversionError::InvalidGeneratedDocument { .. })
    ));
    Ok(())
}
