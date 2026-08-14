# svg2excal

`svg2excal` converts SVG and SVGZ input into deterministic, validated
Excalidraw v2 JSON. Supported shapes, text, groups, and connectors remain
editable; paint that Excalidraw cannot represent faithfully falls back to
bounded, local PNG images.

[View the RFC source SVG](https://github.com/tyrchen/svg2excal/blob/master/fixtures/rfc.svg)
· [Download the generated Excalidraw scene](https://github.com/tyrchen/svg2excal/raw/master/fixtures/rfc.excalidraw)

[![RFC lifecycle example](https://raw.githubusercontent.com/tyrchen/svg2excal/master/fixtures/rfc.svg)](https://github.com/tyrchen/svg2excal/blob/master/fixtures/rfc.svg)

The checked-in `.excalidraw` file is the exact deterministic output of the
checked-in SVG under the default `balanced` profile. Open it in
[Excalidraw](https://excalidraw.com/) to inspect and edit the 251 generated
elements.

## Install

Install the command-line converter from crates.io:

```bash
cargo install svg2excal --locked
```

Or add the conversion library to a Rust project:

```bash
cargo add svg2excal-core
```

Rust 1.97.1 or newer is required.

## Command-line usage

```bash
svg2excal fixtures/rfc.svg --output rfc.excalidraw \
  --profile balanced --report rfc.report.json
```

Use `-` as the input or output path for stdin/stdout. File outputs and reports
are committed atomically. Run `svg2excal --help` for the complete interface.

The conversion profiles are:

- `balanced`: prefers native editability, then diagnosed approximations, then
  minimal fallback;
- `editable`: prefers bounded vector decomposition;
- `fidelity`: prefers localized raster fallback over approximation;
- `strict`: rejects painted content that is not exactly representable.

## Library usage

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
also opt-in. Input, decoded resources, geometry, target size, fallback pixels,
and serialization are all guarded by explicit limits.

## Local HTTP service

The optional server is an unauthenticated loopback-only adapter:

```bash
cargo install svg2excal-server --locked
svg2excal-server

curl --data-binary @fixtures/rfc.svg \
  'http://127.0.0.1:3000/v1/convert?profile=balanced&includeReport=true'
```

Configuration is read from `config/server.yaml` when running from the
repository and from `SVG2EXCAL__...` environment overrides everywhere. The
service refuses non-loopback bind addresses. Put authentication, rate limits,
and network-level deadlines in front of it before any broader deployment.

## Documentation

- [Operations and verification](https://github.com/tyrchen/svg2excal/blob/master/docs/svg2excal-operations.md)
- [Library API](https://docs.rs/svg2excal-core)
- [Release process](https://github.com/tyrchen/svg2excal/blob/master/docs/svg2excal-releasing.md)
- [Third-party font assets](https://github.com/tyrchen/svg2excal/blob/master/docs/svg2excal-third-party-assets.md)
- [Design specifications](https://github.com/tyrchen/svg2excal/blob/master/specs/index.md)

## Development

The repository uses the pinned Rust toolchain and vendored compatibility
references:

```bash
git submodule update --init --recursive
make verify
FUZZ_SECONDS=60 make fuzz
make bench
```

`make verify` runs Rust gates, hostile fixtures, pinned Excalidraw
compatibility, visual differentials, fuzz-target compilation, benchmark
compilation, package dry-runs and size checks, dependency audit, and
license/source policy checks.

Do not bridge `usvg` or `resvg` log records into application logs for hostile
input because upstream messages may contain source-derived strings.

## License

The Rust project is
[MIT licensed](https://github.com/tyrchen/svg2excal/blob/master/LICENSE.md).
Bundled fonts remain under the SIL Open Font License 1.1; see
[third-party assets](https://github.com/tyrchen/svg2excal/blob/master/docs/svg2excal-third-party-assets.md).
