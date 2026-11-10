# 沒有 em 的字型

fork 的 commit：fontique: leave out fonts without an em。

## 建議的標題

fontique: skip fonts with no `head` table or zero `unitsPerEm`

## 改了什麼

`fontique/src/font.rs` 的 `FontInfo::from_font_ref`，在「沒有可用的 charmap 就不收」的檢查後面多一個：

```rust
if font.head().map_or(true, |head| head.units_per_em() == 0) {
    return None;
}
```

7 行，加 `parley/src/tests/test_font_without_em.rs`。

## 為什麼

Apple 舊式的 bitmap 字型沒有 `head`，只有 `bhed`（和 `bdat`、`bloc`）；macOS 內建的 GB18030 Bitmap 就是。read-fonts 對它回報的 em 是 0，parley 算 `font_size / units_per_em` 得到無限大，glyph 的位置和 run 的 advance 變成 NaN，整個 layout 的寬高跟著壞掉。在 macOS 上只要 fallback 走到這個字型就會遇到（它涵蓋很多 CJK 的字，系統的 fallback 清單裡有它）。

不收之後，要這個字型的文字照「找不到字型」fallback 到下一個。這些字型沒有 outline，parley 的使用者本來就畫不出它的字。

**重現的方法**（送出前在 macOS 上再確認一次，把實際看到的數字寫進說明）：用系統字型的 `FontContext`，family 指定 `GB18030 Bitmap`，排任何中文字，看 `Layout::width()`。

## 測試

`test_font_without_em.rs`：拿 `parley_dev` 的一個測試字型，直接改 bytes：

- 原樣、和把 `unitsPerEm` 改成 1000：收（確認不是「改過」本身讓它被拒絕）。
- `unitsPerEm` 改成 0：不收。
- 把 table directory 裡 `head` 的 tag 改成 `hea_`（等於沒有 `head`；tag 的排序不變）：不收。

測試放在 `parley` 的 tests 裡是因為 `parley_dev` 的字型目錄在那裡用得到；上游可能想把它放進 `fontique` 自己的測試，那要另外找字型的來源。

## reviewer 可能會問的

- **為什麼不在用到 `units_per_em` 的地方防（除以零時用預設值）？** 那樣字型還是會被選到，但畫不出東西（沒有 outline）；fallback 到下一個字型對使用者比較好。也可以兩邊都做。
- **`bhed` 的字型要不要支援？** 要支援就得讀 `bhed` 當 `head`、畫 bitmap，是另一件事；這個 PR 只讓它不再弄壞 layout。
- **OTS。** 瀏覽器的 OTS 也拒絕沒有 `head` 或 em 不在 16–16384 的 web font；這裡只擋 0，比它寬。

## Changelog 的分類

`### Fixed`、`#### Fontique`：沒有 `head` 或 em 是 0 的字型不再被收進 collection（原本讓 layout 出現 NaN）。
