# Roadmap — SVG to Editable Excalidraw

Status: ready for implementation v1 · Owner: svg2excal maintainers · Last updated: 2026-08-04

## 0. Principles

- **Always importable.** Every milestone produces Excalidraw data accepted by the pinned target profile.
- **Editability with honest loss.** Native content stays native; every approximation/fallback is reported.
- **Safety is a feature.** No milestone accepts uncontrolled parsing, external access, or unbounded work.
- **Contracts before breadth.** Typed output, determinism, reports, and validation land before adding more SVG constructs.

## 1. User-visible milestone graph

```text
┌───────────────────────────┐
│ M0 Trustworthy core       │
│ minimal SVG → valid JSON  │
│ deterministic + explained│
└─────────────┬─────────────┘
              ▼
┌───────────────────────────┐
│ M1 Editable diagrams      │
│ rfc.svg native shapes,    │
│ text, icons, connectors   │
└─────────────┬─────────────┘
              ▼
┌───────────────────────────┐
│ M2 Hybrid fidelity        │
│ minimal image islands for │
│ effects/complex geometry  │
└─────────────┬─────────────┘
              ▼
┌───────────────────────────┐
│ M3 Production ready       │
│ hostile inputs, compat,   │
│ performance, adapters     │
└───────────────────────────┘
```

## 2. M0 — Trustworthy core

**User outcome:** A caller converts a bounded SVG containing solid rectangles, ellipses, simple lines/polygons, and simple text into valid Excalidraw v2 JSON and receives an explicit report.

**Includes:** typed API/errors/options, deterministic IDs/order/JSON, target validator, minimal safe XML/usvg path, balanced/strict behavior, no external resources.

**Exit criteria:**

- a documented library example converts a minimal SVG;
- the result loads through pinned Excalidraw restore without element loss;
- repeated conversion is byte-identical;
- malformed, DTD-bearing, oversized, external-resource, and unsupported strict inputs return exact typed errors;
- all Rust and dependency gates for the changed surface pass.

**Calendar:** 3–4 engineering weeks after Phase 0 characterization.

## 3. M1 — Editable architecture diagrams

**User outcome:** Diagram-like SVGs using CSS, groups, transforms, defs/use, markers, routed paths, and simple text become meaningfully editable scenes. `fixtures/rfc.svg` is the reference experience.

**Includes:** dual source/paint trees, cascade/reference normalization, correlation, path flattening, arrowhead recognition, group IDs, deterministic font fallback and target text measurement, and bounded omission of recognized cosmetic drop shadows.

**Exit criteria:**

- every `rfc.svg` structural assertion in the verification plan passes;
- all 131 strings are editable target text;
- representable cards/lanes/connectors are native;
- correct marker direction, CSS conflict resolution, icon transforms, and paint order are verified;
- no frames/bindings/card groups are invented;
- balanced visual score meets the pre-fallback portion of the calibrated gate and all loss is diagnosed.

**Calendar:** 5–7 additional engineering weeks; cumulative 9–12 including Phase 0.

## 4. M2 — Hybrid fidelity

**User outcome:** Complex SVG remains useful: representable regions stay editable while gradients, masks, clips, filters, compound fills, complex text, and excessive curves become localized image fallbacks.

**Includes:** paint-island isolation, bounded resvg node rasterization, binary files, existing images, all four profiles, compound-path editable decomposition where safe.

**Exit criteria:**

- every feature-matrix construct has a native/approximate/fallback/strict outcome and diagnostic;
- no fallback absorbs an unrelated native text/shape when a smaller valid island exists;
- fallback islands meet SSIM/no-clipping gates at 1× and 2×;
- `rfc.svg` balanced whole-image SSIM is at least 0.95 at 1× and 2×;
- file references and output budgets validate after upstream restore.

**Calendar:** 3–4 additional engineering weeks; cumulative 12–16 including Phase 0.

## 5. M3 — Production ready

**User outcome:** The converter can safely back a CLI and bounded service workflow with stable compatibility and predictable resource use.

**Includes:** full hostile corpus, fuzzing, performance gates, target-profile compatibility automation, CLI, integration of the placeholder server through bounded `spawn_blocking`, structured/redacted telemetry, dependency audits.

**Exit criteria:**

- one-below/at/one-above tests cover every hard limit;
- fuzz targets have no known panic/hang/unbounded-allocation issue at the agreed run budget;
- performance targets pass on documented hardware;
- current pinned Excalidraw compatibility and full visual/fixture/hostile gates pass via Make targets;
- CLI/service documentation explains profiles, diagnostics, resource policy, and limits.

**Calendar:** 3–4 additional engineering weeks; cumulative 15–20 focused engineering weeks including Phase 0.

## 6. Calendar calibration

For one experienced developer, plan 17–23 calendar weeks including review, fixture authoring, CI stabilization, and normal coordination overhead. Two developers can overlap target-model/compatibility work with SVG normalization/fixture work after Phase 0, reducing elapsed time to roughly 11–15 weeks; mapping and final hardening still contain serial integration points.

## 7. Deferred beyond M3

These are named future products, not hidden M3 requirements:

- optional diagram-semantic inference (cards, bindings, frames) with a separate confidence contract;
- stylized hand-drawn import profile;
- browser/WASM packaging;
- remote resource provider under a dedicated SSRF design;
- reverse Excalidraw-to-SVG round trip;
- animated/interactive SVG.

## 8. Cross-references

- [PRD](./svg-to-excalidraw-prd.md)
- [Implementation plan](./svg-to-excalidraw-impl-plan.md)
- [Verification plan](./svg-to-excalidraw-verification-plan.md)
