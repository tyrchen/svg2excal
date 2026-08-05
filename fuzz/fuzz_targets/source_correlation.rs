#![no_main]
#![forbid(unsafe_code)]

use libfuzzer_sys::fuzz_target;
use svg2excal_core::{ConversionOptions, convert};

fuzz_target!(|data: &[u8]| {
    if let Ok(fragment) = std::str::from_utf8(data) {
        let input = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><defs>{fragment}</defs><use href="#a"/></svg>"##
        );
        let _ = convert(input.as_bytes(), &ConversionOptions::default());
    }
});
