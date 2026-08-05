#![no_main]
#![forbid(unsafe_code)]

use libfuzzer_sys::fuzz_target;
use svg2excal_core::{ConversionLimits, ExcalidrawDocument};

fuzz_target!(|data: &[u8]| {
    let _ = ExcalidrawDocument::from_json(data, &ConversionLimits::default());
});
