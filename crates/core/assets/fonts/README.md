# Bundled font assets

These fonts make SVG text measurement and fallback rendering deterministic.
They are runtime data included in the `svg2excal-core` crate, not Rust project
source licensed under MIT.

| File | Source | Treatment | SHA-256 |
| --- | --- | --- | --- |
| `LiberationSans-Regular.ttf` | Excalidraw commit `ab0255f2`; upstream Liberation Fonts | Unmodified | `42231919b6322858a1e80dfdaf671b0a530694392f50633604a6eff06ddb1628` |
| `NotoEmoji-Regular.ttf` | Excalidraw commit `ab0255f2`; upstream Noto Emoji | Unmodified | `0ed584d111778b259f205a40807d78131de02a5b04dad4e24c43cc84f793b634` |
| `Xiaolai-Regular-Basic-CJK.ttf` | Excalidraw commit `ab0255f2`; upstream Xiaolai | Deterministic subset of the original `17e58fb25e7a421b64ebea1c50104fadf752d9045fb102501434bce577e22b3f` file | `4d67dc7d7f917480be1439dc2af7ca3fdd2deaa9a022617f9c3e610271323d61` |

All three fonts are distributed under the SIL Open Font License 1.1 in
[`OFL.txt`](./OFL.txt). The Xiaolai subset retains CJK punctuation, the Basic
CJK Unified Ideographs block, CJK Compatibility Ideographs, and half/fullwidth
forms. It drops hinting and characters outside those ranges so the published
crate remains below crates.io's archive-size limit.

The copyright attributions retained in the font metadata are:

- Liberation Sans: digitized data © 2007 Ascender Corporation; © 2013 Google
  LLC.
- Noto Emoji: © 2013 Google LLC.
- Xiaolai: © 2020 LXGW.

Regenerate and verify the checked-in assets with `make update-font-assets`.
