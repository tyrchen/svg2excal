# Study: SVG to editable Excalidraw conversion

Status: Done · Owner: svg2excal maintainers · Date: 2026-08-04

Vendor pins:

| Repository and pin | Role | License |
| --- | --- | --- |
| `vendors/excalidraw` @ `ab0255f21eb40b5408f3e9ed9725474108eda9e6` (`v0.18.0-370-gab0255f2`) | Target schema, restore, and render compatibility oracle; not a Rust runtime dependency | [MIT](../../vendors/excalidraw/LICENSE) |
| `vendors/resvg` @ `08c79a3148df4ce8ab08fca72204b142b95423dd` (`usvg`/`resvg` 0.48.1) | Normalization and minimal raster-fallback engine | [MIT](../../vendors/resvg/LICENSE-MIT) or [Apache-2.0](../../vendors/resvg/LICENSE-APACHE) |
| `vendors/svg-to-excalidraw` @ `6f6e4b7269c4194b56cf7517a8357ba73be12a3a` | Historical official prototype and negative-prior-art reference; not reused as a runtime | [MIT](../../vendors/svg-to-excalidraw/LICENSE) |

Operationally, the Excalidraw and resvg pins were current active upstream commits on 2026-08-04; the official prototype's pin is from 2021 and its copied schema is obsolete. This supports using resvg as a maintained Rust dependency and Excalidraw as a pinned compatibility oracle, while treating the prototype only as design evidence.

## Why this study

This study answers one question: **what architecture can turn static SVG into an editable, visually faithful, deterministic Excalidraw scene without making correctness depend on diagram-specific heuristics?** The answer must cover arbitrary SVG constructs, define honest fallbacks for target-format gaps, and work well on [`fixtures/arch.svg`](../../fixtures/arch.svg).

The research used the [W3C SVG 2 specification](https://www.w3.org/TR/SVG/), the [current Excalidraw source and documentation](https://github.com/excalidraw/excalidraw), and the [current usvg API documentation](https://docs.rs/usvg/latest/usvg/), inspected on 2026-08-04. SVG is a retained painting format with shapes, text, images, groups, styling, transforms, references, clipping, masking, and filters. Excalidraw is a scene of a much smaller set of editable primitives. A total, lossless, editable mapping therefore does not exist; the converter needs a principled loss policy rather than a list of ad hoc tag conversions.

## Architecture map

```text
Untrusted SVG bytes
       │
       ▼
┌────────────────────────────────────────────────────────────────────┐
│ Bounded input boundary                                             │
│ - reject DTD/entities and active content                           │
│ - cap bytes/decompression/nodes/depth/references/assets            │
│ - deny external resources unless an explicit sandbox is configured│
└──────────────────────────────┬─────────────────────────────────────┘
                               │ one validated document
             ┌─────────────────┴──────────────────┐
             ▼                                    ▼
┌──────────────────────────┐        ┌───────────────────────────────┐
│ Source semantic tree     │        │ usvg normalized paint tree   │
│ - original tags/IDs      │        │ - computed style/inheritance │
│ - explicit groups        │        │ - resolved units/references  │
│ - defs/use/marker intent │        │ - transforms/bounds/text     │
│ - byte-range provenance  │        │ - compositing/effect islands │
└─────────────┬────────────┘        └──────────────┬────────────────┘
              └──────────────────┬─────────────────┘
                                 ▼
                    ┌─────────────────────────┐
                    │ Correlated scene IR     │
                    │ - stable source keys    │
                    │ - z-order/paint islands │
                    │ - resolved geometry     │
                    │ - explicit loss facts   │
                    └────────────┬────────────┘
                                 ▼
                    ┌─────────────────────────┐
                    │ Lowering planner        │
                    │ native / approximate /  │
                    │ minimal raster fallback │
                    └───────┬────────┬────────┘
                            │        │
                ┌───────────▼──┐  ┌──▼────────────────────┐
                │ Typed scene  │  │ Conversion report     │
                │ + file map   │  │ reason/source/impact  │
                └───────┬──────┘  └───────────────────────┘
                        ▼
                Deterministic `.excalidraw` JSON
```

The central conclusion is the dual representation. The source tree preserves authoring semantics needed for editability; the normalized tree is the authority for what actually paints. Neither is sufficient alone.

## SVG: the source-side problem

SVG basic shapes, paths, images, and text can be grouped, styled, transformed, referenced, and composited. The visual result depends on the CSS cascade, inherited properties, viewport/unit resolution, accumulated affine transforms, paint order, and reference expansion. Paths may contain lines, quadratic/cubic Béziers, elliptical arcs, multiple subpaths, and either fill rule.

`usvg` exists specifically to resolve this complexity. Its crate contract applies CSS, resolves inherited/default attributes, converts relative units, expands `<use>` and markers, resolves text, and normalizes all basic shapes to absolute path segments ([`usvg/src/lib.rs:4-37`](../../vendors/resvg/crates/usvg/src/lib.rs#L4-L37)). Its public render tree contains only `Group`, `Path`, `Image`, and `Text`, and exposes resolved absolute transforms and bounds ([`tree/mod.rs:903-991`](../../vendors/resvg/crates/usvg/src/tree/mod.rs#L903-L991)). That tree is excellent visual truth.

The same normalization deliberately destroys authoring distinctions. Rectangle, circle, ellipse, line, polygon, polyline, and path all enter one path conversion ([`converter.rs:663-681`](../../vendors/resvg/crates/usvg/src/parser/converter.rs#L663-L681)); `<use>` and markers become expanded content; groups that do not affect rendering can disappear. Consequently:

- A raw XML-only converter will get CSS, units, transforms, text, and references wrong.
- A `usvg`-only converter cannot reliably know whether a normalized path was authored as a rectangle, connector, icon subpath, marker, or arbitrary path.
- IDs cannot be the sole correlation key: visible elements often have no ID, IDs can be duplicated in hostile input, and generated marker/use nodes intentionally lose IDs.

The source tree therefore assigns every element a stable `SourceKey` from document order and byte range. Correlation uses a paired traversal plus tag/geometry/style fingerprints, records one-to-many expansions, and refuses semantic promotion when the match is ambiguous. Ambiguity lowers from “native” to “approximation” or “fallback”; it never guesses silently.

### Security caveats in the chosen parser

`usvg` is a renderer-oriented library, not the product security boundary:

- Its XML parser enables DTD parsing ([`parser/mod.rs:158-175`](../../vendors/resvg/crates/usvg/src/parser/mod.rs#L158-L175)).
- SVGZ decompression uses `read_to_end` without a product-level output cap ([`parser/mod.rs:178-188`](../../vendors/resvg/crates/usvg/src/parser/mod.rs#L178-L188)).
- Its default image resolver can read local paths ([`parser/image.rs:78-113`](../../vendors/resvg/crates/usvg/src/parser/image.rs#L78-L113)).
- Its built-in structural limit is far larger than a useful service limit.

The application must preflight bytes, reject DTD/entities, bounded-decompress SVGZ before parsing, pre-resolve assets into an immutable bounded store, install lookup-only image resolvers, cap data-URL lexical size before normalization, cap decoded assets and nested SVG, and enforce aggregate work budgets. The lexical cap is essential because usvg decodes a data URL before invoking the custom data resolver; the resolver alone cannot bound that allocation. The default nested-SVG resolver is also forbidden because it re-enters `from_data_nested` ([`parser/mod.rs:126-154`](../../vendors/resvg/crates/usvg/src/parser/mod.rs#L126-L154)); opt-in nested SVG must recurse through the same bounded DTD-disabled `from_xmltree` pipeline before parent normalization.

## Excalidraw: the target-side contract

The current file envelope is `{ type, version, source, elements, appState, files }` ([`data/types.ts:14-21`](../../vendors/excalidraw/packages/excalidraw/data/types.ts#L14-L21)). The current envelope type and version are `"excalidraw"` and `2` ([`common/constants.ts:285-290`](../../vendors/excalidraw/packages/common/src/constants.ts#L285-L290), [`common/constants.ts:352-355`](../../vendors/excalidraw/packages/common/src/constants.ts#L352-L355)). Serialization preserves element order and filters the binary file map to referenced images ([`data/json.ts:35-75`](../../vendors/excalidraw/packages/excalidraw/data/json.ts#L35-L75)).

Every element shares position, dimensions, angle, stroke/fill styling, opacity, roundness, a rough-render seed, version fields, ordering index, group membership, frame/binding state, timestamps, links, and optional custom data ([`element/types.ts:40-82`](../../vendors/excalidraw/packages/element/src/types.ts#L40-L82)). The element union provides rectangles, diamonds, ellipses, text, line/arrow, freedraw, image, frame, and embed-like types—but no arbitrary SVG/Bezier path ([`element/types.ts:177-216`](../../vendors/excalidraw/packages/element/src/types.ts#L177-L216)).

That absence defines the fallback boundary. Excalidraw line/arrow elements store local points and finite arrowhead/binding fields ([`element/types.ts:306-359`](../../vendors/excalidraw/packages/element/src/types.ts#L306-L359)). Unrounded lines render as polylines/polygons; rounded ones render as an interpolating RoughJS curve, not the original Bézier ([`element/shape.ts:872-918`](../../vendors/excalidraw/packages/element/src/shape.ts#L872-L918)). Freedraw likewise reinterprets point samples as a pen stroke ([`element/shape.ts:958-980`](../../vendors/excalidraw/packages/element/src/shape.ts#L958-L980)); it is not a lossless general path carrier.

Text is also intentionally constrained: one font family ID, size, string, alignment, container, auto-resize, and line height ([`element/types.ts:235-257`](../../vendors/excalidraw/packages/element/src/types.ts#L235-L257)). SVG mixed spans, arbitrary font faces/weights/styles, letter spacing, text paths, per-glyph offsets, and decorations have no native representation. Excalidraw's available font IDs are fixed ([`common/constants.ts:130-141`](../../vendors/excalidraw/packages/common/src/constants.ts#L130-L141)).

Images are the fidelity escape hatch. An image element refers to a `fileId` ([`element/types.ts:146-156`](../../vendors/excalidraw/packages/element/src/types.ts#L146-L156)); the file map stores MIME type, data URL, and metadata ([`excalidraw/types.ts:115-145`](../../vendors/excalidraw/packages/excalidraw/types.ts#L115-L145)). `resvg::render_node` can rasterize exactly one normalized node using its layer bounds ([`resvg/src/lib.rs:45-69`](../../vendors/resvg/crates/resvg/src/lib.rs#L45-L69)), enabling minimal paint-island fallback instead of flattening the entire document.

## Prior converter: useful warning, not a base

The official `svg-to-excalidraw` prototype demonstrates direct primitive conversion but is not a generic foundation. It accepts only a short tag list and omits text and line handling ([`walker.ts:29-47`](../../vendors/svg-to-excalidraw/src/walker.ts#L29-L47), [`walker.ts:239-242`](../../vendors/svg-to-excalidraw/src/walker.ts#L239-L242)). It reads presentation attributes directly rather than computing CSS ([`attributes.ts:21-95`](../../vendors/svg-to-excalidraw/src/attributes.ts#L21-L95)), expands `<use>` manually ([`walker.ts:73-171`](../../vendors/svg-to-excalidraw/src/walker.ts#L73-L171)), approximates paths by sampled drawing elements, and fakes holes with white fills ([`walker.ts:362-447`](../../vendors/svg-to-excalidraw/src/walker.ts#L362-L447)). It also emits an obsolete element schema and random identifiers ([`ExcalidrawElement.ts:13-92`](../../vendors/svg-to-excalidraw/src/elements/ExcalidrawElement.ts#L13-L92)).

Patterns to keep are transform accumulation, local-point normalization, and explicit group IDs. Patterns to avoid are DOM-attribute merging as computed style, white-as-transparency, random output, obsolete copied target types, and unsupported-tag omission without diagnostics.

## `arch.svg` case study

The fixture is a 2,180 × 1,420 static architecture diagram with 317 XML elements. It contains 55 rectangles, 86 text nodes, 24 lines, 46 paths, 26 `<use>` instances, 53 groups, 16 circles, one ellipse, five markers, and one drop-shadow filter. Definitions include 24 reusable icons and five arrowhead markers ([`arch.svg:4-108`](../../fixtures/arch.svg#L4-L108)); a 28-class stylesheet carries most colors, type roles, stroke widths, anchoring, and dash styles ([`arch.svg:110-131`](../../fixtures/arch.svg#L110-L131)).

The fixture proves five load-bearing points:

1. **Computed style is mandatory.** `.card { fill:#FFFFFF }` is a class rule and outranks presentation attributes on two card rectangles. Direct-attribute precedence would render the wrong fill ([`arch.svg:115`](../../fixtures/arch.svg#L115), [`arch.svg:268`](../../fixtures/arch.svg#L268), [`arch.svg:364`](../../fixtures/arch.svg#L364)).
2. **Reference and transform resolution are mandatory.** Icons inherit style through definition groups and `<use>` instances; one use is additionally translated and scaled ([`arch.svg:25-108`](../../fixtures/arch.svg#L25-L108), [`arch.svg:163`](../../fixtures/arch.svg#L163)).
3. **Document groups are not diagram semantics.** Most cards consist of a filtered rectangle followed by sibling icon/text/header elements. Mechanical `<g>` mirroring will not group what users perceive as a card.
4. **Connector promotion is valuable but must be conservative.** Twenty-seven objects carry markers; visible routes include straight, bidirectional, dashed, fan-out, and orthogonal multi-point connectors ([`arch.svg:214-228`](../../fixtures/arch.svg#L214-L228), [`arch.svg:306-333`](../../fixtures/arch.svg#L306-L333)). They map naturally to arrows, but SVG does not encode node binding.
5. **Fidelity gaps are real but localized.** Twenty-four filter uses create shadows; exact dash arrays, numeric corner radii, font weights, Inter metrics, one-sided rounded header paths, and curved icon paths cannot all be expressed natively.

For this fixture the balanced profile should preserve all 86 text strings as editable text, emit lane/card rectangles and connector lines/arrows natively, expand and group icon instances, flatten only curved icon/header paths within tolerance, omit a provably cosmetic shadow within threshold or rasterize its isolation group, diagnose that choice, and preserve source paint order. It must not infer frames, card groups, or arrow bindings from proximity.

## Mapping algorithm

### 1. Partition into paint islands

A paint island is the smallest normalized node/subtree whose compositing can be evaluated independently. An effective filter, mask, clip, non-normal blend, isolation, group opacity with overlapping children, unsupported paint server, or nonrepresentable transform makes the enclosing isolation group atomic. Raster fallback operates on this boundary, preventing seams and double application of effects.

### 2. Classify with a strict native predicate

Native conversion is permitted only when geometry, transform, paint, and grouping are representable:

| Resolved/source construct | Native target | Native predicate |
| --- | --- | --- |
| `rect` | rectangle | solid colors; no effect; translation/scale/rotation only; roundness within target error budget |
| `circle`, `ellipse` | ellipse | solid colors; no effect; no skew; ellipse axes representable after transform |
| `line`, line-only open path | line | finite points; representable stroke; no unsupported markers |
| line-only closed path/polygon | line with `polygon: true` | one non-self-intersecting subpath; representable fill rule |
| connector + recognized markers | arrow | exact correspondence; recognized start/end heads; connector semantics above confidence threshold |
| simple text/chunk | text | horizontal linear flow; uniform representable span; stable fallback font; no per-glyph transforms |
| supported raster image | image | decoded within budget; affine placement representable; no unsupported enclosing effect |
| explicit source `<g>` | `groupIds` | at least two emitted children; paint order unchanged |

SVG groups never become Excalidraw frames by default. Frames have their own visible and clipping semantics; group membership is instead repeated as deepest-to-shallowest `groupIds` ([`element/types.ts:71-74`](../../vendors/excalidraw/packages/element/src/types.ts#L71-L74)).

### 3. Approximate under an explicit error budget

Open curves are adaptively flattened to polyline points using a conservative subdivision bound. Closed, single-subpath curves may become a polygon only if fill semantics remain valid. Tolerance is measured in output pixels after the full transform, not in source coordinates. V1 retains accepted subdivision endpoints rather than adding a sampling-based simplifier. Point and output-element budgets are hard limits.

Exact dash arrays map to `solid`, `dashed`, or `dotted` by a documented classifier; numeric radius maps to Excalidraw roundness only when the rendered-radius error is within tolerance. Colors are converted to sRGB and separate fill/stroke alpha is combined only when the result remains equivalent; otherwise the island falls back. Every approximation adds a stable diagnostic code.

### 4. Rasterize the smallest unsupported isolation boundary

For gradients, patterns, compound fills with holes, complex text, filters, masks, clips, blend modes, or unbounded path complexity, render the minimal normalized node with `resvg`, encode a PNG in the Excalidraw file map, and place one image element at the node's absolute layer bounds. Adjacent fallback islands may be coalesced only when it reduces output and does not absorb an otherwise editable text or primitive.

### 5. Emit and validate deterministically

The emitter derives IDs, nonzero render seeds, version nonces, group IDs, and file IDs from a domain-separated hash of the source digest, stable source key, occurrence, and emitted role. Excalidraw passes the element seed directly into RoughJS ([`shape.ts:194-204`](../../vendors/excalidraw/packages/element/src/shape.ts#L194-L204)), and RoughJS seed zero selects nondeterministic `Math.random()` ([`math.ts:8-14`](https://github.com/rough-stuff/rough/blob/56a2762171b1294d643501e8d14f120db6b27bd7/src/math.ts#L8-L14)); render seeds therefore exclude zero. The emitter uses `roughness: 0`, `fillStyle: "solid"`, `version: 1`, `isDeleted: false`, null bindings/containers/frames in v1, and stable timestamps. Array order follows SVG paint order. A final validator checks finite/bounded geometry, local-point invariants, unique IDs, null relationship fields, valid group/file references, and allowed enum values before JSON serialization.

## What we will adopt

- `usvg`/`resvg` 0.48.1 as the standards-resolution and minimal-fallback engine, behind our own bounded input/resource layer.
- A separate source tree with byte-range provenance and stable traversal keys.
- Native-first lowering with strict predicates, bounded approximation, and minimal raster fallback.
- Typed Rust target models mirroring the pinned Excalidraw v2 contract rather than copied dynamic JSON.
- Deterministic output and a structured report whose diagnostics are part of the public contract.
- Source `<g>` to `groupIds`; null bindings/containers in v1; no diagram-specific card/lane inference in the correctness path.
- Differential rendering against `resvg` plus structural assertions for editability.

## What we will avoid

- Whole-document rasterization as the default.
- Treating presentation attributes as final computed style.
- Using `usvg` node IDs as the sole source-correlation mechanism.
- Treating freedraw as a lossless SVG path format.
- Inventing frames, card groups, or arrow bindings from proximity.
- Silent omission of unsupported SVG features.
- Default filesystem/network access from SVG resource references.
- Random IDs, timestamps, seeds, ordering, or serialization.

## Open questions

None block specification or Phase 1. Font substitution thresholds, curve tolerances, fallback scale, and confidence thresholds are specified as validated options with conservative defaults and are calibrated by the verification corpus rather than left as design gaps.
