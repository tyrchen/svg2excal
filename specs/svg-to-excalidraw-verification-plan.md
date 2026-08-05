# SVG to Excalidraw Verification Plan

Status: ready for implementation v1 · Owner: svg2excal maintainers · Depends on: all component designs

## 1. Purpose

This plan verifies four different properties independently: source interpretation, target structure/editability, visual fidelity, and hostile-input safety. A visual golden alone cannot prove editability; a JSON snapshot alone cannot prove the SVG was interpreted correctly.

## 2. Test layers

```text
                    ┌──────────────────────────────┐
                    │ Upstream compatibility tests│
                    │ load/restore/render current │
                    └──────────────┬───────────────┘
                                   │
              ┌────────────────────▼───────────────────┐
              │ End-to-end fixture + visual differential│
              └────────────────────┬───────────────────┘
                                   │
          ┌────────────────────────▼────────────────────────┐
          │ Feature-matrix integration + hostile corpus     │
          └────────────────────────┬────────────────────────┘
                                   │
       ┌───────────────────────────▼───────────────────────────┐
       │ Unit + property + fuzz tests for types and algorithms │
       └───────────────────────────────────────────────────────┘
```

## 3. Unit tests

Unit tests live beside implementation modules and use `test_should_...` names per `AGENTS.md` § Testing.

Required suites:

- input-size, SVGZ, UTF-8, DTD, root namespace, depth/count/string caps;
- validated newtypes and exact error variants/messages;
- unit/viewBox/preserveAspectRatio and affine composition/decomposition;
- source keys, duplicate IDs, reference graph, cycles, and expansion budgets;
- CSS cascade/inheritance/presentation-attribute precedence;
- color/opacity/dash/radius classification;
- path segment normalization, subpath topology, closure, self-intersection, and fill rules;
- adaptive quadratic/cubic flattening and simplification error bounds;
- marker geometry recognition/orientation/start/end mapping;
- SVG text anchor/baseline to target measured box placement;
- paint-island discovery for opacity/filter/mask/clip/blend;
- deterministic ID/seed/nonce/file hash domain separation;
- render seed is never zero and therefore never activates RoughJS randomness;
- every target element variant and final-validator failure path;
- empty/nonpainting SVG text is omitted rather than emitted as target text that upstream restoration may delete;
- deterministic diagnostic sorting and redacted `Debug` output.

## 4. Property tests

Use `proptest` for invariants:

- affine composition agrees with sequential point transformation within floating tolerance;
- finite accepted inputs never produce non-finite output;
- flattened curves stay within configured output-space tolerance;
- increasing flatten tolerance never increases required sample count for the same curve;
- line normalization always starts at `[0,0]` and preserves absolute points;
- polygon normalization preserves winding/topology where native mapping is allowed;
- deterministic IDs differ across role/occurrence/source-key domains and repeat exactly;
- serialize → deserialize → serialize is byte-identical;
- canonical numeric conversion removes negative zero and is idempotent on coordinate/angle grids;
- source paint order remains target array order through grouping and fallback;
- all output references resolve and deleting any referenced object makes validation fail.

## 5. Feature-matrix fixtures

Root fixtures live under `fixtures/` and are the canonical integration corpus. Small focused SVGs cover:

| Area | Required cases |
| --- | --- |
| Viewport/units | width/height only, viewBox only, meet/slice/none, nested SVG, `%`, em/rem, physical units |
| CSS | element/class/ID selectors, specificity, source order, `!important`, presentation attrs, inline style, inheritance, unsupported at-rules |
| Geometry | every basic shape, relative/absolute path commands, arcs, Béziers, multiple subpaths, holes, self-intersection, degenerate paths |
| Transforms | nested translate/scale/rotate-about-point, negative scale, skew, matrix, stroke scaling, non-scaling stroke |
| References | defs/use/symbol, nested use, duplicate/missing IDs, cycles, markers with both orientations/units |
| Paint | solid/alpha, independent fill/stroke alpha, gradients, patterns, dash arrays/offsets, caps/joins, paint order |
| Text | anchors, tspans, mixed styles, bidi, vertical, per-glyph offsets/rotation, textPath, missing fonts, Unicode fallback |
| Compositing | group opacity with overlap/non-overlap, filters, clip paths, masks, blend/isolation |
| Images | each allowed MIME, preserveAspectRatio, bounded data URL, nested SVG, denied external path/URL |
| Metadata/active | title/desc, foreign namespaces, script/events/animation/foreignObject |

Every fixture has a declarative expectation file containing source feature counts, expected decision classes/codes, native text strings, target element constraints, and visual mask/tolerance. It does not snapshot random or environment-dependent fields because none should exist.

## 6. `arch.svg` acceptance

[`fixtures/arch.svg`](../fixtures/arch.svg) is the M1 end-to-end gate.

### Source/normalization assertions

- root viewport is 2,180 × 1,420 CSS px;
- XML counts match the research census: 55 rect, 86 text, 24 line, 46 path, 26 use, 53 group, 16 circle, 1 ellipse, 5 marker;
- all 28 stylesheet classes participate correctly;
- `.card` class fill wins on the “Ready to Publish” and “Policy Engine” rectangles, so their computed fill is white;
- 26 icon uses and 28 marker instances expand with correct inherited styles/transforms;
- the users icon's 1.7 scale affects geometry and stroke;
- 24 shadow filter applications remain discoverable as isolation boundaries;
- six visible orthogonal connector paths preserve exact turn points.

### Target/editability assertions

- all 86 decoded source strings occur as native target text, with center/start anchoring preserved within the text threshold;
- every representable scene rectangle and straight/orthogonal connector is native, not absorbed by unrelated fallback;
- all 27 marker-bearing connector/legend objects preserve the correct start/end head; the purple “assists” flow is double-ended;
- each expanded icon instance with two or more target elements has its own group ID;
- group membership never changes paint order;
- there are zero frames, frame memberships, inferred container texts, or arrow bindings;
- root white background remains the first painted element;
- target IDs, file IDs, seeds, nonces, array order, report, and JSON bytes repeat exactly across runs;
- the same deterministic corpus produces identical JSON/report/PNG hashes on supported Linux and macOS CI runners;
- final target validation and pinned upstream restore both retain every generated element.

### Expected loss reporting

Balanced mode must report the applicable subset of:

- `font-substituted` / `font-style-approximated` for Inter and unsupported weights;
- `corner-radius-approximated` for radii outside exact target behavior;
- `dash-pattern-approximated` for numeric SVG dash arrays;
- `path-flattened` for curved icon/header geometry kept editable;
- `filter-omitted` for the fixture's recognized `feDropShadow` (`opacity 0.10`, `stdDeviation 3`, `dx 0`, `dy 2`), which is inside balanced defaults; fidelity instead reports `paint-island-rasterized`;
- `binding-not-inferred` once as a summary, not one noisy entry per connector.

There must be no unreported visible loss.

## 7. Visual differential tests

### Reference and candidate

- Reference: render the accepted source SVG with the pinned `resvg` at canonical scales 1× and 2× on transparent and white backgrounds as appropriate.
- Candidate: load generated JSON through pinned Excalidraw restoration and render with its static export path at identical viewport, background, and scale.
- Fonts: use the same versioned bundled target fonts in both candidate measurement and target renderer.
- Runtime: visual goldens run on one pinned Linux image with pinned Node/browser, software rendering, device scale factor, locale, color profile, disabled animation, and a font-ready barrier. Cross-platform gates compare document/report/PNG-fallback bytes, not browser raster pixels.

### Metrics

Images are compared in linearized sRGB after alignment. Gates combine:

- SSIM for overall appearance;
- per-pixel perceptual color delta for fills/text;
- edge-distance transform for geometry alignment;
- alpha-aware masks for fallback/filter margins;
- exact region classification so a diagnosed approximation is not mistaken for an unexplained regression.

Initial thresholds, calibrated in Phase 0 and then frozen:

| Region | Gate |
| --- | --- |
| Exact solid native primitives | edge p99 ≤ 1 px and mean color delta ≤ 1 |
| Native text with exact font | glyph-boundary p99 ≤ 1 px |
| Substituted text | anchor/box error ≤ 1.5 px and no clipping |
| Flattened paths | edge p99 ≤ configured curve tolerance + 0.75 px |
| Raster fallback islands | SSIM ≥ 0.995 and no clipped nontransparent pixel |
| Whole `arch.svg`, balanced | SSIM ≥ 0.98 plus all region-specific gates |

Threshold changes require a key decision with before/after evidence; tests may not be weakened to accept an unexplained diff.

## 8. Upstream compatibility

A small TypeScript harness under a test-tool directory imports the pinned vendored Excalidraw restore/static-render paths. It verifies:

- envelope type/version and current schema fields;
- import/restore does not discard or mutate generated semantic content;
- markerless lines remain markerless and explicit arrowheads remain correct;
- null indices are repaired while preserving array order;
- file-backed image elements load;
- compact `customData.svg2excal` survives restore;
- re-export remains loadable.

Automation is exposed as `make test-compat`; no standalone shell script is introduced. The harness is a test oracle, not a runtime dependency.

## 9. Security and hostile corpus

Integration tests cover one-below/at/one-above every hard limit plus:

- DTD/entity documents;
- gzip bombs and excessive compression ratios;
- deep nesting, wide trees, huge attribute values, long IDs/classes/path data;
- use/reference/paint/mask/clip cycles;
- data-URL lexical and decoded bombs;
- local path, traversal, symlink, absolute path, file URL, HTTP(S), and DNS-like strings;
- extreme/NaN/infinite numbers and checked-arithmetic overflow;
- curve subdivision, tessellation, correlation, filter-region, and output explosions;
- malformed images/fonts and nested SVG recursion;
- diagnostic log injection through newlines/control characters.

Assertions include exact error variant, no partial output, no external access, bounded elapsed time, bounded allocation where measurable, and no source-bearing usvg/resvg log record through shipped adapter configuration.

## 10. Fuzzing

Fuzz targets:

- bounded XML preflight and source builder;
- path/transform/style value parsers owned by the project;
- source-to-paint correlator with generated small trees;
- curve flattening and topology classification;
- target document validator/deserializer.

Seed corpus includes every focused fixture and minimized historical failure. Fuzz runs forbid panics and `unsafe`; sanitizer/Miri coverage is added where supported without relaxing the no-unsafe project rule.

## 11. Performance regression

Phase 4 adds criterion benchmarks for source parse, usvg adaptation, correlation, curve flattening, raster fallback, target validation, and serialization. Bench fixtures represent tiny, `arch.svg`, 1 MiB/5k-node, path-heavy, text-heavy, and fallback-heavy cases. CI uses statistically meaningful regression thresholds after baselines stabilize.

## 12. Make targets and gates

Implementation adds Makefile targets, not shell scripts:

- `test-fixtures` — structural feature-matrix integration tests;
- `test-compat` — pinned Excalidraw restore/render compatibility;
- `test-visual` — deterministic differential render suite;
- `test-hostile` — limit/security corpus;
- `fuzz` — documented local fuzz entry;
- `verify` — full project-prescribed Rust gates plus the applicable converter gates.

Rust-changing phases run `cargo build`, `cargo test`, `cargo +nightly fmt`, and `cargo clippy -- -D warnings`; strict pedantic lints are used where they add signal. Dependency phases also run `cargo audit` and `cargo deny check`.

## 13. Cross-references

- ← [Mapping design](./svg-mapping-design.md)
- ← [Emission design](./excalidraw-emission-design.md)
- ↔ [Security and performance](./svg-to-excalidraw-security-performance.md)
- → [Implementation plan](./svg-to-excalidraw-impl-plan.md)
