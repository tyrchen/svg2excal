//! Checked cross-platform hashes for the canonical conversion corpus.

use svg2excal_core::{ConversionOptions, convert};

const ARCH: &[u8] = include_bytes!("../../../fixtures/arch.svg");
const ARCH_DOCUMENT_BLAKE3: &str =
    "36463694ddbf19948cb9c6107a01ae312f895e696b69557f794037c8255fdd5e";
const ARCH_REPORT_BLAKE3: &str = "92c81b7be6488835e0cbd1e6a2d17c100fad4be4c7365a89c54a13b5099a0a11";
const FALLBACK: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><defs><linearGradient id="g"><stop stop-color="#f00"/><stop offset="1" stop-color="#00f"/></linearGradient></defs><rect width="20" height="20" fill="url(#g)"/></svg>"##;
const FALLBACK_DOCUMENT_BLAKE3: &str =
    "ba1e1a4708a4e97332bccf93c9e194fdc2cad33eefe094ea9675efd8ab7b5767";
const FALLBACK_REPORT_BLAKE3: &str =
    "324aaa64ef9198bfd597ab819683d0b1b2518faee291db2d4ec3cab6f07e69f3";

#[test]
fn test_should_match_frozen_arch_document_and_report_hashes()
-> Result<(), Box<dyn std::error::Error>> {
    let options = ConversionOptions::default();
    let first = convert(ARCH, &options)?;
    let document = first.document.to_pretty_json_with_limits(&options.limits)?;
    let report = serde_json::to_vec_pretty(&first.report)?;
    assert_eq!(
        blake3::hash(document.as_bytes()).to_hex().as_str(),
        ARCH_DOCUMENT_BLAKE3
    );
    assert_eq!(blake3::hash(&report).to_hex().as_str(), ARCH_REPORT_BLAKE3);

    let second = convert(ARCH, &options)?;
    assert_eq!(
        document,
        second
            .document
            .to_pretty_json_with_limits(&options.limits)?
    );
    assert_eq!(report, serde_json::to_vec_pretty(&second.report)?);
    Ok(())
}

#[test]
fn test_should_match_frozen_fallback_png_document_and_report_hashes()
-> Result<(), Box<dyn std::error::Error>> {
    let options = ConversionOptions::default();
    let result = convert(FALLBACK, &options)?;
    assert_eq!(result.document.files().len(), 1);
    let document = result
        .document
        .to_pretty_json_with_limits(&options.limits)?;
    let report = serde_json::to_vec_pretty(&result.report)?;
    assert_eq!(
        blake3::hash(document.as_bytes()).to_hex().as_str(),
        FALLBACK_DOCUMENT_BLAKE3
    );
    assert_eq!(
        blake3::hash(&report).to_hex().as_str(),
        FALLBACK_REPORT_BLAKE3
    );
    Ok(())
}
