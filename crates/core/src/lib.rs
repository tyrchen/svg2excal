//! Deterministic, security-bounded SVG to Excalidraw conversion.
//!
//! The crate parses SVG or SVGZ bytes without external I/O, resolves SVG paint
//! semantics through `usvg`, and emits a typed Excalidraw v2 document. Use
//! [`convert`] for the deny-by-default path or [`convert_with_resources`] when
//! a caller intentionally provides bounded relative resources.
//!
//! # Example
//!
//! ```
//! use svg2excal_core::{convert, ConversionOptions};
//!
//! let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10">
//!   <rect width="20" height="10" fill="#4dabf7"/>
//! </svg>"##;
//! let result = convert(svg, &ConversionOptions::default())?;
//! let json = result.document.to_pretty_json()?;
//! assert!(json.contains("\"type\": \"rectangle\""));
//! # Ok::<(), svg2excal_core::ConversionError>(())
//! ```

#![forbid(unsafe_code)]
#![warn(rust_2024_compatibility, missing_docs, missing_debug_implementations)]

mod convert;
mod error;
mod identity;
mod ingest;
mod options;
mod report;
mod resource;
mod target;

pub use convert::{convert, convert_with_resources};
pub use error::{ConversionError, InputRejection, LimitResource};
pub use options::{
    ConversionLimits, ConversionOptions, ConversionProfile, FontOptions, GeometryOptions,
    ProvenanceMode, RasterOptions,
};
pub use report::{
    ConversionDiagnostic, ConversionReport, ConversionResult, DiagnosticCode, DiagnosticSeverity,
};
pub use resource::{
    ProvidedResource, ProvidedResourcePolicy, RelativeFilePolicy, ResourceContext, ResourceError,
    ResourceProvider, ResourceRequest,
};
pub use target::{
    BinaryFile, ExcalidrawAppState, ExcalidrawDocument, ExcalidrawElement, FileId, GroupId,
    ImageMimeType,
};
