# SVG Mapping and Fallback Design

Status: ready for implementation v1 · Owner: svg2excal maintainers · Depends on: scene model, SVG ingestion

## 1. Purpose

The mapping planner classifies each resolved paint island as native, bounded approximation, minimal raster fallback, or allowed omission. It owns fidelity profiles, primitive recognition, path flattening, text compatibility, marker promotion, grouping, and budget reservation. It does not serialize Excalidraw JSON.

## 2. Planning flow

```text
CorrelatedScene
      │
      ▼
┌──────────────────────────────┐
│ Discover isolation boundaries│
│ filter/mask/clip/blend/opacity│
└──────────────┬───────────────┘
               ▼
┌──────────────────────────────┐
│ Try exact native predicate   │──── success ───▶ NativePlan
└──────────────┬───────────────┘
               │ no
               ▼
┌──────────────────────────────┐
│ Try profile approximation    │──── in budget ─▶ ApproximationPlan
└──────────────┬───────────────┘
               │ no / fidelity profile
               ▼
┌──────────────────────────────┐
│ Select smallest complete     │
│ isolation ancestor           │───────────────▶ RasterPlan
└──────────────────────────────┘

Every branch reserves target element/point/pixel/file budgets before emission.
```

## 3. Profile policy

| Construct | Balanced | Editable | Fidelity | Strict |
| --- | --- | --- | --- | --- |
| Exact native primitive/text | Native | Native | Native | Native |
| Curved open path within point/error budget | Flatten | Flatten | Raster | Error |
| Simple radius/dash/font mismatch within threshold | Approximate | Approximate within editable threshold | Raster when visible | Error |
| Gradient/pattern/compound fill | Raster island | Vector decomposition only if within element/error budget; else raster | Raster island | Error |
| Filter/mask/complex clip/blend | Omit only a recognized source-preserving cosmetic shadow within bounds; otherwise raster isolation group | Omit a recognized non-structural filter or raster structural effect | Raster isolation group | Error |
| Complex text | Split only if equivalent; else raster | Prefer editable split/metric approximation | Raster text island | Error |

An “exact native” result may still differ by the global visual tolerance caused by renderer anti-aliasing; it uses no known semantic approximation.

## 4. Paint-island construction

The planner starts a new atomic island at any group requiring offscreen compositing:

- effective filter;
- mask or nontrivial clip path;
- non-normal blend mode or isolation;
- group opacity when children overlap or when distributing opacity changes compositing;
- unsupported paint server spanning multiple children;
- an image/nested SVG boundary that cannot be recursively represented.

It may prove a group-opacity flatten safe only when painted child regions do not overlap and no filter/blend/mask/clip observes group output. Otherwise the group remains atomic.

Raster fallback selects the smallest ancestor that contains every input needed to render the effect exactly. It MUST NOT rasterize an entire document merely because one descendant is unsupported.

The sole v1 filter decomposition is a recognized source-preserving drop shadow: one [`feDropShadow`](https://www.w3.org/TR/filter-effects-1/#feDropShadowElement) (or its exact normalized equivalent), normal compositing/color behavior, unchanged SourceGraphic, solid flood color, and no dependent filter inputs. Balanced mode may keep SourceGraphic native and omit only the shadow when resolved flood opacity is at most `0.15`, standard deviation is at most `4 px` on each axis, and absolute offset is at most `4 px`; editable mode uses separately validated options with defaults `0.25`, `8 px`, and `8 px`. The omission emits `filter-omitted` with resolved parameters. If recognition or any bound fails, the complete filter island rasterizes. Fidelity always rasterizes it; strict always reports a violation. No arbitrary filter graph is partially evaluated or labeled cosmetic.

## 5. Native geometry predicates

### 5.1 Common style

Native geometry requires:

- finite bounded geometry and stroke width;
- a solid sRGB fill and/or stroke;
- fill/stroke/group opacity representable by one target element opacity, or a proven equivalent split into fill-only and stroke-only elements;
- the resolved SVG `paint-order` representable directly or by coincident fill-only/stroke-only elements emitted in that exact order;
- `roughness: 0` and `fillStyle: "solid"`;
- the transformed stroke envelope representable by one scalar target stroke width; a painted non-uniform/skew transform or `non-scaling-stroke` is non-native unless outline comparison proves equivalence within tolerance;
- stroke cap/join/miter differences below tolerance, or an approximation diagnostic;
- dash classified as exact-enough `solid`, `dashed`, or `dotted`;
- no unsupported compositing ancestor.

When independent fill and stroke alpha or SVG `paint-order` cannot share one target element, the planner may emit two coincident elements in the resolved fill/stroke order if their compositing remains equivalent and the output budget permits. Marker ordering is considered separately. Otherwise it falls back.

### 5.2 Rectangles

An SVG/source rectangle maps to target `rectangle` when correlation is exact/unique and its transform decomposes to translation, positive scale, and rotation without skew. Width/height are normalized positive; angle is radians.

Numeric `rx`/`ry` maps to Excalidraw adaptive roundness only when the rendered target radius is within `max_native_radius_error_px` on every corner. Unequal radii, elliptical corners, independently rounded corners, and one-sided rounded header shapes are not exactly native. Balanced/editable may approximate; fidelity/strict falls back/errors.

### 5.3 Circles and ellipses

Circle/ellipse maps to `ellipse` when the transformed axes remain orthogonal and the shape can be expressed by width, height, and rotation. Skewed ellipses fall back. Reflections of symmetric ellipses are normalized when exactly equivalent.

### 5.4 Lines and polygons

Line-only paths, `<line>`, `<polyline>`, and `<polygon>` map to target `line` with:

- element `x/y` equal to the first absolute point;
- first local point exactly `[0, 0]`;
- subsequent points relative to `x/y`;
- element `angle: 0`, because the full affine transform is already baked into those points;
- `roundness: null` for exact straight segments;
- `polygon: true` only for a valid closed polygon with representable fill; target points explicitly repeat `[0, 0]` as the final point and therefore contain at least four entries, matching the pinned target's polygon invariant;
- width/height derived from the local point bounds according to target normalization rules.

Degenerate duplicates are removed without changing topology. Fewer than two distinct points is nonpainting; a filled degenerate polygon follows SVG paint semantics and otherwise falls back.

### 5.5 Diamonds

SVG has no diamond primitive. A closed four-segment path may be promoted to target `diamond` only when its vertices match the target diamond geometry after affine normalization and correlation is unambiguous. Otherwise it remains a polygon. Visual geometry, not a class name or source text, drives recognition.

## 6. Path lowering

### 6.1 Exact line extraction

Normalized `MoveTo`, `LineTo`, and `Close` segments are split into subpaths. A single simple subpath follows the line/polygon rules. Multiple open subpaths become separate grouped line elements. Multiple filled subpaths require fill-rule analysis and do not become one target polygon.

### 6.2 Adaptive curve flattening

Quadratic/cubic segments are recursively subdivided after the full absolute transform. Acceptance uses a conservative control-polygon/convex-hull upper bound on curve-to-chord distance, not point sampling; a chord is emitted only when that bound is at most `curve_tolerance_px`. The algorithm:

1. checks recursion/segment/point budgets before subdivision;
2. evaluates in `f64` with finite checked arithmetic;
3. retains cusp, extrema, and subpath boundary points;
4. retains every accepted subdivision endpoint in v1 instead of applying a second, harder-to-prove simplifier;
5. uses independent dense/adaptive sampling only as a test oracle, never as the production proof of the error bound.

The default balanced tolerance is 0.5 px at 1× output scale; editable is 0.75 px; fidelity does not flatten unsupported curves. Tolerance may be configured only within the bounded range in the security/performance spec.

Open flattened paths become `line`, never `freedraw`, because target freedraw applies pen-stroke semantics. Closed single subpaths become polygons only when non-self-intersecting and fill semantics are preserved. Exceeding depth, work, or point budgets selects fallback/error according to profile; it never loosens tolerance dynamically.

SVG implicitly closes an open subpath for filling but not for stroking. Therefore an open fill-only subpath may become a closed fill polygon, while an open fill-and-stroke subpath requires two coincident outputs—a closed fill-only polygon and an open stroke-only line—in the resolved SVG paint order. It MUST NOT become one stroked target polygon, which would invent a closing stroke. The split is allowed only when topology, opacity, compositing, and budgets preserve equivalence; otherwise the island falls back.

### 6.3 Compound paths and holes

Excalidraw has no compound path/fill-rule primitive. Balanced and fidelity rasterize the smallest island. Editable may tessellate a solid-color fill into transparent-stroke polygons only when:

- tessellation is deterministic and respects nonzero/evenodd fill;
- the triangle/polygon count fits `max_decomposition_elements`;
- seams remain within visual thresholds at tested zooms;
- the stroke is emitted separately from the original flattened boundary;
- a `compound-path-decomposed` diagnostic is emitted.

If any condition fails, raster fallback is mandatory.

## 7. Markers and arrows

A connector is promoted to target `arrow` only when:

- the source connector correlates exactly/uniquely to its base path and generated marker nodes;
- its centerline is an open line/polyline or flattened curve within profile budget;
- each effective start/end marker matches a supported target arrowhead by normalized geometry and paint;
- the source marker orientation and side are preserved;
- marker artwork has no independent effect/paint that target arrowheads cannot express.

Supported mappings include arrow/triangle, bar, circle/outline, diamond/outline, and cardinality forms when the normalized marker matches. Unrecognized markers remain grouped geometry or cause island fallback per profile.

Excalidraw has no midpoint-marker field. Any effective `marker-mid`, or a marker whose repeat/orientation/context-paint behavior cannot be represented by one target endpoint arrowhead, is preserved as explicit grouped geometry when budgets allow or by fallback; it is never silently collapsed to a start/end head.

Every emitted arrow explicitly sets `startArrowhead` and `endArrowhead`, including `null`; omitted fields are forbidden because upstream restoration assigns a legacy default end arrowhead. Markerless geometry emits `line`, not `arrow`.

Arrow `startBinding`/`endBinding` are always null in v1. SVG defines no standard graph-topology contract, and v1 has no editor-specific metadata profile. A future binding profile requires its own source schema, reciprocal-reference invariants, and decision entry; proximity alone will remain insufficient.

For `rfc.svg`, the 12 marker-bearing connectors keep their centerlines as
native lines and their noncanonical marker artwork as explicit grouped
geometry. None receive node bindings.

## 8. Text mapping

A source text chunk is natively compatible only when:

- writing mode is horizontal and text flow is linear;
- the emitted run has one target-compatible font family, size, solid color, and line height;
- every scalar is covered by the pinned bundled face selected for that run; native mapping never relies on an operating-system fallback font;
- there are no per-glyph rotations/offsets, textPath, unsupported decoration, or spacing that changes placement beyond tolerance;
- target measurement under the selected font can reproduce anchor/baseline placement within `max_text_anchor_error_px`;
- transform has no skew/reflection and rotation is representable.

Compatible multi-span or multi-script text may split into separately positioned target text elements when each run has a pinned target face and the concatenated visual layout stays within tolerance. Split runs share an explicit group ID. Unsupported glyph coverage, emoji/color-font dependence, or environment-only fallback makes the text non-native; it rasterizes in fidelity/balanced or uses a diagnosed deterministic replacement in editable.

Output fields are derived as follows:

- `text` and `originalText`: decoded author text;
- `fontSize`: resolved CSS px size adjusted for baked scale;
- `fontFamily`: selected target font ID;
- `textAlign`: SVG anchor start/middle/end mapped to left/center/right;
- `verticalAlign`: `top` for unbound text;
- `x/y/width/height`: target-font measured box positioned from resolved SVG glyph bounds, anchor, baseline, and rotation;
- `containerId`: null in v1 because SVG has no standard bound-text semantics;
- `autoResize`: true for unwrapped source text;
- `lineHeight`: configured target font metric.

Font weight/style unsupported by target produces `font-style-approximated`; it is never encoded by choosing a misleading font ID. `rfc.svg` uses single-line text, so all 131 strings remain native in balanced/editable mode with a deterministic Inter-to-Liberation-Sans substitution and position correction.

## 9. Images and raster fallback

Existing supported raster `<image>` content becomes an Excalidraw image/file pair when its placement is representable. Pixels are bounded-decoded and canonical-PNG encoded per the emission spec; this freezes animated formats to the diagnosed first-frame static snapshot and removes metadata/color-profile ambiguity. Preserve-aspect-ratio cropping is baked into pixels when target crop cannot reproduce the exact result.

Nested SVG recursively re-enters ingestion/planning within shared depth/work budgets. Its children expand into native target elements only when the replaced-element viewport transform, clipping, opacity, and paint-order insertion are exactly representable; the composed transform is baked into every child and all children occupy the original image's single z-order interval. Otherwise the complete nested image viewport becomes one raster island. A nested child never escapes the outer `<image>` viewport clip.

Fallback rendering:

1. obtains absolute layer bounds from the normalized node;
2. expands to integer pixels with a deterministic anti-alias/filter margin;
3. selects scale subject to `fallback_scale` and aggregate pixel budget;
4. renders exactly that node with `resvg::render_node` on a transparent sRGB pixmap;
5. deterministically PNG-encodes it;
6. emits one target image with scale `[1, 1]`, `crop: null`, and a content-addressed file ID.

Transparent padding is included in image geometry so filtered pixels align with the original scene. Multiple source keys and the fallback reason are attached as compact provenance.

## 10. Groups, z-order, and semantic inference

- Target array order follows normalized SVG paint order.
- Explicit source `<g>`/`<use>` instance nesting maps to deepest-to-shallowest `groupIds` only when at least two emitted elements share the group.
- Rendering-only normalized groups do not become user groups unless they correspond to an explicit source group.
- Regrouping MUST NOT move elements across intervening paint order.
- Ordinary groups/lane rectangles never become frames.
- Card/lane/icon-label composition inference is outside the correctness path and not part of v1.

## 11. Diagnostics

Required stable codes include:

- `path-flattened`
- `compound-path-decomposed`
- `paint-island-rasterized`
- `filter-omitted`
- `gradient-rasterized`
- `mask-rasterized`
- `clip-rasterized`
- `font-substituted`
- `font-style-approximated`
- `dash-pattern-approximated`
- `corner-radius-approximated`
- `marker-preserved-as-geometry`
- `binding-not-inferred`
- `ambiguous-source-correlation`
- `animated-image-snapshot`

## 12. Relevant AGENTS.md sections

- Type Design & API, Safety & Security, Error Handling: binding.
- Performance: borrowing/iteration/preallocation and profile-before-optimization are binding; the algorithmic budgets are normative.
- Async & Concurrency: N/A in core; mapping is deterministic single-operation CPU work.
- Serialization: N/A until emission; plan types remain strongly typed.
- Testing/Documentation: binding; flattening, fill topology, marker recognition, and text placement require property and fixture tests.

## 13. Cross-references

- ← [SVG ingestion](./svg-ingestion-design.md)
- → [Excalidraw emission](./excalidraw-emission-design.md)
- ↔ [Verification plan](./svg-to-excalidraw-verification-plan.md)
- ↔ [Security and performance](./svg-to-excalidraw-security-performance.md)
