# PRD — SVG to editable Excalidraw

Status: ready for implementation v1 · Owner: svg2excal maintainers · Last updated: 2026-08-04

## 1. Problem

SVG describes a painted 2D document; Excalidraw stores a smaller scene of editable whiteboard primitives. A simple tag-to-tag converter fails on CSS cascade, inherited style, units, nested transforms, `<defs>/<use>`, markers, text shaping, arbitrary paths, clipping/masking, filters, and source paint order. A visually perfect solution that embeds the entire SVG as one image is not usefully editable, while a native-only solution silently loses unsupported appearance.

The product must make that trade-off explicit and local: preserve editability wherever the target can represent the source, approximate only within a declared budget, and rasterize only the smallest unsupported paint island.

## 2. Vision

A user supplies an untrusted static SVG and receives:

1. an importable `.excalidraw` v2 document;
2. editable native rectangles, ellipses, text, lines, arrows, polygons, groups, and images wherever representable;
3. minimal embedded image fallbacks for unsupported effects/geometry;
4. a deterministic, machine-readable report of every approximation, fallback, omission, and security rejection.

For `fixtures/rfc.svg`, all titles and labels remain editable text, cards and
lanes remain editable shapes, routed flows remain editable line elements,
marker artwork remains explicit geometry, icons remain grouped editable vector
approximations where budgets allow, z-order is preserved, and the
shadow/font/dash/radius limitations are reported.

## 3. Goals

| # | Goal | Measure |
| --- | --- | --- |
| G1 | Produce valid current Excalidraw JSON | Every accepted fixture loads through the pinned Excalidraw restore path with no dropped generated element or dangling reference |
| G2 | Preserve editability | On `rfc.svg`, 100% of its 131 text strings and all representable rectangles/straight or orthogonal connectors are native elements |
| G3 | Preserve appearance honestly | Balanced-profile render differs from the reference by at most the thresholds in the verification plan; every threshold-exempt region has a diagnostic |
| G4 | Be generic | The corpus covers every supported SVG element/style/effect class in the feature matrix, including constructs absent from `rfc.svg` |
| G5 | Be deterministic | Identical bytes, options, explicit resource/font bytes, converter version, and target profile produce byte-identical JSON on every supported CI platform, including IDs, nonzero seeds, ordering, diagnostics, and embedded assets |
| G6 | Be safe on hostile input | All byte/depth/count/decoded-asset/output/work limits fail with typed errors and no external I/O by default |
| G7 | Be embeddable | Core conversion is synchronous and deterministic; the default path is I/O-free, external bytes require an explicit bounded resource provider, and custom fonts are caller-supplied bounded bytes |
| G8 | Explain loss | Every non-exact source construct is traceable by stable code, severity, source location/key, affected target IDs, and mitigation |

## 4. Non-goals

- Reconstructing author intent not encoded in SVG, such as “this rectangle plus sibling text is a card.”
- Emitting arrow bindings, frames, text containers, or inferred graph topology in v1.
- Supporting script, event handlers, animation, SMIL, live CSS, or browser DOM behavior.
- Guaranteeing editable representation of arbitrary Bézier paths, compound fills, filters, masks, gradients, or complex text.
- Round-tripping Excalidraw edits back into the original SVG.
- Preserving source XML formatting, comments, entity spelling, or editor-specific metadata unless retained as bounded provenance.
- Making the placeholder server the only product interface. The core library contract comes first; CLI/service adapters are consumers.
- Styling imports with Excalidraw's hand-drawn look by default. Faithful imports use `roughness: 0`; stylization is an explicit future profile.

## 5. Users

### Primary

- Engineers and technical writers importing architecture, sequence, flow, and infrastructure diagrams for further editing.
- Tool authors embedding SVG-to-Excalidraw conversion in documentation or diagram pipelines.

### Secondary

- Designers importing icon sheets or simple vector art, accepting localized raster fallbacks.
- Services batch-converting bounded user uploads.

### Anti-personas

- Users needing a browser-complete SVG engine with animation or interactivity.
- Users requiring lossless editable conversion of arbitrary illustration artwork.
- Systems expecting network access or unrestricted local-file resolution from SVG references.

## 6. Success metrics

- `rfc.svg` meets all fixture-specific structural assertions and calibrated visual budgets.
- At least 95% of elements across the maintained “diagram-like SVG” corpus are native or bounded-vector approximations in balanced mode; fallback percentage is reported by painted area and element count.
- Strict mode either produces an all-exact scene or returns `StrictFidelityViolation` with the complete ordered set of blocking diagnostics.
- Corpus fuzzing and hostile-limit tests have zero panic, hang, uncontrolled allocation, external fetch, or filesystem escape.
- Median conversion time for a 1 MiB / 5,000-node diagram is at most 250 ms and p99 at most 2 s on the documented CI reference machine; fallback raster work observes the separate pixel budget.

## 7. Binding product behavior

### Profiles

- `balanced` is the default: native when exact, bounded approximation when visually safe, otherwise minimal raster fallback.
- `editable` prefers bounded vector decomposition/flattening and may omit non-structural effects; every loss is reported.
- `fidelity` rasterizes an unsupported isolation boundary rather than approximate it.
- `strict` returns no document if any effective painted construct cannot be represented exactly under the strict feature matrix.

Profiles choose policy; hard security/resource limits are never relaxed by profile.

### Output contract

Successful conversion returns a typed document plus report. Warnings do not make success ambiguous. Fatal parse, security, limit, normalization, or emission errors return a typed `ConversionError` and no partial document.

## 8. Naming conventions

- Public Rust crate: `svg2excal-core`.
- Primary operation: `convert`.
- Source-side keys/types use `Svg` or `Source`; target-side types use `Excalidraw`; shared normalized types use `Scene`.
- Diagnostics use stable `kebab-case` codes such as `font-substituted`, `path-flattened`, `paint-island-rasterized`, and `binding-not-inferred`.
- JSON follows Excalidraw's existing camelCase field names via `serde(rename_all = "camelCase")`.
- Custom target metadata is namespaced under `customData.svg2excal`.

## 9. Cross-references

- → [Conversion scene model](./conversion-scene-model-design.md)
- → [Roadmap](./svg-to-excalidraw-roadmap.md)
- ↔ [Research study](../docs/research/study-svg-to-excalidraw.md)
