# 最後一個強制換行之後的行

fork 的 commit：Make the line after a final break only where asked。

## 建議的標題

Add `Layout::set_line_after_final_break`

## 改了什麼

- `Layout::set_line_after_final_break(made: bool)`：文字以強制換行結尾時，換行之後那一行（上面什麼都沒有）要不要做出來。預設 `true`，和現在一樣；`Layout::clear` 和重新 build 時回到 `true`。要在斷行之前呼叫。
- `LayoutData::line_after_final_break` 存這個設定。
- `BreakLines::break_next`：最後一行沒有 item、而且前面還有別的行時，設定是 `false` 就把它拿回來、回傳 `None`。逐行斷行（`break_lines().break_next()`）的呼叫者也看不到那一行。
- 一共 30 行左右的實作，加 `parley/src/tests/test_final_break.rs`。

## 為什麼

parley 現在一律做出那一行：編輯器要在那裡放游標。CSS 不要：最後一個強制換行之後沒有內容時沒有 line box，`<div>a<br></div>` 是一行高（`white-space: pre` 的 `a\n` 也是）。用 parley 做 CSS inline layout 的 embedder 現在只能在斷完之後自己丟掉那一行，再把 `Layout::height()` 之類的值扣回來；逐行斷行、行高各自不同時要扣的不只一個數字。

Blink 的做法：line breaker 在內容結束時停下來，不產生那一行；`<textarea>` 這種要游標位置的，在值以換行結尾時多放一個 placeholder `<br>`。所以「要不要」是呼叫者的事，不是文字本身的性質。

## 測試

`test_final_break.rs`：

- 關掉時 `a\n` 和 `\n` 是一行，`a\n\n` 是兩行（只拿掉最後那一行）；`a\nb` 不受影響。
- 空的文字仍然有一行：第一行不會被拿掉。
- 預設時 `a\n` 是兩行（保護編輯器的行為）；重新 build 進同一個 layout 時回到預設。
- 換行之後有一個 out-of-flow box 的那一行不是空的，留著（高度是 0）。
- 每個案例都把一次斷完（`break_all_lines`）和逐行斷（`break_lines().break_next()`）的行數和高度比一次。

## reviewer 可能會問的

- **為什麼是 `Layout` 上的 setter，不是 `StyleProperty`？** 它是整個 layout 的設定，不是某段文字的；和 `set_alignment`、`set_text_indent` 放在一起。上游想放別的地方（例如斷行的參數）的話照改。
- **名字。** `set_line_after_final_break` 是描述性的；上游可能想要更短的，或倒過來的名字（`set_trailing_empty_line` 之類）。
- **`PlainEditor` 會不會受影響？** 不會，它用預設。
- **和 Blitz 的關係。** Blitz 也是 CSS 的 embedder，應該有同樣的需要；送之前在上游的 issue 和 Blitz 裡找一下有沒有人提過或繞過這件事，有的話連過去。（還沒找。）

## Changelog 的分類

`### Added`、`#### Parley`：`Layout::set_line_after_final_break`。
