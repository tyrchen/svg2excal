#![no_main]
#![forbid(unsafe_code)]

use libfuzzer_sys::fuzz_target;
use svg2excal_core::{ConversionOptions, convert};

fuzz_target!(|data: &[u8]| {
    if let Ok(path) = std::str::from_utf8(data) {
        let escaped = path
            .replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let input = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><path d="{escaped}" fill="none" stroke="black"/></svg>"#
        );
        let _ = convert(input.as_bytes(), &ConversionOptions::default());
    }
});
