# 送 taffy 上游的草稿

只在本地，還沒有送出任何東西。每份對應 fork 的 `machopo` 分支上的一個 commit（`../third_party/taffy`，用 commit 的標題找）。

| 草稿 | fork 的 commit | 狀態 |
| --- | --- | --- |
| [column-width.md](column-width.md) | Don't measure the items' heights for the width of a single-line column | 草稿，等審核 |

## 送出前要做的事

草稿是材料，不是可以貼上去的說明：送出的人照它用自己的話寫，並照上游當時的規定揭露 LLM 的參與（送出前先讀上游的 `CONTRIBUTING.md` 和 PR 範本）。

1. 從上游最新的 main 開一個 branch，把那個 commit cherry-pick 過去。
2. 用 taffy 的 `rust-version`（1.71）編一次：fork 平常用 machopo 的 toolchain 編。
3. 跑上游的 CI 跑的東西：`cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`。
4. 上游收了之後：fork rebase 時那個 commit 自己消失，`README.machopo.md` 的表拿掉那一列。
