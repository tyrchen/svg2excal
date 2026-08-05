//! Emits a deterministic generated scene for the pinned upstream compatibility harness.

// This synchronous build-time fixture emitter intentionally uses blocking filesystem I/O.
#![allow(clippy::disallowed_methods)]

use std::{error::Error, fs};

use svg2excal_core::{ConversionOptions, ProvenanceMode, convert};

fn main() -> Result<(), Box<dyn Error>> {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="120">
      <defs><linearGradient id="g"><stop stop-color="#ff6b6b"/><stop offset="1" stop-color="#4dabf7"/></linearGradient></defs>
      <rect x="10" y="10" width="80" height="60" fill="#a5d8ff"/>
      <path d="M110 15 L190 15 L150 70 Z" fill="none" stroke="#1e1e1e" stroke-linecap="round" stroke-linejoin="round"/>
      <rect x="195" y="75" width="35" height="30" fill="url(#g)"/>
    </svg>"##;
    let options = ConversionOptions::builder()
        .provenance(ProvenanceMode::Compact)
        .build();
    let result = convert(svg, &options)?;
    fs::create_dir_all("target/compat")?;
    fs::write(
        "target/compat/generated-minimal.excalidraw",
        result
            .document
            .to_pretty_json_with_limits(&options.limits)?,
    )?;
    Ok(())
}
