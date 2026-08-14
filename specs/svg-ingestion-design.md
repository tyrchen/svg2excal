# SVG Ingestion and Normalization Design

Status: ready for implementation v1 · Owner: svg2excal maintainers · Depends on: PRD, conversion scene model

## 1. Purpose

This subsystem turns bounded SVG/SVGZ bytes into both a source-semantic document and a resolved paint document. It owns XML safety, SVG feature census, resource/font policy, CSS/reference/unit/viewport/transform normalization, and source-to-paint correlation. It does not decide target lowering.

## 2. Architecture

```text
Caller bytes + validated options
              │
              ▼
┌──────────────────────────────────────┐
│ Format gate                         │
│ - byte cap                          │
│ - UTF-8 or bounded SVGZ decode      │
│ - content/root validation           │
└──────────────────┬───────────────────┘
                   ▼
┌──────────────────────────────────────┐
│ roxmltree parse (DTD disabled)       │
│ - depth/node/attribute/text caps     │
│ - duplicate ID/reference census     │
│ - active/external/data URL census   │
└────────────┬─────────────────────────┘
             │ same validated XML document
       ┌─────┴─────────────────────┐
       ▼                           ▼
┌───────────────────┐   ┌────────────────────────────┐
│ Source builder    │   │ usvg::Tree::from_xmltree  │
│ tags/groups/refs  │   │ custom resources/fonts    │
│ byte provenance   │   │ resolved visual truth     │
└─────────┬─────────┘   └──────────────┬─────────────┘
          └──────────────┬──────────────┘
                         ▼
                ┌───────────────────┐
                │ Correlator        │
                │ exact / unique /  │
                │ ambiguous/generated│
                └─────────┬─────────┘
                          ▼
                   CorrelatedScene
```

## 3. Parse boundary

The implementation MUST NOT call `usvg::Tree::from_data`, `from_str`, or `from_data_nested` on untrusted input because those paths enable DTD parsing and/or unbounded SVGZ decompression in upstream. It also MUST NOT install `ImageHrefResolver::default_data_resolver`, which reaches `from_data_nested` for embedded SVG. Instead:

1. Reject input over `max_input_bytes` before allocation or decoding.
2. Detect gzip magic. If SVGZ is disabled, return `InputRejected::SvgzDisabled`. If enabled, stream-decompress through a byte-counting reader into a pre-sized buffer capped by `max_decompressed_bytes` and compression-ratio limit.
3. Validate UTF-8 and reject NUL/control characters outside XML-legal ranges.
4. Parse once with `usvg::roxmltree::Document::parse_with_options` and `allow_dtd: false`.
5. Enforce product limits by iterative traversal before normalization.
6. Pass that same document to `usvg::Tree::from_xmltree`.

The document root MUST be an SVG namespace `<svg>`. Namespace-aware matching is required; prefix spelling is irrelevant. Processing instructions are ignored with an informational diagnostic. DTD, entity declarations, scripts, event attributes, animation, and active foreign content are rejected or ignored according to the feature matrix; none execute.

## 4. Feature census

Before `usvg` can discard unsupported constructs, the source builder records:

- every element kind, namespace, attribute count, and byte range;
- style blocks and selectors, including unsupported at-rules;
- local/external references and reference cycles;
- `<defs>`, `<use>`, symbols, markers, paint servers, filters, clips, and masks;
- images, data URLs, nested SVG, and external resource requests;
- text/tspan/textPath/font feature use;
- script, event, animation, and `foreignObject` presence;
- non-finite or extreme numeric tokens rejected during normalization.

Unknown foreign-namespace metadata may be ignored within size limits. Unknown SVG-namespace rendering elements produce `unsupported-svg-element`; strict mode fails if they could paint.

## 5. Resource policy

The default `convert` entry point has no resource provider and denies every external reference:

- Local fragment references are allowed and cycle-checked.
- Bounded `data:` images are allowed only for an allowlisted raster MIME set.
- HTTP, HTTPS, file URLs, absolute paths, and relative filesystem paths are denied.
- Nested data-URL SVG is disabled by default; when enabled explicitly it consumes the same aggregate nesting, bytes, nodes, and raster budgets.

Data URLs are length-checked in the source DOM before calling `usvg`; this is essential because upstream decodes the URL before invoking its custom resolver. Both resolver closures are replaced. Before parent normalization, the ingestion stage walks resource requests in source order, deduplicates them by canonical request/content digest, reserves aggregate budgets, and builds an immutable resolved-asset store. Raster bytes are MIME/magic/dimension checked; explicitly enabled nested SVG recursively runs through the same bounded decompression, DTD-disabled parse, census, and `Tree::from_xmltree` flow under shared depth/node/byte/work budgets. Cycles are rejected by the active content-digest/reference stack.

The usvg data and string resolver closures perform immutable store lookups only: no I/O, decoding policy decision, recursive parsing, or budget mutation occurs inside an upstream callback. The data resolver rechecks MIME/decoded digest before returning a cloned `ImageKind`; the string resolver returns no data under default conversion. No nested bytes are ever delegated back to an upstream convenience parser.

The explicit `convert_with_resources` entry point requires a `ResourceContext` containing both the core-enforced allow policy and a `ResourceProvider`. V1 policy can admit validated relative file references only; it cannot admit URL schemes. The core validates the request and reserves byte/work budget before the callback, and it validates the returned MIME, magic, and size afterward.

The application-supplied sandboxed-directory provider canonicalizes its configured root and candidate, rejects NUL, `..`, absolute paths, URL schemes, and separators inconsistent with the host, verifies the canonical target starts with the root, opens without following an unexpected replacement symlink where the platform permits, and enforces per-resource plus aggregate byte limits. A generic provider is not an authority escape hatch: requests outside the core policy never reach it. Network retrieval is not part of v1.

## 6. Font policy and text normalization

Font selection MUST be deterministic. Core does not scan system fonts by default. The implementation provides a versioned bundled font set containing the target-compatible Liberation Sans metrics and may accept caller-provided bounded font bytes keyed by validated family names.

The resolver records requested family/style/weight/stretch, selected face, fallback reason, and coverage gaps. A missing source face falls through its declared family list and CSS generic family to the configured deterministic fallback. It never downloads a font or consults an operating-system fallback. Coverage is evaluated per emitted run so mapping can split among pinned target faces or choose fallback instead of producing environment-dependent glyphs.

The paint model retains both:

- `usvg` text chunks/spans/layout/bounds for exact source-side placement; and
- author text plus source keys for editable target text.

Font substitution changes metrics. Native text placement is computed from the target font's measured bounds and SVG anchor/baseline, not by copying SVG `x`/`y`. The mapping design decides whether the substituted result fits the profile's geometry budget.

## 7. Viewports, units, and transforms

The normalized coordinate space is CSS pixels at 96 DPI. `usvg` resolves physical, font-relative, percentage, viewport, nested SVG, and `preserveAspectRatio` behavior. The adapter records:

- root intrinsic size and viewBox;
- content and layer bounds separately;
- every node's local and absolute affine transform;
- stroke-aware bounds and non-scaling-stroke implications;
- whether the affine decomposition contains reflection or skew.

All parent transforms are baked into emitted geometry because Excalidraw `groupIds` do not carry transforms. Rotation is stored in radians. Skew is not primitive-native but may be baked into line/polygon points or handled by bounded path approximation. Reflection is baked into linear/polygonal point geometry, normalized for symmetric primitives when provably identical, and may use negative target image scale; reflected text or asymmetric primitives fall back unless another exact lowering exists.

## 8. Source-to-paint correlation

Correlation runs after both trees exist.

### 8.1 Keys and anchors

- Every source node has a `SourceKey` independent of SVG ID.
- A unique, valid SVG ID is a strong anchor but never sufficient by itself.
- Duplicate IDs are diagnosed; reference resolution follows SVG/usvg behavior, but semantic promotion from a duplicate ID is prohibited.
- Paired hierarchy positions, source tag, normalized geometry signature, resolved paint signature, bounds, and document paint order form a fingerprint.

### 8.2 Expansions

The correlator models:

- `<use>` as one source instance to a generated subtree plus its referenced definition provenance;
- markers as a connector plus generated marker paint nodes;
- text as one source node to chunks/spans/flattened glyph paint;
- paint-order splits as one source path to multiple normalized paint nodes;
- unsupported/nonpainting source nodes as one-to-zero.

### 8.3 Matching algorithm

1. Align exact unique-ID anchors that also pass tag/geometry plausibility.
2. Walk intervals between anchors in source paint order and normalized paint order.
3. Generate candidate matches constrained by expansion kind and ancestry.
4. Score geometry, paint, hierarchy, and order; select a monotonic minimum-cost alignment.
5. Mark only a single zero-conflict candidate as `UniqueFingerprint`.
6. Mark ties, contradictions, duplicate-ID matches, and unexplained expansions `Ambiguous`.

The algorithm is bounded by `max_correlation_candidates`; it falls back to paint-only lowering rather than quadratic blow-up. Native primitive/arrow/group promotion requires `Exact` or `UniqueFingerprint`. Visual fallback does not require correlation.

## 9. `rfc.svg` normalization contract

The fixture MUST prove:

- all 18 class rules are applied with correct cascade and inheritance;
- `.card` class fill resolves to white on every card while per-card strokes remain intact;
- all 13 `<use>` instances expand with instance paint and composed transforms;
- all 12 marker-bearing connector paths preserve marker side, orientation, and effective size;
- the four orthogonal path connectors normalize to their exact turn points;
- all 131 text nodes retain decoded Unicode, resolved anchor, requested font metadata, and finite bounds;
- 12 filtered groups retain their filter/isolation boundary;
- source paint order remains recoverable after expansions.

## 10. Errors and diagnostics

Fatal errors include malformed XML, invalid root, disallowed DTD/entity, limit violation, resource denial in strict resource mode, reference expansion overflow, no usable viewport, and non-finite normalized geometry.

Ignored active content, unsupported static features, font substitution, duplicate IDs, ambiguous correlation, and unavailable resources are diagnostics when policy permits continued conversion. Diagnostic text never includes unbounded raw attribute values.

## 11. Relevant AGENTS.md sections

- Safety & Security: hostile boundary, string/collection/range/depth limits, path traversal, checked arithmetic — binding.
- Error Handling: typed `thiserror` boundary errors — binding.
- Type Design & API: private validated newtypes, `TryFrom`, no error-as-`Option` — binding.
- Async & Concurrency: no spawned work or runtime dependency in core. Explicit application-provided resource reads are synchronous bounded callbacks, so async applications wrap the complete conversion in `spawn_blocking`, timeout it, and cap concurrency.
- Serialization: N/A for XML input; source model is typed and validated immediately.
- Testing/Performance/Documentation: binding, including fuzz/property tests for parser and correlation.

## 12. Cross-references

- ← [Conversion scene model](./conversion-scene-model-design.md)
- → [Mapping design](./svg-mapping-design.md)
- ↔ [Security and performance](./svg-to-excalidraw-security-performance.md)
- ↔ [Research study](../docs/research/study-svg-to-excalidraw.md)
