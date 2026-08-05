# svg2excal Specification Index

Status: ready for implementation v1 · Last updated: 2026-08-04

This spec set defines a deterministic Rust converter from static SVG to editable Excalidraw v2 JSON. File names follow the project rule `<feature>-<type>.md`; the table below supplies the dependency order that numeric file prefixes would otherwise encode.

## Reading order

| Order | Specification | Type | Purpose |
| ---: | --- | --- | --- |
| 1 | [SVG to Excalidraw PRD](./svg-to-excalidraw-prd.md) | PRD | Product contract, users, goals, and non-goals |
| 2 | [Conversion scene model](./conversion-scene-model-design.md) | Data-model design | Validated types, source/paint IR, diagnostics, and result contract |
| 3 | [SVG ingestion and normalization](./svg-ingestion-design.md) | Component design | Secure parse, CSS/reference/unit/transform resolution, and correlation |
| 4 | [SVG mapping and fallback](./svg-mapping-design.md) | Component design | Native predicates, geometry lowering, profiles, groups, and raster islands |
| 5 | [Excalidraw emission](./excalidraw-emission-design.md) | Component design | Target schema, deterministic IDs/order, files, and final validation |
| 6 | [Security and performance](./svg-to-excalidraw-security-performance.md) | Cross-cut | Threat model, resource limits, work budgets, and performance targets |
| 7 | [Verification plan](./svg-to-excalidraw-verification-plan.md) | Verification plan | Unit/property/fixture/compatibility/visual gates |
| 8 | [Glossary](./svg-to-excalidraw-glossary.md) | Glossary | Disambiguation of source, render, and target terms |
| 9 | [Roadmap](./svg-to-excalidraw-roadmap.md) | Stakeholder roadmap | User-visible milestones and exit criteria |
| 10 | [Implementation plan](./svg-to-excalidraw-impl-plan.md) | Engineer plan | Dependency-ordered phases, tasks, estimates, and verification |
| 11 | [Key decisions](./svg-to-excalidraw-key-decisions.md) | Decision log | Load-bearing choices and rejected alternatives |

## Build-order graph

```text
┌───────────────────────────┐
│ PRD                       │
│ outcomes / loss contract  │
└─────────────┬─────────────┘
              ▼
┌───────────────────────────┐
│ Conversion scene model    │
│ types / invariants/report │
└─────────────┬─────────────┘
              ▼
┌───────────────────────────┐
│ SVG ingestion             │
│ source tree + paint tree  │
└─────────────┬─────────────┘
              ▼
┌───────────────────────────┐
│ Mapping planner           │
│ native / approx / fallback│
└─────────────┬─────────────┘
              ▼
┌───────────────────────────┐
│ Excalidraw emitter        │
│ typed v2 JSON + assets    │
└─────────────┬─────────────┘
              ▼
┌───────────────────────────┐
│ CLI / service integration │
│ bounded I/O only          │
└─────────────┬─────────────┘
              ▼
┌───────────────────────────┐
│ Corpus + compatibility +  │
│ visual hardening gates    │
└───────────────────────────┘

Security/performance and verification constrain every box.
```

## Research basis

- [Study: SVG to editable Excalidraw conversion](../docs/research/study-svg-to-excalidraw.md)
- [`fixtures/arch.svg`](../fixtures/arch.svg), the M1 end-to-end acceptance fixture
- Vendored Excalidraw, `resvg/usvg`, and the earlier official converter under `../vendors/`

## Normative conventions

- **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are normative.
- “Exact” means equivalent within the verification plan's pixel and geometry tolerances, not byte-identical SVG rendering.
- Unsupported or approximated behavior is never silent; it appears in the conversion report.
- `AGENTS.md` is binding. Component specs cite the relevant sections instead of duplicating project-wide engineering rules.
