# Conversion Scene Model Design

Status: ready for implementation v1 · Owner: svg2excal maintainers · Depends on: PRD

## 1. Purpose

This specification defines the types shared by parsing, normalization, planning, emission, and reporting. It owns invariants and stage transitions; it does not own XML parsing or target serialization.

## 2. Public API

```rust
use std::fmt::Debug;

pub fn convert(
    svg: &[u8],
    options: &ConversionOptions,
) -> Result<ConversionResult, ConversionError>;

pub fn convert_with_resources(
    svg: &[u8],
    options: &ConversionOptions,
    resources: &ResourceContext<'_>,
) -> Result<ConversionResult, ConversionError>;

#[non_exhaustive]
#[derive(Debug)]
pub struct ConversionResult {
    pub document: ExcalidrawDocument,
    pub report: ConversionReport,
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ConversionOptions {
    pub profile: ConversionProfile,
    pub limits: ConversionLimits,
    pub geometry: GeometryOptions,
    pub raster: RasterOptions,
    pub fonts: FontOptions,
    pub provenance: ProvenanceMode,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionProfile {
    Balanced,
    Editable,
    Fidelity,
    Strict,
}

#[derive(Debug)]
pub struct ResourceContext<'a> {
    policy: ProvidedResourcePolicy,
    provider: &'a dyn ResourceProvider,
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum ProvidedResourcePolicy {
    RelativeFiles(RelativeFilePolicy),
}

pub trait ResourceProvider: Debug + Send + Sync {
    fn load(
        &self,
        request: &ResourceRequest,
    ) -> Result<ProvidedResource, ResourceError>;
}
```

`convert` is the ordinary API and unconditionally denies external path/URL resources. `convert_with_resources` is the only external-resource entry point; its `ResourceContext` couples a validated allow policy with a provider. The core validates and budgets each `ResourceRequest` before invoking the provider, then revalidates MIME, magic, and returned byte count. A provider cannot widen the policy, and no v1 context admits network URLs.

`ConversionOptions` has more than five fields and MUST use `typed-builder`, per `AGENTS.md` § Type Design & API. Defaults are safe and deterministic. Validated numeric options use private-field newtypes and `TryFrom`; no public option accepts NaN, infinity, zero where invalid, or an unbounded collection. Provider-facing request/resource/error types are bounded domain types with redacted `Debug`; `ResourceProvider` is intentionally synchronous and object-safe so application adapters can supply explicitly authorized bytes without coupling the core to an async runtime.

## 3. Stage model

```text
ValidatedInput
      │ parse/preflight
      ▼
SourceDocument ─────────────┐
                           │ correlate
ResolvedPaintDocument ─────┤
                           ▼
                    CorrelatedScene
                           │ classify/plan
                           ▼
                     LoweringPlan
                           │ emit
                           ▼
                 ExcalidrawDocument
                           │ validate
                           ▼
                   ConversionResult
```

Each stage is a distinct non-exhaustive type with private fields. Construction is possible only through the preceding stage, making it impossible to emit from unvalidated XML or an unclassified paint tree.

## 4. Source model

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceKey {
    document_digest: DocumentDigest,
    preorder_index: u32,
    byte_start: u32,
}

#[non_exhaustive]
#[derive(Debug)]
pub struct SourceNode {
    key: SourceKey,
    kind: SvgElementKind,
    source_id: SourceId,
    byte_range: SourceRange,
    parent: SourceParent,
    children: Vec<SourceKey>,
    reference_edges: Vec<ReferenceEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SourceParent {
    DocumentRoot,
    Node(SourceKey),
}
```

`SourceId` explicitly distinguishes absent, unique, and duplicate source IDs. It is never used as a unique key. Text content, attribute values, URLs, path data, stylesheets, and custom metadata are length-bounded before entering domain types.

The source model preserves:

- original element kind and document order;
- explicit `<g>`, nested `<svg>`, `<symbol>`, and instance boundaries;
- definitions and local fragment references;
- source byte ranges for diagnostics;
- author-facing text and accessibility strings;
- a feature census, including unsupported/ignored constructs.

It does not declare computed style or rendered bounds authoritative.

## 5. Resolved paint model

```rust
#[non_exhaustive]
#[derive(Debug)]
pub enum PaintNode {
    Group(PaintGroup),
    Path(PaintPath),
    Text(PaintText),
    Image(PaintImage),
}

#[non_exhaustive]
#[derive(Debug)]
pub struct PaintGroup {
    transform: Affine2,
    bounds: LayerBounds,
    compositing: Compositing,
    children: Vec<PaintNodeId>,
}
```

The paint model adapts `usvg::Tree` without exposing dependency types in the public API. It contains finite `f64` coordinates, absolute and local transforms, tight and stroke/layer bounds, resolved paint/stroke/text/image data, and explicit compositing/effect state.

All input `f32` values from `usvg` are checked and widened to `f64`. Arithmetic touching input uses checked helpers and returns `GeometryOverflow` rather than panicking or silently wrapping, per `AGENTS.md` § Safety & Security.

## 6. Correlation model

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrelationConfidence {
    Exact,
    UniqueFingerprint,
    Ambiguous,
    Generated,
}

#[derive(Debug)]
pub struct Correlation {
    source: SourceKey,
    paint_nodes: Vec<PaintNodeId>,
    confidence: CorrelationConfidence,
    expansion: ExpansionKind,
}
```

Correlation is one-to-zero, one-to-one, or one-to-many. It uses paired traversal, explicit unique IDs, hierarchy, geometry fingerprints, style fingerprints, and expansion context. It MUST NOT promote semantics when confidence is `Ambiguous` or `Generated`. Exact visual conversion remains possible from the paint tree without semantic promotion.

## 7. Paint islands and lowering decisions

```rust
#[non_exhaustive]
#[derive(Debug)]
pub enum LoweringDecision {
    Native(NativePlan),
    Approximate(ApproximationPlan),
    Rasterize(RasterPlan),
    Omit(OmissionPlan),
}
```

A `PaintIslandId` names the smallest independently compositable subtree. Every effective painted node belongs to exactly one plan decision. A decision includes estimated output elements, points, pixels, fidelity impact, source correlations, and diagnostics before emission. The planner reserves all budgets atomically; emission cannot discover an unbudgeted explosion.

`Omit` is legal only for non-painted metadata/active content, a profile-authorized non-structural effect, or content wholly outside the effective viewport. Painted visible content is never silently omitted.

## 8. Diagnostics and report

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagnosticSeverity {
    Info,
    Approximation,
    Fallback,
    Omission,
}

#[derive(Debug)]
pub struct ConversionDiagnostic {
    code: DiagnosticCode,
    severity: DiagnosticSeverity,
    source: SourceLocation,
    affected_target_ids: Vec<ElementId>,
    message: BoundedDiagnosticMessage,
    mitigation: Mitigation,
}
```

Diagnostics expose documented read-only accessors; construction remains internal so unbounded messages or inconsistent source/target references cannot enter a report. Diagnostics are deterministically ordered by source byte position, then code, then target ID. Messages are bounded and escape control characters before logging or serialization. The report also includes input/output counts, canonical fixed-precision painted-area fractions by decision, deterministic work units by stage, peak logical-budget counters, font/resource decisions, and target profile. It deliberately excludes wall-clock timings and process-memory readings so identical conversions produce identical reports.

Strict mode runs the complete planner so it can return the full ordered set of fidelity violations; it emits no document when any approximation, fallback, or painted omission exists.

## 9. Error model

`svg2excal-core` defines a non-exhaustive `ConversionError` enum using `thiserror`, with source errors attached through `#[source]`, per `AGENTS.md` § Error Handling. A dependency error originating from hostile content is first converted to a bounded sanitized source-error type containing category and safe line/column metadata; raw attribute/text/path/URL content is not retained in `Display`, `Debug`, or the source chain. Required top-level variants:

- `InputRejected`
- `MalformedXml`
- `UnsupportedRoot`
- `ResourceDenied`
- `LimitExceeded`
- `NormalizationFailed`
- `GeometryOverflow`
- `RasterizationFailed`
- `StrictFidelityViolation`
- `InvalidGeneratedDocument`

No production path uses `unwrap`, `expect`, indexing on untrusted data, `panic`, or `unsafe`. Library functions return `Result`; application adapters add `anyhow::Context` at I/O boundaries.

## 10. Relevant AGENTS.md sections

- Error Handling: `thiserror`, source chains, `Result` — binding.
- Type Design & API: newtypes, non-exhaustive public types, builders — binding.
- Safety & Security: no unsafe/panics, checked arithmetic, boundary validation — binding.
- Serialization & Data: typed serde models, camelCase output, immediate validation — binding.
- Async & Concurrency: conversion is synchronous. The default path is CPU-bound and I/O-free; explicit providers may perform bounded synchronous reads, so service adapters use `spawn_blocking`, timeouts, and bounded concurrency around the whole operation.
- Logging & Observability: core returns diagnostics and does not log source values; apps emit structured, redacted spans.
- Testing/Documentation/Performance: binding; every public item is documented and critical invariants have unit/property tests.

## 11. Cross-references

- ← [PRD](./svg-to-excalidraw-prd.md)
- → [SVG ingestion](./svg-ingestion-design.md)
- → [Mapping design](./svg-mapping-design.md)
- → [Excalidraw emission](./excalidraw-emission-design.md)
- ↔ [Security and performance](./svg-to-excalidraw-security-performance.md)
