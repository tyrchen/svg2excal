# SVG to Excalidraw Key Decisions

Status: ready for implementation v1 · Owner: svg2excal maintainers · Last updated: 2026-08-04

Decisions are append-only. A future change supersedes a decision with a new ID and reverse pointers.

## D1 — Use dual source-semantic and normalized-paint representations

- **Context:** Raw SVG retains authoring intent; `usvg` resolves visual behavior but normalizes away primitive/reference/group distinctions.
- **Alternatives considered:** raw XML only; usvg tree only; browser DOM as runtime dependency.
- **Decision:** Build both trees from one validated XML document and correlate them conservatively.
- **Why:** It combines standards-aware visual truth with editable semantic promotion without requiring a browser.
- **Pinned by:** [scene model §§4–6](./conversion-scene-model-design.md#4-source-model), [ingestion §8](./svg-ingestion-design.md#8-source-to-paint-correlation)
- **Date:** 2026-08-04

## D2 — Treat usvg as visual authority, not the security boundary

- **Context:** usvg resolves SVG well but enables DTD in convenience entry points, has unbounded SVGZ decode, and defaults to local image-path reads.
- **Alternatives considered:** trust upstream defaults; fork a full SVG parser; browser sandbox.
- **Decision:** Parse once with DTD disabled, bounded-decompress ourselves, preflight data URLs, install custom resource/font resolvers, and call `Tree::from_xmltree`.
- **Why:** It retains upstream resolution quality while enforcing product limits before expensive work.
- **Pinned by:** [ingestion §§3–6](./svg-ingestion-design.md#3-parse-boundary), [security §3](./svg-to-excalidraw-security-performance.md#3-boundary-validation-sequence)
- **Date:** 2026-08-04

## D3 — Target a pinned Excalidraw v2 compatibility profile

- **Context:** Envelope version `2` is stable while element fields evolve upstream.
- **Alternatives considered:** emit a historical minimal schema; track upstream main implicitly; use untyped JSON.
- **Decision:** Mirror current typed fields at pinned commit `ab0255f...`, write envelope v2, and test restore/render compatibility.
- **Why:** A pin makes “valid Excalidraw” testable and future changes explicit.
- **Pinned by:** [emission §2](./excalidraw-emission-design.md#2-target-profile), [verification §8](./svg-to-excalidraw-verification-plan.md#8-upstream-compatibility)
- **Date:** 2026-08-04

## D4 — Default to balanced native-first conversion

- **Context:** Native-only loses appearance; image-only loses editability.
- **Alternatives considered:** always native; always raster; one hard-coded compromise.
- **Decision:** Provide balanced/editable/fidelity/strict profiles; balanced is default.
- **Why:** Users can choose the trade-off while security and reporting remain invariant.
- **Pinned by:** [PRD §7](./svg-to-excalidraw-prd.md#7-binding-product-behavior), [mapping §3](./svg-mapping-design.md#3-profile-policy)
- **Date:** 2026-08-04

## D5 — Rasterize only the smallest complete isolation boundary

- **Context:** Filters, masks, clips, blend modes, group opacity, gradients, compound fills, and complex text may be unrepresentable.
- **Alternatives considered:** whole-document image; omit effects; flatten individual children through compositing boundaries.
- **Decision:** Use `resvg::render_node` on the smallest normalized subtree that contains the complete effect.
- **Why:** It preserves exact pixels locally without sacrificing unrelated editable content or breaking compositing.
- **Pinned by:** [mapping §§4,9](./svg-mapping-design.md#4-paint-island-construction)
- **Date:** 2026-08-04

## D6 — Never use freedraw as a general SVG path carrier

- **Context:** Excalidraw freedraw applies pen/pressure smoothing; it does not preserve arbitrary Bézier/fill semantics.
- **Alternatives considered:** sample every path into freedraw; encode path data in customData; rasterize every curve.
- **Decision:** Exact line sequences use line/polygon; curves use bounded polyline approximation or fallback.
- **Why:** Target rendering behavior matches the data model and remains predictable/editable.
- **Pinned by:** [mapping §6](./svg-mapping-design.md#6-path-lowering)
- **Date:** 2026-08-04

## D7 — Do not infer diagram semantics by default

- **Context:** SVG encodes painting, not cards, lanes, node topology, frames, or arrow bindings.
- **Alternatives considered:** proximity-based grouping/binding; fixture-specific class heuristics; computer-vision reconstruction.
- **Decision:** Preserve explicit groups and exact marker semantics only; v1 always leaves frames, card groups, text containers, and bindings absent.
- **Why:** False semantics change editing behavior and are worse than ungrouped but faithful geometry.
- **Pinned by:** [mapping §§7,10](./svg-mapping-design.md#7-markers-and-arrows), [PRD §4](./svg-to-excalidraw-prd.md#4-non-goals)
- **Date:** 2026-08-04

## D8 — Map explicit SVG groups to groupIds, never ordinary frames

- **Context:** Excalidraw groups are ID membership; frames are visible clipping containers.
- **Alternatives considered:** source `<g>` to frame; discard all groups; infer user groups from normalized rendering layers.
- **Decision:** Map meaningful explicit source groups/use instances with two or more outputs to deepest-to-shallowest `groupIds` without reordering.
- **Why:** It preserves selection hierarchy without inventing layout/clipping semantics.
- **Pinned by:** [mapping §10](./svg-mapping-design.md#10-groups-z-order-and-semantic-inference), [emission §10](./excalidraw-emission-design.md#10-final-validator)
- **Date:** 2026-08-04

## D9 — Make output byte-deterministic

- **Context:** Upstream constructors use random IDs/seeds and current timestamps.
- **Alternatives considered:** preserve randomness; normalize only in tests; accept semantic-only determinism.
- **Decision:** Domain-separated content-derived IDs/nonzero seeds/nonces, stable timestamps, stable maps/order, canonical numeric grids, and canonical pretty JSON.
- **Why:** It enables reviewable diffs, caching, reproducible tests, and stable automation.
- **Pinned by:** [emission §§7,11](./excalidraw-emission-design.md#7-deterministic-identity)
- **Date:** 2026-08-04

## D10 — Emit null fractional indices and preserve array paint order

- **Context:** Excalidraw exported elements allow null index and restore repairs invalid indices from array order.
- **Alternatives considered:** reimplement fractional indexing; omit the field; use arbitrary monotonic strings.
- **Decision:** Serialize `index: null` in profile v1 and enforce source paint order in the array.
- **Why:** It uses an upstream-supported path, avoids a needless ordering implementation, and remains compatibility-tested.
- **Pinned by:** [emission §4](./excalidraw-emission-design.md#4-common-element-fields), [verification §8](./svg-to-excalidraw-verification-plan.md#8-upstream-compatibility)
- **Date:** 2026-08-04

## D11 — Use deterministic target-compatible font fallback

- **Context:** SVG can request arbitrary fonts/weights; Excalidraw has fixed numeric families and no weight field.
- **Alternatives considered:** system-font-dependent output; always rasterize text; embed arbitrary font IDs.
- **Decision:** Bundle/version target-compatible Liberation Sans metrics, accept bounded caller fonts for source layout, and map/substitute with target remeasurement and diagnostics.
- **Why:** Simple diagram text remains editable and deterministic; complex cases retain a fidelity fallback.
- **Pinned by:** [ingestion §6](./svg-ingestion-design.md#6-font-policy-and-text-normalization), [mapping §8](./svg-mapping-design.md#8-text-mapping)
- **Date:** 2026-08-04

## D12 — Core conversion is synchronous and I/O-free by default

- **Context:** Parsing/mapping is CPU-bound; services need async orchestration but core determinism should not depend on a runtime.
- **Alternatives considered:** async core API; internal thread pool; global font/resource state.
- **Decision:** `convert(&[u8], &ConversionOptions)` is synchronous, I/O-free, and denies external references. A separate `convert_with_resources` requires an explicit bounded policy/provider context; apps wrap either path in `spawn_blocking`, timeouts, and concurrency caps.
- **Why:** It is easy to embed/test and prevents hidden tasks, I/O, cancellation, and global-state behavior.
- **Pinned by:** [scene model §2](./conversion-scene-model-design.md#2-public-api), [security §8](./svg-to-excalidraw-security-performance.md#8-work-budget-and-cancellation)
- **Date:** 2026-08-04

## D13 — Deny all external resources by default

- **Context:** SVG image/font references can read files or trigger SSRF.
- **Alternatives considered:** follow browser behavior; allow relative paths by default; fetch HTTPS automatically.
- **Decision:** Default to fragments and bounded allowlisted raster data URLs only; optional local files require an explicit sandbox root; v1 has no network provider.
- **Why:** Conversion of an uploaded document must not expand its authority.
- **Pinned by:** [ingestion §5](./svg-ingestion-design.md#5-resource-policy), [security §§4–5](./svg-to-excalidraw-security-performance.md#4-active-content-and-injection)
- **Date:** 2026-08-04

## D14 — Preserve source background as an element

- **Context:** A full-viewport rectangle may look like app background but can carry paint-order/crop/edit semantics.
- **Alternatives considered:** always promote to `appState.viewBackgroundColor`; drop white backgrounds; profile-dependent magic.
- **Decision:** Keep it as the bottom-most element in v1 and use white app background independently.
- **Why:** It avoids semantic guessing and makes source paint explicit/editable.
- **Pinned by:** [emission §3](./excalidraw-emission-design.md#3-document-envelope), [verification §6](./svg-to-excalidraw-verification-plan.md#6-rfcsvg-acceptance)
- **Date:** 2026-08-04

## D15 — Scale oversized scenes uniformly below target restoration limits

- **Context:** Current Excalidraw restoration replaces extremely large linear elements, and the renderer warns on coordinates/sizes beyond one million.
- **Alternatives considered:** emit and hope; reject every large but otherwise valid SVG; split coordinates into frames.
- **Decision:** Uniformly scale scenes above 70,000 px extent in non-strict profiles and report it; strict mode rejects.
- **Why:** It preserves relative geometry and importability without silent target-side deletion.
- **Pinned by:** [security §2](./svg-to-excalidraw-security-performance.md#2-default-hard-limits), [emission §10](./excalidraw-emission-design.md#10-final-validator)
- **Date:** 2026-08-04

## D16 — Diagnostics are part of successful output

- **Context:** Format gaps are unavoidable and callers must distinguish exact, approximate, fallback, and omission outcomes.
- **Alternatives considered:** warnings in logs; fail on any loss; undocumented best effort.
- **Decision:** Return a structured deterministic report beside every successful document; strict mode returns the complete violation set as a typed error.
- **Why:** Loss becomes testable and automatable rather than hidden.
- **Pinned by:** [scene model §8](./conversion-scene-model-design.md#8-diagnostics-and-report), [PRD goals G3/G8](./svg-to-excalidraw-prd.md#3-goals)
- **Date:** 2026-08-04
