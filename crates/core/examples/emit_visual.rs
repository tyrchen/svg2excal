//! Emits the canonical architecture conversion for the visual compatibility gate.

// This synchronous developer tool intentionally uses blocking filesystem I/O.
#![allow(clippy::disallowed_methods)]

use std::{error::Error, fs};

use svg2excal_core::{ConversionOptions, convert};

fn main() -> Result<(), Box<dyn Error>> {
    let input = fs::read("fixtures/arch.svg")?;
    let options = ConversionOptions::default();
    let result = convert(&input, &options)?;
    fs::create_dir_all("target/visual")?;
    fs::write(
        "target/visual/arch.excalidraw",
        result
            .document
            .to_pretty_json_with_limits(&options.limits)?,
    )?;
    fs::write(
        "target/visual/arch-report.json",
        serde_json::to_vec_pretty(&result.report)?,
    )?;
    Ok(())
}
