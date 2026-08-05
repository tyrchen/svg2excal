//! Sanitized library error types.

use thiserror::Error;

use crate::ConversionDiagnostic;

/// Input-format rejection categories.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputRejection {
    /// Input was empty.
    Empty,
    /// Input was not valid UTF-8 after bounded decompression.
    InvalidUtf8,
    /// XML included a DTD or entity declaration.
    DtdOrEntity,
    /// XML included a disallowed control character.
    IllegalControlCharacter,
    /// Caller-supplied options violated their documented range.
    InvalidOptions,
}

impl std::fmt::Display for InputRejection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "input is empty",
            Self::InvalidUtf8 => "input is not valid UTF-8",
            Self::DtdOrEntity => "DTD and entity declarations are not allowed",
            Self::IllegalControlCharacter => "input contains an illegal XML control character",
            Self::InvalidOptions => "conversion options are outside their valid range",
        })
    }
}

/// Resource whose hard bound was exceeded.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitResource {
    /// Input bytes.
    InputBytes,
    /// Decompressed SVGZ bytes or expansion ratio.
    DecompressedBytes,
    /// XML element nodes.
    XmlElements,
    /// XML nesting depth.
    XmlDepth,
    /// Attributes on one element or in aggregate.
    XmlAttributes,
    /// XML text and attribute bytes.
    XmlText,
    /// Reference count or depth.
    References,
    /// Input image elements.
    Images,
    /// Normalized paint nodes.
    PaintNodes,
    /// Path segments.
    PathSegments,
    /// Generated target elements.
    TargetElements,
    /// Generated target local points.
    TargetPoints,
    /// Raster pixels.
    RasterPixels,
    /// Embedded bytes.
    EmbeddedBytes,
    /// Serialized JSON bytes.
    SerializedJson,
    /// Deterministic work units.
    WorkUnits,
}

impl std::fmt::Display for LimitResource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

/// Fatal conversion error. No variant contains unbounded source text.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum ConversionError {
    /// A caller requested cooperative cancellation.
    #[error("conversion cancelled")]
    Cancelled,
    /// Input was rejected before XML normalization.
    #[error("input rejected: {0}")]
    InputRejected(InputRejection),
    /// XML parsing failed; the message contains only a bounded category and position.
    #[error("malformed XML at line {line}, column {column}: {category}")]
    MalformedXml {
        /// Safe parser category.
        category: &'static str,
        /// One-based source line.
        line: u32,
        /// One-based source column.
        column: u32,
    },
    /// The document root is not an SVG-namespace `svg` element.
    #[error("document root is not an SVG element")]
    UnsupportedRoot,
    /// A path or URL resource was denied by policy.
    #[error("external resource denied ({kind})")]
    ResourceDenied {
        /// Bounded resource category, never the raw URL/path.
        kind: &'static str,
    },
    /// A hard resource limit was exceeded.
    #[error("limit exceeded for {resource}: limit {limit}")]
    LimitExceeded {
        /// Limited resource.
        resource: LimitResource,
        /// Configured limit.
        limit: u64,
    },
    /// `usvg` could not normalize the validated document.
    #[error("SVG normalization failed: {category}")]
    NormalizationFailed {
        /// Sanitized upstream error category.
        category: &'static str,
    },
    /// Input geometry was non-finite or overflowed a target bound.
    #[error("geometry is non-finite or outside target bounds")]
    GeometryOverflow,
    /// A fallback island could not be rendered or encoded.
    #[error("raster fallback failed: {category}")]
    RasterizationFailed {
        /// Bounded failure category.
        category: &'static str,
    },
    /// Strict mode found one or more non-native fidelity decisions.
    #[error("strict profile rejected non-native painted content")]
    StrictFidelityViolation {
        /// Complete deterministic fidelity diagnostics.
        diagnostics: Vec<ConversionDiagnostic>,
    },
    /// The generated target graph violated an Excalidraw invariant.
    #[error("generated Excalidraw document is invalid: {category}")]
    InvalidGeneratedDocument {
        /// Bounded invariant category.
        category: &'static str,
    },
}

impl ConversionError {
    /// Stable machine-facing error code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::InputRejected(_) => "input-rejected",
            Self::MalformedXml { .. } => "malformed-xml",
            Self::UnsupportedRoot => "unsupported-root",
            Self::ResourceDenied { .. } => "resource-denied",
            Self::LimitExceeded { .. } => "limit-exceeded",
            Self::NormalizationFailed { .. } => "normalization-failed",
            Self::GeometryOverflow => "geometry-overflow",
            Self::RasterizationFailed { .. } => "rasterization-failed",
            Self::StrictFidelityViolation { .. } => "strict-fidelity-violation",
            Self::InvalidGeneratedDocument { .. } => "invalid-generated-document",
        }
    }
}
