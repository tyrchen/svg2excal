# Phase 0 Executable Contracts

Status: complete · Updated: 2026-08-13 · Scope: toolchain, dependency, normalization, target-compatibility, and visual baselines

## Versions and licenses

Versions were rechecked against crates.io on 2026-08-13.

| Component | Pinned version/revision | License |
| --- | --- | --- |
| Rust | 1.97.1 (2026-07-14 stable) | Apache-2.0 OR MIT |
| `usvg` | 0.48.1 | Apache-2.0 OR MIT |
| `resvg` | 0.48.1 | Apache-2.0 OR MIT |
| `tiny-skia` | 0.12.0 (through `resvg`) | BSD-3-Clause |
| Excalidraw | `ab0255f21eb40b5408f3e9ed9725474108eda9e6` | MIT |
| Liberation Sans baseline | Excalidraw-pinned regular TTF | SIL OFL-1.1 |
| Noto Emoji baseline | Excalidraw-pinned regular TTF | SIL OFL-1.1 |
| Xiaolai Basic CJK subset | Project-generated deterministic subset | SIL OFL-1.1 |

The Cargo features disable `usvg`/`resvg` system-font and memory-mapped-font discovery. Text normalization loads only the bundled faces, so the baseline does not depend on fonts installed on the host. The Xiaolai subset and its provenance are documented in [third-party assets](../svg2excal-third-party-assets.md).

## `rfc.svg` normalization contract

`make characterize` parses XML with DTD support disabled, constructs the current `usvg` tree, and produces frozen 1×/2× `resvg` renders plus a machine-readable census.

Key observations:

- the fixture is 24,213 bytes with an intrinsic size of 1920×1080 CSS pixels;
- the source contains 263 elements, including 46 rectangles, 131 texts, 12 lines, 21 paths, 13 uses, 8 circles, 5 markers, and 12 effective filter references;
- normalization produces 63 groups, 100 paths, 131 text nodes, and 12 filtered groups;
- of the normalized paths, 39 are line-only and 61 contain quadratic or cubic segments;
- all paint in this fixture resolves to solid colors (63 fills and 80 strokes);
- a focused cascade probe confirms that a stylesheet class declaration resolves ahead of a conflicting SVG presentation attribute (`#123456`, not `#abcdef`);
- without an explicit bundled fallback family, the requested Inter/Helvetica family list has no match and `usvg` drops all text. Mapping both generic serif and sans-serif fallback to bundled Liberation Sans preserves all 131 text nodes.

The frozen artifacts are in [`fixtures/baselines/`](../../fixtures/baselines/). Their SHA-256 values are checked by `make test-visual`.

## Excalidraw target contract

`make test-compat` runs a Vitest harness against the pinned Excalidraw submodule. The generated v2 fixture deliberately uses `index: null`; upstream restoration assigns valid fractional indices, preserves IDs and types, and static SVG export renders native and file-backed elements. This confirms the target profile's null-index and required-field assumptions against revision `ab0255f2`.

The harness uses upstream source directly rather than an npm release. It installs the pinned Yarn lockfile with lifecycle scripts disabled, then imports `restoreElements` and `exportToSvg` through the upstream Vitest aliases.

## Reproduction

```sh
make characterize
make test-visual
make test-compat
```

All contracts reproduced on macOS arm64 with Node 24.16.0 and Yarn 1.22.22. No Phase 0 evidence contradicts the normative spec set; no decision amendment is required.
