# svg2excal

`svg2excal` converts hostile or ordinary SVG/SVGZ input into deterministic,
validated Excalidraw v2 JSON. It keeps supported shapes, connectors, text, and
groups editable and uses bounded PNG fallback only for complete paint islands
that Excalidraw cannot represent faithfully.

## Quick start

```bash
cargo run -p svg2excal -- fixtures/arch.svg --output arch.excalidraw \
  --profile balanced --report arch.report.json
```

Use `-` as the input or output path for stdin/stdout. File outputs and reports
are committed atomically. The four profiles are:

- `balanced`: native editability, diagnosed approximations, then minimal fallback;
- `editable`: prefers bounded vector decomposition;
- `fidelity`: prefers localized raster fallback over approximation;
- `strict`: rejects painted content that is not exactly representable.

Library usage:

```rust
use svg2excal_core::{ConversionOptions, convert};

let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
  <rect width="10" height="10" fill="red"/>
</svg>"#;
let options = ConversionOptions::default();
let result = convert(svg, &options)?;
let json = result.document.to_pretty_json_with_limits(&options.limits)?;
# Ok::<(), svg2excal_core::ConversionError>(())
```

The core never performs network or filesystem access. External resources are
denied unless an embedder supplies an explicit bounded provider; nested SVG is
also opt-in. Do not bridge `usvg`/`resvg` log records into application logs for
hostile input because upstream messages may contain source-derived strings.

## HTTP service

```bash
cargo run -p svg2excal-server
curl --data-binary @fixtures/arch.svg \
  'http://127.0.0.1:3000/v1/convert?profile=balanced&includeReport=true'
```

Configuration is read from `config/server.yaml` and `SVG2EXCAL__...`
environment overrides. Request bodies, conversion concurrency, and time are
bounded. A timed-out blocking conversion retains its semaphore permit until the
worker really exits. This unauthenticated adapter is deliberately local-only and
rejects non-loopback bind addresses.

See [operations and verification](docs/svg2excal-operations.md) for API details,
limits, fuzzing, benchmarks, and production guidance.

## Development

```bash
make verify
FUZZ_SECONDS=60 make fuzz
make bench
```

`make verify` runs Rust gates, hostile fixtures, pinned Excalidraw compatibility,
visual differentials, fuzz-target compilation, benchmark compilation, dependency
audit, and license/source policy checks.

## License

MIT. See [LICENSE.md](LICENSE.md).
