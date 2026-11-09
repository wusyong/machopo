# Test fonts

The only fonts in test-fonts mode (`machopo --test-fonts`, always on in the
reftests and the WPT runs), so rendering doesn't depend on what the machine
has installed. The renderer embeds them with `include_bytes!` (`crates/layout`), and so does the toolbar's address field for its text (`crates/shell`, Lato only).

| File | What | License | From |
| --- | --- | --- | --- |
| `Ahem.ttf` | Every common character is a full em square (ascent 0.8em, descent 0.2em); the space is blank. Layout tests use it so glyphs are solid boxes. | Public domain (CC0 where that isn't recognized), per the W3C Ahem README | [WPT `fonts/Ahem.ttf`](https://github.com/web-platform-tests/wpt/blob/master/fonts/Ahem.ttf) |
| `Lato-Medium.ttf` | A real text face; every generic family (`sans-serif`, `serif`, `monospace`, ...) maps to it. | SIL Open Font License 1.1, see `Lato-OFL.txt` | [google/fonts `ofl/lato`](https://github.com/google/fonts/tree/main/ofl/lato) |

SHA-256, as downloaded on 2026-09-30:

```
b719ecb31c5b21fc573c03f6421c74ac63c271a5a3ff841e34f9705fb94b8448  Ahem.ttf
d3ac182a6833e005745dd75679fbad081c0b12535df4e93ad8ed57817a31a338  Lato-Medium.ttf
```
