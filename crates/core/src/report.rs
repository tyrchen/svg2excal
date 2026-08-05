//! Deterministic conversion diagnostics and aggregate report.

use serde::{Deserialize, Serialize};

use crate::{ConversionProfile, ExcalidrawDocument};

/// Diagnostic severity ordered from informational through painted omission.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSeverity {
    /// No fidelity loss.
    Info,
    /// Bounded editable approximation.
    Approximation,
    /// Raster or explicit-geometry fallback.
    Fallback,
    /// Profile-authorized omission.
    Omission,
}

/// Stable diagnostic code.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticCode {
    /// A curve was flattened within its error budget.
    PathFlattened,
    /// A paint island was rasterized.
    PaintIslandRasterized,
    /// A recognized non-structural filter was omitted.
    FilterOmitted,
    /// A gradient required fallback.
    GradientRasterized,
    /// A mask required fallback.
    MaskRasterized,
    /// A clip required fallback.
    ClipRasterized,
    /// A deterministic target-compatible font replaced the source font.
    FontSubstituted,
    /// A font style was approximated.
    FontStyleApproximated,
    /// A dash pattern was classified to a target dash style.
    DashPatternApproximated,
    /// Stroke cap, join, miter, or dash semantics were approximated.
    StrokeStyleApproximated,
    /// A corner radius was approximated.
    CornerRadiusApproximated,
    /// Marker artwork remained explicit geometry.
    MarkerPreservedAsGeometry,
    /// No proximity-based binding was inferred.
    BindingNotInferred,
    /// Source-to-paint correlation was ambiguous.
    AmbiguousSourceCorrelation,
    /// Active source content was ignored.
    ActiveContentIgnored,
    /// Scene geometry was scaled to fit target restoration bounds.
    SceneScaledToTargetRange,
    /// An existing raster animation was frozen to its first frame.
    AnimatedImageSnapshot,
}

/// One bounded, deterministic conversion diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionDiagnostic {
    code: DiagnosticCode,
    severity: DiagnosticSeverity,
    source_order: u32,
    message: String,
    affected_target_ids: Vec<String>,
}

impl ConversionDiagnostic {
    pub(crate) fn new(
        code: DiagnosticCode,
        severity: DiagnosticSeverity,
        source_order: u32,
        message: &'static str,
    ) -> Self {
        Self {
            code,
            severity,
            source_order,
            message: message.to_owned(),
            affected_target_ids: Vec::new(),
        }
    }

    pub(crate) fn with_target(mut self, target_id: &str) -> Self {
        self.affected_target_ids.push(target_id.to_owned());
        self
    }

    /// Stable diagnostic code.
    #[must_use]
    pub const fn code(&self) -> DiagnosticCode {
        self.code
    }

    /// Fidelity severity.
    #[must_use]
    pub const fn severity(&self) -> DiagnosticSeverity {
        self.severity
    }

    /// Bounded human-readable message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Generated elements affected by the decision.
    #[must_use]
    pub fn affected_target_ids(&self) -> &[String] {
        &self.affected_target_ids
    }
}

/// Deterministic counts and decisions returned with a successful document.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionReport {
    /// Selected conversion profile.
    pub profile: ConversionProfileReport,
    /// Input byte count after compression, before bounded decompression.
    pub input_bytes: usize,
    /// Source XML element count.
    pub source_elements: usize,
    /// Local and external source reference count.
    pub source_references: usize,
    /// Normalized paint node count.
    pub paint_nodes: usize,
    /// Emitted target element count.
    pub target_elements: usize,
    /// Emitted target local point count.
    pub target_points: usize,
    /// Embedded PNG byte count.
    pub embedded_bytes: usize,
    /// Raster fallback pixel count.
    pub fallback_pixels: u64,
    /// Deterministic ordered diagnostics.
    pub diagnostics: Vec<ConversionDiagnostic>,
}

/// Serializable mirror of [`ConversionProfile`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConversionProfileReport {
    /// Balanced profile.
    Balanced,
    /// Editable profile.
    Editable,
    /// Fidelity profile.
    Fidelity,
    /// Strict profile.
    Strict,
}

impl From<ConversionProfile> for ConversionProfileReport {
    fn from(value: ConversionProfile) -> Self {
        match value {
            ConversionProfile::Balanced => Self::Balanced,
            ConversionProfile::Editable => Self::Editable,
            ConversionProfile::Fidelity => Self::Fidelity,
            ConversionProfile::Strict => Self::Strict,
        }
    }
}

/// Complete successful conversion output.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ConversionResult {
    /// Typed, validated Excalidraw document.
    pub document: ExcalidrawDocument,
    /// Deterministic diagnostics and aggregate counts.
    pub report: ConversionReport,
}

pub(crate) fn sort_diagnostics(diagnostics: &mut [ConversionDiagnostic]) {
    diagnostics.sort_by(|left, right| {
        left.source_order
            .cmp(&right.source_order)
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.affected_target_ids.cmp(&right.affected_target_ids))
    });
}
