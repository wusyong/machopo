# RFC 0105 的進度：layout 的座標模型和分段

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-19

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a 型別和 converter | 完成 | 2026-11-19 |
| 1b inline | | |
| 1c table | | |
| 1d float、list marker、line clamp、置中、slider 的 thumb | | |
| 1e 第 1 階段的檢查和文件 | | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 1a：型別和 converter

macOS、Apple M4，2026-11-19。

- 新的測試：`layout` 的 `logical::` 的單元測試 4 個：5 種 writing mode 乘 2 種 direction 的位移、大小、rect 轉成物理的再轉回來，和四邊的對應（CSS Writing Modes 4 § 6.4 的表）；位移的預期值照 Blink 的 `WritingModeConverter`。
- mutation test（`mutants-1a.toml`）9 個，全部抓到：`rtl` 的橫排沒鏡射、從遠的那一邊量時沒扣掉內層的大小、`sideways-lr` 的 inline-start 放在上面、四邊從錯的那一邊讀、轉回物理時 block-start 和 block-end 對調、直排的大小沒對調、轉回邏輯時沒換回軸、rect 用邏輯的大小轉位移、inline 的和加到 block 的那一邊。
- `ComputedStyle::writing_direction()` 這一段還沒有程式用（一律回答橫排、`ltr`），弄壞它沒有東西會變：1b 開始用時一起測。
- 跑過：`cargo test -p layout --lib logical::`、`cargo test -p computed_style`；`sanity.py numerics`、`tests`、`unsafe`、`deps`、`docs`。
