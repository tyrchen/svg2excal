# Excalidraw Emission Design

Status: ready for implementation v1 · Owner: svg2excal maintainers · Depends on: scene model, SVG ingestion, mapping planner

## 1. Purpose

This subsystem converts a fully budgeted `LoweringPlan` into typed Excalidraw v2 data, validates all target invariants, and serializes deterministic JSON. It owns target schema compatibility, IDs/seeds/order, files, provenance, and referential integrity. It does not revisit SVG interpretation or mapping policy.

## 2. Target profile

The initial target profile is:

```text
Excalidraw envelope type:    "excalidraw"
Envelope version:            2
Schema reference commit:     ab0255f21eb40b5408f3e9ed9725474108eda9e6
Source:                      https://github.com/tyrchen/svg2excal
```

The profile is explicit even though the envelope version remains `2` across upstream element-field evolution. Compatibility tests pin current required fields and restoration behavior. A future profile is additive; existing profile serialization does not change silently.

## 3. Document envelope

```rust
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcalidrawDocument {
    r#type: ExcalidrawDocumentType,
    version: u32,
    source: SourceUrl,
    elements: Vec<ExcalidrawElement>,
    app_state: ExcalidrawAppState,
    files: BTreeMap<FileId, BinaryFile>,
}
```

The JSON shape follows the upstream exported-data contract. `appState` contains only stable import-relevant fields:

```json
{
  "viewBackgroundColor": "#ffffff",
  "gridModeEnabled": false
}
```

The source canvas background remains an ordinary bottom-most element unless an explicit future option promotes an exact full-viewport solid fill. This preserves paint order and editability.

## 4. Common element fields

Every variant embeds one typed base:

```rust
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementBase {
    id: ElementId,
    x: FiniteCoordinate,
    y: FiniteCoordinate,
    width: NonNegativeFiniteLength,
    height: NonNegativeFiniteLength,
    angle: Radians,
    stroke_color: ExcalidrawColor,
    background_color: ExcalidrawColor,
    fill_style: FillStyle,
    stroke_width: NonNegativeFiniteLength,
    stroke_style: StrokeStyle,
    roundness: Roundness,
    roughness: u8,
    opacity: OpacityPercent,
    group_ids: Vec<GroupId>,
    frame_id: Option<ElementId>,
    index: Option<FractionalIndex>,
    seed: NonZeroRoughSeed,
    version: NonZeroU32,
    version_nonce: ExcalidrawRandom,
    is_deleted: bool,
    bound_elements: Option<Vec<BoundElement>>,
    updated: StableTimestamp,
    link: Option<BoundedUrl>,
    locked: bool,
    custom_data: Option<Svg2ExcalCustomDataEnvelope>,
}
```

Binding defaults for imported SVG are:

- `roughness: 0`
- `fillStyle: "solid"`
- `version: 1`
- `isDeleted: false`
- `frameId: null`
- `boundElements: null`
- `link: null`
- `locked: false`
- `index: null`
- `updated: 1`

`index: null` is deliberate: it is accepted by the exported element type and upstream restore deterministically repairs ordering from array order. The converter's element array is authoritative paint order. A compatibility test detects any future profile that requires explicit fractional indices.

Colors serialize canonically as lowercase `#rrggbb` when opaque or `transparent` when absent. Separate source alpha is represented through element opacity or element splitting only when the mapping plan proved equivalence.

## 5. Element variants

The Rust enum is internally tagged by `type` and uses only current upstream names. Every variant flattens `ElementBase` into the element object; the base is never serialized as a nested `base` key.

```rust
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ExcalidrawElement {
    Rectangle(ShapeElement),
    Diamond(ShapeElement),
    Ellipse(ShapeElement),
    Line(LineElement),
    Arrow(ArrowElement),
    Text(TextElement),
    Image(ImageElement),
}
```

### Rectangle, ellipse, diamond

Generic shapes add no fields beyond the base. `roundness` is null or `{ "type": 3, "value": radius }` only when the mapping predicate selected an exact/approved radius.

### Line

```rust
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineElement {
    #[serde(flatten)]
    base: ElementBase,
    points: Vec<LocalPoint>,
    start_binding: Option<FixedPointBinding>,
    end_binding: Option<FixedPointBinding>,
    start_arrowhead: Option<Arrowhead>,
    end_arrowhead: Option<Arrowhead>,
    polygon: bool,
}
```

Both arrowhead fields serialize explicitly as `null` for a line. `points` begins with `[0, 0]`, has at least two finite distinct points, and is consistent with width/height. When `polygon` is true, the final point is also `[0, 0]` and the vector has at least four entries, as required by the pinned target restore/type guard.

### Arrow

Arrow adds `elbowed`. V1 emits `elbowed: false`; orthogonal source routes remain ordinary multi-point arrows because target elbow arrows carry editing semantics and fixed-segment state not present in SVG. Every start/end arrowhead field is explicit, including null. Bindings are always null in the v1 target profile.

### Text

Text adds `fontSize`, numeric `fontFamily`, `text`, `originalText`, `textAlign`, `verticalAlign`, `containerId`, `autoResize`, and unitless `lineHeight`. Width/height and x/y are measured target-font box values, not copied SVG baselines.

### Image

Image adds `fileId`, `status: "saved"`, `scale: [1, 1]` unless an exact source image reflection is retained, and `crop: null` unless an exact target crop plan exists. Its file must exist in `files` and share the same `FileId` value.

Freedraw, frame, magic frame, iframe, and embeddable are not emitted in v1. Their serde variants are not exposed until a mapping owns their complete invariant set.

## 6. Binary files

```rust
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryFile {
    mime_type: ImageMimeType,
    id: FileId,
    data_url: BoundedDataUrl,
    created: StableTimestamp,
    version: NonZeroU32,
}
```

`created` is `1` and file `version` is `1` for deterministic generated content. PNG is the sole v1 output file format. Every accepted JPEG/PNG/GIF/WebP is dimension-probed, bounded-decoded to the static source snapshot, converted to canonical sRGB pixels, stripped of metadata, and deterministically re-encoded as PNG. Animated input uses its defined first-frame snapshot with `animated-image-snapshot` in non-strict profiles; strict reports a fidelity violation. Original compressed bytes are never copied into the target file map.

The `files` map is a `BTreeMap` so JSON key order is stable. Unreferenced files are rejected by final validation.

## 7. Deterministic identity

All generated identities use domain-separated BLAKE3 digests over canonical binary fields:

```text
element-id = H("svg2excal/element/v1", documentDigest, sourceKey, occurrence, role)
group-id   = H("svg2excal/group/v1",   documentDigest, sourceKey, groupDepth)
file-id    = H("svg2excal/file/v1",    mimeType, rawFileBytes)
seed       = 1 + (low31(H("svg2excal/seed/v1", element-id)) mod (2^31 - 1))
nonce      = low31(H("svg2excal/nonce/v1", element-id))
```

IDs use unpadded URL-safe base64 of the first 15 digest bytes (20 characters), matching the practical shape of upstream IDs without relying on randomness. Collision detection is mandatory; an observed collision returns `InvalidGeneratedDocument` rather than adding nondeterministic salt.

The seed range is deliberately `1..=2^31-1`: RoughJS interprets seed `0` as a request to use `Math.random()`, which would break render determinism even though zero is structurally accepted by Excalidraw. Version nonces remain in `0..=2^31-1` because they do not seed rendering.

Inputs to the hash are length-prefixed fixed-endian fields, never concatenated ambiguous strings. Original source IDs are provenance, not target IDs.

## 8. Provenance

When `ProvenanceMode::Compact` is enabled, elements may include:

```json
{
  "svg2excal": {
    "version": 1,
    "sourceKey": "...",
    "sourceTag": "path",
    "mapping": "approximate",
    "diagnosticCodes": ["path-flattened"]
  }
}
```

The envelope has a strict serialized-size cap. It never embeds raw SVG, full path data, full text, credentials, URLs, or unbounded attribute values. `ProvenanceMode::None` omits `customData` completely.

## 9. Emission order

The builder runs in dependency order:

1. allocate deterministic element/group/file IDs from the complete plan;
2. emit unbound shapes, lines, text, and images in paint order;
3. emit arrows in paint order with explicit null bindings;
4. attach deepest-to-shallowest groups without reordering elements;
5. build the referenced file map;
6. validate the whole document;
7. serialize.

No partially linked public document exists between steps.

## 10. Final validator

Validation MUST check:

- envelope type/version/source and allowed app-state fields;
- unique element/group/file IDs and deterministic order;
- finite x/y/width/height/angle/stroke/points and target coordinate bounds;
- opacity 0–100, seed 1–`2^31 - 1`, nonce 0–`2^31 - 1`, version nonzero;
- allowed enum values and complete variant-specific fields;
- line/arrow point count, `[0,0]` origin, polygon closure rules, explicit arrowheads;
- text and originalText are nonempty after SVG whitespace semantics, byte-bounded, and mutually consistent; nonpainting empty source text is omitted before emission; measured dimensions, font ID, and line height are valid;
- every image file reference exists and every file is referenced;
- group IDs are deepest-to-shallowest and contain at least two elements;
- binding, container, and frame fields are null under the v1 profile;
- no deleted elements, frames, links, bound elements, or unsupported variants are accidentally emitted;
- serialized JSON and embedded-byte budgets.

Validation returns a typed path such as `elements[42].points[7]` without indexing unsafely or leaking source text.

## 11. Serialization

Serde models use `#[serde(rename_all = "camelCase")]` and individual renames where required. The implementation does not build scene JSON through `serde_json::Value`. Pretty JSON uses two-space indentation, LF line endings, stable struct-field order, stable `BTreeMap` key order, and one terminal newline.

Before target validation, numeric fields pass through the pinned target profile's canonical-number policy:

- coordinates, lengths, local points, font sizes, and stroke widths round to nearest-ties-even on a `10^-6` CSS-pixel grid;
- angles round to nearest-ties-even on a `10^-12` radian grid;
- negative zero canonicalizes to positive zero;
- classification/error metrics use the same canonical inputs and take the more conservative branch when a value lies within one canonical unit of a decision threshold.

The sub-micropixel rounding error is included in geometry budgets. It prevents harmless platform math differences from changing JSON bytes or flipping a native/fallback decision. Determinism is scoped to identical input/options/explicit resource bytes, converter version, target profile, and bundled assets across the supported CI platforms; version/profile changes may intentionally change bytes.

Serialization is tested byte-for-byte. Deserializing the generated JSON into our typed model and reserializing MUST be idempotent.

## 12. Relevant AGENTS.md sections

- Serialization & Data: typed serde, camelCase, validation — binding.
- Type Design & API: specific newtypes, non-exhaustive public types, illegal states unrepresentable — binding.
- Error Handling/Safety: typed errors, no panic/indexing/unsafe, checked bounds — binding.
- Documentation/Testing: public docs/examples and exact error-path tests — binding.
- Async & Concurrency: N/A; emission is synchronous and deterministic.
- Logging: N/A in library; no logging of document content.

## 13. Cross-references

- ← [Mapping design](./svg-mapping-design.md)
- ↔ [Verification plan](./svg-to-excalidraw-verification-plan.md)
- ↔ [Key decisions](./svg-to-excalidraw-key-decisions.md)
