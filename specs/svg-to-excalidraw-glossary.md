# SVG to Excalidraw Glossary

Status: ready for implementation v1 · Owner: svg2excal maintainers

## Approximation

An editable target representation with a known, measured visual or semantic difference from the resolved SVG, within the selected profile's configured error and complexity budgets.

## Binding

An Excalidraw relationship that makes an arrow endpoint or text follow a target container. SVG geometry near a shape is not itself binding evidence.

## Computed style

The final SVG property values after CSS cascade, specificity, source order, presentation attributes, inline style, inheritance, and defaults. It is distinct from the attributes physically present on an element.

## Correlation

The bounded relationship between one source-semantic node and zero or more normalized paint nodes. Correlation confidence controls whether semantic promotion is safe.

## Exact

Representable without a known semantic approximation and within renderer anti-aliasing tolerances defined by verification. Exact does not mean byte-identical raster output.

## Fallback

A target image generated from the smallest unsupported normalized paint island. Fallback preserves appearance but reduces editability within that island.

## Feature census

The source-side inventory taken before normalization can discard unsupported or nonpainting constructs. It drives diagnostics and strict-mode decisions.

## Frame

An Excalidraw visible rectangular container with membership/clipping semantics. It is not the target equivalent of an arbitrary SVG `<g>`.

## Group

In source SVG, an authoring or compositing container. In Excalidraw, repeated `groupIds` membership on elements, with no transform. A normalized usvg group may instead be only a rendering isolation layer.

## Isolation boundary

The smallest normalized group that must be rendered as a unit because filters, masks, clips, blending, or group opacity observe combined child pixels.

## Native

An editable target rectangle, ellipse, diamond, line, arrow, text, or image whose strict representability predicate passes.

## Normalized paint tree

The `usvg`-derived rendering tree after CSS, units, references, text layout, transforms, and bounds are resolved. It is visual truth, not authoring truth.

## Paint island

The smallest independently classifiable/renderable set of normalized paint nodes. It is the unit of native conversion, approximation, or fallback.

## Paint order

The SVG rendering order after reference/marker expansion and compositing semantics. It becomes Excalidraw element array order; grouping must not change it.

## Profile

One of balanced, editable, fidelity, or strict. A profile selects how representational gaps are handled; it never relaxes safety limits.

## Semantic promotion

Choosing a higher-level target type—such as rectangle, arrow, diamond, or user group—based on correlated source intent and resolved geometry rather than emitting generic path geometry.

## Source semantic tree

The bounded original-XML-derived tree retaining tags, explicit groups, IDs, references, author text, and byte-range provenance. It is authoring truth, not final visual truth.

## Strict mode

A profile that returns no document if any effective painted construct would require approximation, fallback, or omission.

## Target profile

A converter-owned compatibility contract for a specific pinned Excalidraw schema/restore behavior, even when upstream keeps the same envelope version number.

## Visual truth

The pixels/geometry implied after the SVG cascade, units, transforms, references, text layout, and compositing are resolved.

## Cross-references

- [Scene model](./conversion-scene-model-design.md)
- [Mapping design](./svg-mapping-design.md)
- [Research study](../docs/research/study-svg-to-excalidraw.md)
