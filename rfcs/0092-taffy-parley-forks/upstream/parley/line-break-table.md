# 斷行表

fork 的 commit：Look line breaks up in a table。

這一份最大（實作約 880 行，其中表本身佔大半），**建議先開 issue 問上游要不要這個方向，再送 PR**；下面的材料兩者都用得到。上游正在把 segmentation 移出 `parley_engine`（#755），送之前先看它的進度：它合併之後這個 commit 要跟著搬位置。

## 建議的標題

Look up line break opportunities for Latin text in a pair table

## 改了什麼

- `parley_engine/src/line_table.rs`（新）：字元到 U+017F、和 U+2000 到 U+203F（Latin-1、Latin Extended-A、最常見的標點）的斷行機會查表，做法和 UAX #14 的 pair table 一樣：每個字元一個 class，前後兩個 class 查出「可以斷」「不可以斷」「表決定不了」。
- 表的內容就是 ICU4X 的 line segmenter 在預設規則下給的結果：一個測試（`tables_are_up_to_date`）從 ICU4X 產生表、和檔案裡的比，不一樣就失敗並印出新的表。ICU4X 或 Unicode 升級時照印出來的貼上。
- 表決定不了的（字裡有表以外的字元，inline object 的 U+FFFC 也算；或某一對的結果要看更遠的字元，例如引號）：那個字連同前後的字交給 ICU4X。
- 只在預設規則下用：有 `word-break`、`line-break`、`overflow-wrap: anywhere` 之類的設定時整段照原來的路走 ICU4X。
- debug build 裡每段查表的文字也整段交給 ICU4X 再比一次，不一樣就 panic。
- `AnalysisOptions::skip_word_boundaries`（`LayoutContext::set_word_boundaries`，預設開）：不算 word boundary。沒有人用 parley 的 cluster 按字移動游標的文字（網頁的內文）不需要它，ICU4X 的 word segmenter 的時間可以省掉。**這是另一件事，送的時候拆成另一個 PR**（它很小，而且和表無關也有用）。
- `Analyzer::line_breaks_looked_up`、`LayoutContext::line_breaks_looked_up`（`#[doc(hidden)]`）：查表的文字數和交給 ICU4X 的字數，給測試用。上游不想要公開的計數的話，改成 `#[cfg(test)]` 或拿掉，測試改用別的方式確認走了哪條路。

## 為什麼

ICU4X 的 line segmenter 對每個字元走它的 state machine 和資料表，通用但慢；拉丁文字的網頁上它佔 `analyze_text` 的大部分時間。Blink 的 `LazyLineBreakIterator` 也是先查自己的 ASCII / Latin-1 pair table，查不到才交給 ICU。

**量測（送出前要補）**：machopo 自己的數字是整頁的 layout 時間，不能直接拿來說服上游。送之前用上游的 `parley_bench` 量有表和沒有表的 `analyze_text`（或整個 build）在拉丁文字上的時間，數字寫進說明；沒有明顯的差別就不要送。

## 測試

- `line_table.rs` 自己的：`tables_are_up_to_date`（表和 ICU4X 產生的一致）；`short_texts_break_as_icu4x_breaks_them`（每個 class 各一個字元、空白、表決定不了的和表以外的字元，組成所有四個字元以內的文字，逐一和 ICU4X 比）；`long_texts_break_as_icu4x_breaks_them`（五萬段固定種子的隨機文字，逐一和 ICU4X 比）。
- `parley/src/tests/test_line_break_table.rs`：拉丁文字查表（計數）、有歐元符號或引號的字交給 ICU4X、有其他規則的整段不查、inline object 算表以外的字元、關掉 word boundary 時斷行機會不變。
- debug build 的比對在 parley 所有的測試上都跑到。

## reviewer 可能會問的

- **多了一份要維護的表。** 表是產生的，過期時測試會失敗並印出新的；沒有手寫的規則。
- **為什麼只到 U+017F 和 U+2000–U+203F？** 再大表就不小了，而且 CJK、泰文等本來就要 ICU4X 的字典或更複雜的規則。範圍可以調。
- **正確性靠什麼保證？** 產生表的測試、debug build 的全量比對。release build 裡沒有比對。
- **`no_std`。** 表是 `const` 陣列，不需要 `std`。
- **和 #755 的衝突。** 見開頭。

## Changelog 的分類

`### Changed`、`#### Parley Engine`：預設規則下拉丁文字的斷行機會查表（結果不變，比較快）。`skip_word_boundaries` 另一條 `### Added`。
