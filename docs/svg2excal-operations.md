# svg2excal Operations and Verification

## Interfaces

The `svg2excal` CLI reads one document per process. Input is capped at 16 MiB,
conversion uses the core's complete limit set, and file outputs use a temporary
file in the destination directory followed by an atomic persist. The CLI never
resolves SVG resource paths.

The server exposes `GET /health` and `POST /v1/convert`. Query fields are
`profile=balanced|editable|fidelity|strict` and `includeReport=true|false`.
Success returns `{document, report?}`. Error bodies contain only a stable code;
they never echo source bytes, URLs, IDs, text, or parser messages.

The server is an unauthenticated local-process adapter and refuses non-loopback
bind addresses. Default service limits are a 16 MiB body, four admitted
requests/conversions, and a 30 second whole-request deadline. Timed-out work
receives cooperative cancellation; a bounded supervisor retains its task and
blocking semaphore permit until it exits. Deploy conversion in process-isolated
workers when hard preemption of upstream normalization/rasterization is required.
Do not expose this adapter directly to untrusted clients. A network deployment
must place an authenticated, rate-limited reverse proxy in front of it with
bounded connections plus request-body and response-write deadlines; the
in-process deadline ends when the bounded response body is produced, not when a
slow downstream socket finishes consuming it.

## Resource and privacy policy

The shipped adapters do not install a filesystem or network resource provider.
Raster data URLs are MIME/magic checked, dimension-probed, and decoded only
under core budgets. Nested SVG is disabled by default. Structured server logs
contain bounded counts, profile, result code, and fallback pixels only. The
logging filter disables `usvg` and `resvg`; embedders must preserve that rule for
hostile content.

Deterministic normalization uses bundled Liberation Sans and Noto Emoji fonts
plus a Basic CJK subset of Xiaolai. Their provenance and redistribution terms
are documented in [third-party assets](./svg2excal-third-party-assets.md).

## Verification

- `make test-fixtures`: semantic feature matrix and canonical RFC fixture.
- `make test-hostile`: parser, resource, geometry, and target-validator attacks.
- `make test-compat`: pinned Excalidraw restore/re-export compatibility.
- `make test-visual`: 1×/2× SSIM, CIE76 solid/foreground color, p99 edge distance,
  and alpha-aware fallback-border checks.
- `make fuzz`: bounded campaigns for all five parser, correlation, geometry, and
  validator fuzz targets; `FUZZ_SECONDS` controls the per-target duration.
- `make bench`: Criterion end-to-end tiny, `rfc.svg`, 1 MiB/5,000-node,
  10 MiB/50,000-node, path-heavy, text-heavy, 16 MP fallback, deterministic
  rerun, serialization, and target-validation regressions.
- `make package`: synchronized fixture check, clean-room Cargo package builds,
  and crates.io's 10 MiB archive-size limit for all workspace crates.

The 2026-08-13 reference run used an Apple M5 Pro (`arm64`), macOS 26.5.2, and
Rust 1.97.1. Criterion measured the 24 KiB canonical `rfc.svg` conversion at
4.01 ms. Treat latency numbers as release gates only on controlled hardware;
shared CI compiles benchmarks while scheduled runs exercise the full corpus and
fuzz suite.

The RFC visual differential requires SSIM of at least 0.95 at 1× and 2×, exact
solid-interior mean CIE76 delta no greater than 1.0, foreground mean CIE76 delta
no greater than 3.5, and bidirectional p99 edge distance no greater than 2.25
CSS pixels. These thresholds account for the fixture's explicitly diagnosed
font, marker, dash, filter, and clip approximations.

## Failure interpretation

Conversion errors are typed and source-redacted. `StrictFidelityViolation`
means the input is valid but the selected profile forbids a required
approximation, omission, or fallback. `LimitExceeded` means a deterministic
resource budget was exhausted. `ResourceDenied` means input requested an
unapproved external or recursive resource. No partial document is returned.
