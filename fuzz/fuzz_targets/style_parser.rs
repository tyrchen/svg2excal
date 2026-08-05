#![no_main]
#![forbid(unsafe_code)]

use libfuzzer_sys::fuzz_target;
use svg2excal_core::{ConversionOptions, convert};

fuzz_target!(|data: &[u8]| {
    if let Ok(style) = std::str::from_utf8(data) {
        let escaped = style
            .replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let input = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><path style="{escaped}" d="M0 0H10V10Z"/></svg>"#
        );
        let _ = convert(input.as_bytes(), &ConversionOptions::default());
    }
});
