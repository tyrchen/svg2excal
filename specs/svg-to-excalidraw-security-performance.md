# SVG to Excalidraw Security and Performance

Status: ready for implementation v1 · Owner: svg2excal maintainers · Applies to: all components

## 1. Threat model

SVG bytes, SVGZ streams, XML names/attributes/text, CSS, numeric tokens, paths, references, fonts, images, data URLs, file paths, and nested SVG are hostile. Attackers may attempt entity expansion, decompression bombs, reference cycles, path/point explosions, extreme coordinates, expensive filters, image bombs, local-file reads, SSRF, parser panics, memory exhaustion, CPU exhaustion, or log injection.

The core library performs no network access. External resource resolution is denied by default. Service adapters must not broaden this policy implicitly.

## 2. Default hard limits

All limits are validated private-field newtypes. Defaults may become stricter in a future release but never looser without a decision-log entry and corpus evidence.

| Resource | Default limit |
| --- | ---: |
| Compressed/input bytes | 16 MiB |
| SVGZ decompressed bytes | 64 MiB |
| SVGZ expansion ratio | 100× |
| XML element nodes | 100,000 |
| XML depth | 128 |
| Attributes per element | 256 |
| Total attributes | 500,000 |
| Single non-resource attribute/text-node bytes | 1 MiB |
| Single data-URL lexical bytes | 12 MiB |
| Total XML text + attribute bytes | 32 MiB |
| Unique/local references | 100,000 |
| Reference expansion depth | 32 |
| Expanded paint nodes | 250,000 |
| Parsed path segments per path | 100,000 |
| Parsed path segments aggregate | 1,000,000 |
| Correlation candidates | 1,000,000 |
| Input images | 1,024 |
| Encoded bytes per image/font | 8 MiB |
| Decoded resource bytes aggregate | 64 MiB |
| Custom fonts | 32 |
| Nested SVG depth | 8 |
| Target elements | 100,000 |
| Target local points aggregate | 1,000,000 |
| Decomposition elements per island | 10,000 |
| Raster pixels per fallback island | 16 megapixels |
| Raster pixels aggregate | 64 megapixels |
| Embedded output bytes | 64 MiB |
| Serialized JSON bytes | 64 MiB |
| Absolute target coordinate | 1,000,000 px |
| Target element extent | 70,000 px |

The 70,000 px extent stays below Excalidraw's oversized-linear restoration threshold. A larger finite source scene is uniformly scaled down to fit in balanced/editable/fidelity mode with `scene-scaled-to-target-range`; strict mode fails.

## 3. Boundary validation sequence

```text
bytes cap
  → bounded decompression
  → UTF-8/XML parse with DTD disabled
  → structural/string/reference/data-URL lexical caps
  → custom resource/font resolvers
  → normalization finite/range checks
  → path/correlation/plan work budgets
  → checked raster allocation
  → target element/point/file/JSON caps
```

Later checks do not compensate for missing earlier checks. In particular, a custom usvg data resolver cannot prevent upstream's initial data-URL decode allocation; lexical length is capped before `usvg::Tree::from_xmltree`.

## 4. Active content and injection

- Reject DTD and entity declarations.
- Ignore/reject scripts, event-handler attributes, SMIL/animation, links as interaction, and active `foreignObject`; never execute or emit them.
- Parse CSS with the chosen standards-aware parser; do not evaluate browser JavaScript, `url(javascript:)`, or dynamic variables outside the supported static feature set.
- Treat target links as absent in v1; source hyperlinks remain diagnostics/provenance only.
- Escape control/newline characters in diagnostic messages and use structured `tracing` fields in apps.
- Never pass source strings through `sh -c`, SQL formatting, HTML formatting, or URL fetch APIs.

## 5. Resource isolation

### Default

All external schemes and paths are denied. Data URLs accept only allowlisted raster MIME types after lexical/decoded caps and MIME sniffing. SVG-in-image recursion is opt-in and bounded.

### Sandboxed directory

Path input is validated, canonicalized beneath an explicit root, and opened with symlink-race defenses available on the platform. Absolute paths, parent traversal, NUL, URL syntax, and platform separators that could change interpretation are rejected. The core receives bytes through a provider; it does not expose arbitrary filesystem APIs.

### Service/network

V1 has no network provider. If added later, it requires a separate design covering HTTPS-only allowlists, DNS resolution and IP pinning, private/loopback/link-local rejection, redirects, response/body/time limits, MIME validation, and caching. A generic URL callback is not sufficient.

## 6. Numeric and geometry safety

- Parse numbers to finite bounded types at the boundary.
- Reject NaN, infinity, malformed exponents, and magnitudes outside source-coordinate policy.
- Use checked multiplication/addition for dimensions, counts, buffer sizes, stride, pixels, and serialized-size estimates.
- Evaluate geometry in `f64`; narrow to target JSON numbers only after finite/range checks.
- Curve subdivision, reference expansion, nested SVG, and tree walks are iterative or explicitly depth-bounded.
- Array access derived from input uses `.get`, iterators, or checked split APIs.
- No `unsafe` in any crate, including tests; crate roots use `#![forbid(unsafe_code)]`.

## 7. Raster safety

Before pixmap allocation, compute `width × height × 4` with checked arithmetic and reserve against per-island and aggregate pixel budgets. Filter bounds are intersected with the authorized island/output extent. Images are dimension-probed before decode and decoded through bounded libraries. Decompression and PNG encoding are deterministic and bounded.

Fallback scale is clamped to 0.25–4.0; default is 2.0 in balanced/fidelity and 1.0 in editable. The planner reduces scale deterministically to fit the aggregate pixel budget before choosing a larger fallback island. Strict mode does not rasterize.

## 8. Work budget and cancellation

Core operations consume deterministic work units for nodes, reference expansions, path segments, curve subdivisions, correlation candidates, glyphs, tessellation output, and raster pixels. Exhaustion returns `LimitExceeded::WorkUnits`.

Core is synchronous and does not spawn threads. Server adapters:

- use `tokio::task::spawn_blocking` for conversion;
- bound concurrent conversions with a semaphore;
- wrap each request in a timeout and set a cooperative cancellation flag checked between owned stages/work-budget loops;
- cap request bodies before buffering;
- await and handle every task panic/join error;
- never treat dropping a blocking-task handle as cancellation: after a response timeout, a bounded supervisor retains the join handle and semaphore permit until the task exits;
- use process-isolated workers when a deployment requires hard wall-clock termination, because upstream normalization/raster calls cannot be preempted safely in-process.

CLI handles one document per worker by default; batch mode has an explicit bounded job count.

## 9. Performance targets

Reference hardware is documented in benchmark output and CI. Targets exclude cold dependency compilation and include parsing through serialization.

| Corpus class | Target |
| --- | --- |
| 25 KiB `arch.svg` | median ≤ 50 ms; peak RSS delta ≤ 64 MiB |
| 1 MiB / 5,000 nodes, no fallback | median ≤ 250 ms; p99 ≤ 2 s |
| 10 MiB / 50,000 nodes, no fallback | ≤ 5 s within 512 MiB |
| 16 MP single fallback | ≤ 2 s raster/encode on reference hardware |
| Deterministic rerun | identical bytes and diagnostic ordering |

These are gates after functional correctness. Criterion benchmarks begin during hardening, per `AGENTS.md` § Performance; early phases use timing instrumentation and regression fixtures rather than premature microbenchmarks.

## 10. Complexity requirements

- Source and paint traversal: `O(n)`.
- Reference-cycle detection: `O(n + e)`.
- Correlation: monotonic bounded alignment; worst-case candidate work capped explicitly, never unbounded `O(n²)`.
- Curve flattening: `O(k)` in emitted samples and work-capped.
- Target validation/ID reference checks: `O(m log m)` with ordered maps or expected `O(m)` with deterministic hashing; output order remains explicit.
- Serialization: `O(output bytes)`.

## 11. Dependency and supply-chain gates

New dependencies are workspace dependencies, latest stable at implementation time, minimal-feature, pure Rust where practical, and reviewed for maintenance/license/security. The current researched baseline is `usvg`/`resvg` 0.48.1, `roxmltree` 0.21.1 through usvg, `kurbo` 0.13.1, and `winnow` 1.0.4 where a project-owned string grammar is needed. Actual versions are rechecked before modifying Cargo manifests.

Any dependency/lockfile change runs `cargo audit` and `cargo deny check` in addition to the full Rust gate. No new shell automation is added; fixture, compatibility, corpus, and visual gates receive discoverable Makefile targets.

## 12. Observability and privacy

Core returns deterministic work metrics/diagnostics and does not initialize logging. Apps create one conversion span with input byte count, feature counts, profile, caller-measured total duration, deterministic per-stage work units, output counts, fallback pixels, and result code. Wall-clock timing is observability data, not part of the deterministic conversion report. Apps never record raw SVG, text, paths, URLs, data URLs, font bytes, image bytes, or unbounded IDs. Error `Debug` output follows the same redaction contract and has tests.

The pinned usvg/resvg code uses the `log` facade and some warnings include source-derived IDs or href strings. Shipped CLI/service logging configuration MUST disable those dependency targets during hostile conversion; it must not bridge them unfiltered into tracing. Embedder documentation calls out the same requirement. Hostile-corpus tests install a capture logger and prove shipped adapters emit no dependency record containing source payloads or control characters. Project diagnostics come from the bounded source census, never from scraped upstream log text.

## 13. Cross-references

- ← [Scene model](./conversion-scene-model-design.md)
- ↔ [SVG ingestion](./svg-ingestion-design.md)
- ↔ [Mapping design](./svg-mapping-design.md)
- ↔ [Verification plan](./svg-to-excalidraw-verification-plan.md)
