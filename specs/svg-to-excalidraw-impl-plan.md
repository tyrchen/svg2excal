# Implementation Plan — Dependency-Ordered SVG Conversion

Status: ready for implementation v1 · Owner: svg2excal maintainers · Last updated: 2026-08-04

## 0. Readiness assessment

The design is ready for Phase 0. The research study pins current Excalidraw and `usvg/resvg` source and closes the architectural choice. The repository itself is still a template: core and server contain placeholders, no toolchain pin exists, and no SVG/geometry/font dependencies are present.

Before any Cargo manifest edit, current stable Rust and dependency versions MUST be rechecked because project policy requires latest stable versions at implementation time. The researched 2026-08-04 baseline is `usvg/resvg` 0.48.1, `roxmltree` 0.21.1 through usvg, `kurbo` 0.13.1, and `winnow` 1.0.4 where project-owned string grammars need it.

## 1. Why dependency order differs from feature order

Stakeholders first see “convert `arch.svg`,” but implementation cannot safely start with that fixture's tags. Deterministic target types, diagnostics, limits, and validation must exist before a parser can produce public data. Likewise, CSS/use/marker mapping depends on the dual source/paint model, and raster fallback depends on paint-island planning plus the file schema. Building in UI/fixture order would make every earlier contract provisional.

## 2. Estimated effort

Focused engineering: 15–20 weeks for one experienced developer, based on the task estimates below. Calendar estimate with review/CI/coordination: 17–23 weeks. Work can parallelize after Phase 0 between target compatibility/emission and source normalization/fixtures, but Phases 2–4 each end with a serial integration gate.

## 3. Phase 0 — Risk retirement and executable contracts (3–5 days)

No production converter code lands in this phase.

| # | Deliverable | Evidence | Effort |
| --- | --- | --- | ---: |
| 0.1 | Pin current upstream/dependency/toolchain versions and licenses | version/license table; `cargo info`; vendor pins | 0.5 d |
| 0.2 | Characterize `arch.svg` through current usvg | resolved counts/styles/bounds; CSS conflict; use/marker/filter tree | 1 d |
| 0.3 | Prove minimal hand-authored v2 JSON import/restore/render | pinned Excalidraw compatibility harness | 1 d |
| 0.4 | Calibrate visual comparison and font baseline | 1×/2× reference render, font assets, frozen metric procedure | 1–2 d |
| 0.5 | Record results and amend specs only if evidence contradicts them | dated research spike(s), index update | 0.5 d |

**Exit gate:** all four executable contracts reproduce from Make targets; no unresolved contradiction exists between upstream behavior and specs. If a contradiction appears, update the research/spec decision before Phase 1 rather than coding around it.

**Verification:** documentation/link checks, compatibility harness, reference-render reproducibility, `make check-agent-sync` only if agent/skill files change. No Rust build gate for research-only output.

## 4. Phase 1 — Typed spine and trustworthy core / closes M0 (3–4 weeks)

| # | Task | Spec | Effort |
| --- | --- | --- | ---: |
| 1.1 | Pin latest stable Rust 2024 toolchain; replace placeholders; add crate roots with forbid/warn lints and module docs | AGENTS.md; [scene model](./conversion-scene-model-design.md) | 1 d |
| 1.2 | Add reviewed workspace dependencies with minimal features; update deny policy | [security §11](./svg-to-excalidraw-security-performance.md#11-dependency-and-supply-chain-gates) | 1 d |
| 1.3 | Implement validated options/limits/newtypes and `ConversionError` | [scene model §§2,9](./conversion-scene-model-design.md#2-public-api) | 2–3 d |
| 1.4 | Implement typed Excalidraw v2 envelope/base/shape/line/arrow/text/image/file models | [emission §§2–6](./excalidraw-emission-design.md#2-target-profile) | 3 d |
| 1.5 | Implement deterministic ID/seed/nonce and stable JSON | [emission §§7,11](./excalidraw-emission-design.md#7-deterministic-identity) | 1–2 d |
| 1.6 | Implement whole-document target validator | [emission §10](./excalidraw-emission-design.md#10-final-validator) | 2–3 d |
| 1.7 | Implement minimal bounded XML/usvg path and exact solid basic-shape/text mapping | [ingestion §3](./svg-ingestion-design.md#3-parse-boundary); [mapping §5](./svg-mapping-design.md#5-native-geometry-predicates) | 3–4 d |
| 1.8 | Expose public `convert`, report, docs/example, and minimal CLI-less integration test | [PRD §7](./svg-to-excalidraw-prd.md#7-binding-product-behavior) | 2 d |
| 1.9 | Add `test-compat`, `test-fixtures`, and phase-scoped `verify` Make targets | [verification §§8,12](./svg-to-excalidraw-verification-plan.md#8-upstream-compatibility) | 1 d |

**Exit criteria:** M0 roadmap criteria pass; minimal supported SVG produces byte-stable target JSON; target validator and pinned upstream restore agree; strict mode rejects every non-native focused fixture; no external resources are read.

**Verification:** `cargo build`, `cargo test`, `cargo +nightly fmt`, `cargo clippy -- -D warnings`, targeted pedantic/security lints, `cargo audit`, `cargo deny check`, `make test-compat`, and minimal `make test-fixtures`.

## 5. Phase 2 — Generic normalization and editable diagrams / closes M1 (5–7 weeks)

| # | Task | Spec | Effort |
| --- | --- | --- | ---: |
| 2.1 | Implement bounded SVGZ/XML preflight, feature census, source keys/tree/reference graph | [ingestion §§3–4](./svg-ingestion-design.md#3-parse-boundary) | 3–4 d |
| 2.2 | Implement deny-by-default resource and deterministic font providers | [ingestion §§5–6](./svg-ingestion-design.md#5-resource-policy) | 3 d |
| 2.3 | Adapt normalized usvg tree into private paint types with checked f64 geometry | [scene model §5](./conversion-scene-model-design.md#5-resolved-paint-model) | 3 d |
| 2.4 | Implement bounded source↔paint correlation and expansion modeling | [ingestion §8](./svg-ingestion-design.md#8-source-to-paint-correlation) | 4–5 d |
| 2.5 | Implement exact primitive/line/polygon/group native predicates | [mapping §§5,10](./svg-mapping-design.md#5-native-geometry-predicates) | 3 d |
| 2.6 | Implement adaptive curve flattening, topology checks, and diagnostic budgets | [mapping §6](./svg-mapping-design.md#6-path-lowering) | 3–4 d |
| 2.7 | Implement marker recognition, explicit arrowheads, and null bindings | [mapping §7](./svg-mapping-design.md#7-markers-and-arrows) | 3 d |
| 2.8 | Implement target-font measurement, simple text mapping, span split predicate | [mapping §8](./svg-mapping-design.md#8-text-mapping) | 3–4 d |
| 2.9 | Recognize bounded source-preserving `feDropShadow` and emit deterministic omission diagnostics | [mapping §4](./svg-mapping-design.md#4-paint-island-construction) | 1–2 d |
| 2.10 | Complete `arch.svg` structural and pre-fallback visual acceptance | [verification §6](./svg-to-excalidraw-verification-plan.md#6-archsvg-acceptance) | 3 d |

**Exit criteria:** M1 roadmap criteria pass. `arch.svg` has 86 native text strings, native representable cards/connectors, correct CSS/use/marker/transform behavior, stable groups/order, no invented frames/bindings, and complete approximation reporting.

**Verification:** full Rust gates; `make test-fixtures`; `make test-compat`; `make test-visual` for native/approximation regions; property tests for transforms, flattening, ordering, IDs, and target references.

## 6. Phase 3 — Hybrid fidelity and complete profiles / closes M2 (3–4 weeks)

| # | Task | Spec | Effort |
| --- | --- | --- | ---: |
| 3.1 | Implement isolation-boundary/paint-island discovery | [mapping §4](./svg-mapping-design.md#4-paint-island-construction) | 3 d |
| 3.2 | Implement checked `resvg::render_node` fallback and deterministic PNG/file emission | [mapping §9](./svg-mapping-design.md#9-images-and-raster-fallback); [emission §6](./excalidraw-emission-design.md#6-binary-files) | 3–4 d |
| 3.3 | Implement existing raster image/nested-SVG policy | [ingestion §5](./svg-ingestion-design.md#5-resource-policy) | 2–3 d |
| 3.4 | Implement fill/stroke split, compound-path editable decomposition budget, and complex text fallback | [mapping §§5–8](./svg-mapping-design.md#5-native-geometry-predicates) | 3–4 d |
| 3.5 | Implement/freeze balanced, editable, fidelity, and strict decision matrices | [mapping §3](./svg-mapping-design.md#3-profile-policy) | 2 d |
| 3.6 | Complete feature matrix and whole-image visual gates, including `arch.svg` shadows | [verification §§5–7](./svg-to-excalidraw-verification-plan.md#5-feature-matrix-fixtures) | 3–4 d |

**Exit criteria:** M2 roadmap criteria pass; every effective supported feature has a deterministic decision/report; minimal fallback boundaries and file integrity are proven; whole `arch.svg` balanced SSIM is at least 0.98 with all region gates.

**Verification:** full Rust gates; all fixture/compat/visual targets; dependency audit/deny if manifests changed; exact rerun/hash tests for embedded PNGs.

## 7. Phase 4 — Hostile-input and production hardening / closes M3 (3–4 weeks)

| # | Task | Spec | Effort |
| --- | --- | --- | ---: |
| 4.1 | Implement one-below/at/one-above hostile-limit corpus and redaction tests | [security §§2–7](./svg-to-excalidraw-security-performance.md#2-default-hard-limits) | 3–4 d |
| 4.2 | Add fuzz targets and minimized seeds for parser/correlation/geometry/validator | [verification §10](./svg-to-excalidraw-verification-plan.md#10-fuzzing) | 2–3 d |
| 4.3 | Profile and optimize measured hot paths; add criterion regressions | [security §§9–10](./svg-to-excalidraw-security-performance.md#9-performance-targets) | 3–4 d |
| 4.4 | Build CLI adapter with bounded paths/I/O, profile/report selection, atomic output | [PRD §5](./svg-to-excalidraw-prd.md#5-users) | 2–3 d |
| 4.5 | Integrate server through body/time/concurrency caps and `spawn_blocking`; structured redacted tracing | [security §§8,12](./svg-to-excalidraw-security-performance.md#8-work-budget-and-cancellation) | 3–4 d |
| 4.6 | Complete docs, public API examples, Make `verify`, and CI compatibility/visual/hostile gates | [verification §12](./svg-to-excalidraw-verification-plan.md#12-make-targets-and-gates) | 2–3 d |

**Exit criteria:** M3 roadmap criteria pass; no known hostile corpus/fuzz issue; performance and compatibility profiles pass; CLI/service never bypass core limits or resource policy.

**Verification:** full Rust gates with strict pedantic/security lints, complete Make `verify`, audit/deny, documented fuzz run, benchmarks on reference hardware, manual diff review for public API/docs.

## 8. What makes this order correct

1. **Target invariants precede source breadth.** Every later mapping produces already-defined, validated data.
2. **Visual truth and authoring truth land together.** Generic CSS/reference fidelity is never retrofitted onto an attribute walker.
3. **Native mapping precedes fallback.** Paint islands can deliberately protect editability because native eligibility is known.
4. **Hardening follows complete paths but safety starts at Phase 1.** Default-deny resources and baseline limits exist from the first input; Phase 4 expands adversarial evidence and production adapters.

## 9. Cross-references

- [Roadmap](./svg-to-excalidraw-roadmap.md)
- [Verification plan](./svg-to-excalidraw-verification-plan.md)
- [Key decisions](./svg-to-excalidraw-key-decisions.md)
