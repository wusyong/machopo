# 送 parley 上游的草稿

只在本地，還沒有送出任何東西。每份對應 fork 的 `machopo` 分支上的一個 commit（`../third_party/parley`，用 commit 的標題找）。

| fork 的 commit | 狀態 | 送之前的下一步 |
| --- | --- | --- |
| Edges of non-atomic inline boxes | 沒有草稿 | 先開 issue：不佔字元、有寬度、黏著前後內容的 box 是新的一類項目，上游要不要、API 長什麼樣要先談 |
| Make the line after a final break only where asked | [final-break.md](final-break.md)，等審核 | 照草稿寫 PR |
| fontique: leave out fonts without an em | [font-without-em.md](font-without-em.md)，等審核 | 照草稿寫 PR |
| Take the first font when it covers a cluster | 沒有草稿 | 先開 issue：改的是選字型的順序，對其他使用者是行為的變化 |
| Lay out tabs at tab stops | 沒有草稿 | 和上游還沒合併的 #934 比較，寫出差異；#934 先合併的話，改成決定用哪一邊（RFC 的「待決定」） |
| Break lines after soft hyphens with a hyphen | 沒有草稿 | 和上游還沒合併的 #827 比較，寫出差異；#827 先合併的話同上 |
| Share a layout's shaped text | 沒有草稿 | 先開 issue：動到 `Layout` 的資料怎麼擁有 |
| Keep shapings: of layouts alike, and of words | 沒有草稿，暫時只在 fork | 先開 issue：快取的上限和計數是照瀏覽器的需要訂的，上游要的可能是別的形狀 |
| Look line breaks up in a table | [line-break-table.md](line-break-table.md)，等審核 | 先量測，再照草稿寫 PR |
| Take font families as ids already looked up | 沒有草稿 | 小的 API，不另外寫草稿：送的時候照 commit 的訊息寫 PR |
| Set a property of a tree builder's root style | 沒有草稿 | 同上（對應上游已有的 `RangedBuilder::push_default`） |

沒有草稿的八個不先寫：真的要送某一個時再寫那一份（那時的上游才是要對照的），要開 issue 的先開 issue。#934 和 #827 的差異現在沒有查過，它們合併、fork rebase 遇到時才知道衝突多大。

## 送出前每一份都要做的事

上游的 PR 範本（`.github/pull_request_template.md`）和 LLM 的規定（<https://linebender.org/wiki/llm-policy/>）有兩條會影響這些草稿：

- **PR 的說明要用自己的話寫，不能是 LLM 寫的。** 所以這裡的草稿是材料，不是可以貼上去的說明：列出改了什麼、為什麼、怎麼測的、reviewer 可能問什麼。送出的人照這些自己寫。
- **PR 的第一行要揭露 LLM 的參與**（`LLM Contributions: …`）。fork 的每個 commit 的實作和測試都有 LLM 參與，照實寫（上游最近的 PR 的寫法：`Initial implementation, review.`、`Generated, review.`）。送出前先讀一次規定，確認這樣的參與程度上游收。

另外：

1. 從上游最新的 main 開一個 branch，把那個 commit cherry-pick 過去。有草稿的三個 commit 單獨放到 `1b67b75` 上都只有機械性的衝突（`parley/src/tests/mod.rs` 的 `mod` 清單；斷行表多一處 `parley/src/context.rs`，旁邊是 fork 的快取加的方法）。
2. 拿掉 `machopo:` 的註解標記（註解的內容留著，改成上游的語氣），測試檔開頭的 `//! machopo:` 也是。
3. 在 `CHANGELOG.md` 的 `[Unreleased]` 加一條（每份草稿有建議的分類）。
4. 跑上游的 CI 跑的東西：`cargo test --workspace`、`cargo clippy --workspace --all-targets`、`cargo fmt --all -- --check`，和它 CI 的 `no_std` 檢查。
5. 上游收了之後：fork rebase 時那個 commit 自己消失，`README.machopo.md` 的表拿掉那一列。
