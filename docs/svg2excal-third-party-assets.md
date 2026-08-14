# Third-party assets

`svg2excal-core` embeds fonts so normalization and raster fallback do not depend
on host-installed fonts. The Rust project remains MIT licensed; the font files
remain under the SIL Open Font License 1.1.

| Bundled family | Copyright/source | License |
| --- | --- | --- |
| Liberation Sans | [Liberation Fonts](https://github.com/liberationfonts/liberation-fonts) contributors | SIL OFL 1.1 |
| Noto Emoji | [Noto Emoji](https://github.com/googlefonts/noto-emoji) contributors | SIL OFL 1.1 |
| Xiaolai | Copyright © 2020 LXGW; [Xiaolai source](https://github.com/lxgw/kose-font) | SIL OFL 1.1 |

The exact provenance, transformations, checksums, and full license text ship in
[`crates/core/assets/fonts`](../crates/core/assets/fonts/README.md). The
Xiaolai file is a deterministic Basic CJK subset; the other two files are
unmodified copies from the pinned Excalidraw reference commit.

No bundled font is sold by itself. Redistribution of `svg2excal-core` must keep
the font copyright notices and `OFL.txt` with the font software.
