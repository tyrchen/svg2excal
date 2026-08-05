#![no_main]
#![forbid(unsafe_code)]

use libfuzzer_sys::fuzz_target;
use svg2excal_core::{ConversionOptions, convert};

fuzz_target!(|data: &[u8]| {
    let _ = convert(data, &ConversionOptions::default());
});
