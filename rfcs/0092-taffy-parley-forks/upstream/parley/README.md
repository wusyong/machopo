# 送 parley 上游的草稿

只在本地，還沒有送出任何東西。每份對應 fork 的 `machopo` 分支上的一個 commit（`../third_party/parley`，用 commit 的標題找）。

| 草稿 | fork 的 commit | 狀態 |
| --- | --- | --- |
| [final-break.md](final-break.md) | Make the line after a final break only where asked | 草稿，等審核 |
| [font-without-em.md](font-without-em.md) | fontique: leave out fonts without an em | 草稿，等審核 |
| [line-break-table.md](line-break-table.md) | Look line breaks up in a table | 草稿，等審核；送出前要量測 |

其餘的 commit（邊緣、第一個字型、tab stop、軟連字號、共用 shaped text、shaping 的快取）還沒有草稿。

## 送出前每一份都要做的事

上游的 PR 範本（`.github/pull_request_template.md`）和 LLM 的規定（<https://linebender.org/wiki/llm-policy/>）有兩條會影響這些草稿：

- **PR 的說明要用自己的話寫，不能是 LLM 寫的。** 所以這裡的草稿是材料，不是可以貼上去的說明：列出改了什麼、為什麼、怎麼測的、reviewer 可能問什麼。送出的人照這些自己寫。
- **PR 的第一行要揭露 LLM 的參與**（`LLM Contributions: …`）。這三個 commit 的實作和測試都有 LLM 參與，照實寫（上游最近的 PR 的寫法：`Initial implementation, review.`、`Generated, review.`）。送出前先讀一次規定，確認這樣的參與程度上游收。

另外：

1. 從上游最新的 main 開一個 branch，把那個 commit cherry-pick 過去。三個 commit 單獨放到 `1b67b75` 上都只有機械性的衝突（`parley/src/tests/mod.rs` 的 `mod` 清單；斷行表多一處 `parley/src/context.rs`，旁邊是 fork 的快取加的方法）。
2. 拿掉 `machopo:` 的註解標記（註解的內容留著，改成上游的語氣），測試檔開頭的 `//! machopo:` 也是。
3. 在 `CHANGELOG.md` 的 `[Unreleased]` 加一條（每份草稿有建議的分類）。
4. 跑上游的 CI 跑的東西：`cargo test --workspace`、`cargo clippy --workspace --all-targets`、`cargo fmt --all -- --check`，和它 CI 的 `no_std` 檢查。
5. 上游收了之後：fork rebase 時那個 commit 自己消失，`README.machopo.md` 的表拿掉那一列。
