# RFC 0124 的進度：跨網站的導覽換 renderer

開始日期：2026-11-27。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、盤點（`details.md` 的「盤點」「Chromium」）、量 renderer 啟動的時間、已決定的事項第 1 到 12 條 | 做完 | 2026-11-27 |
| 1. 鋪路：commit 時才定送給哪個 renderer（`CommitParams`、`RenderProcessHost::commit_navigation`）；renderer 在導覽途中不在了照已決定的事項第 13 條（`WebContents::renderer_gone`）；自動化照分頁的 renderer 重新接上 DevTools 的 session（`WebContents::renderer_id`），繼續的導覽的 `Page.navigate`、`Machopo.waitForLoad` 繼續等 | 做完，待審核 | 2026-11-27 |
| 2. DevTools 的 session 的狀態跨過換 renderer | 還沒開始 | |
| 3. 規則和換：跨網站的 commit 在新的 renderer；舊文件的 `unload`、逾時；thread 的 renderer 一樣換 | 還沒開始 | |
| 4. 收尾：量測工具、WPT、`--window` 的情境、文件、整個 workspace 的測試 | 還沒開始 | |

第 1 段開始之前先做 RFC 0125（網址列的偽造，2026-11-27 使用者決定）。

在別的機器上要做的：macOS 和 Windows 上量 renderer process 啟動的時間（已決定的事項第 5 條），明顯比這台 Linux 的 11 ms 慢時重新考慮提前啟動和預備的 renderer。

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| renderer 在 Browser 發起的導覽途中不在了（自己結束、壞訊息、`about:crash`）：導覽繼續，commit 時啟動新的 renderer（新的 frame sink）再送，新的 renderer 的 commit 收下 | `cargo test -p browser --test it renderer_gone::the_browsers_navigation_commits_in_a_new_renderer` |
| 頁面發起的導覽跟著 renderer 取消，history 不變、不為它啟動 renderer | `renderer_gone::a_pages_navigation_goes_with_its_renderer` |
| 頁面的 `history.back()` 跟著取消，Browser 的上一頁繼續 | `renderer_gone::a_pages_traversal_goes_with_its_renderer_and_the_browsers_goes_on` |
| 已經送出 commit 的跟著 renderer 丟掉，網址列回到目前的頁面 | `renderer_gone::a_navigation_sent_to_commit_goes_with_the_renderer` |
| 自動化：載入途中 renderer 被結束，`Page.navigate` 在新的 renderer commit 時回答，session 接到新的 renderer 上、命令照常 | `cargo test -p shell --test it automation_pipe::a_navigation_on_its_way_as_the_renderer_goes_commits_in_a_new_one` |
| 等著的東西在 renderer 不在了時失敗，繼續的導覽的 load 除外 | `windowless::a_renderer_gone_fails_what_waits_for_it` |

其餘的段做到時補上。

## 做過的檢查

### 第 0 步（2026-11-27，Linux：Fedora 45，x86-64，AMD Radeon 890M）

- 盤點讀的是 `main` 的 `acdd0704`；Chromium 的原始碼是當天的 `main`，由另一個 agent 抓下來讀、整理，關鍵的引文（`kUnloadTimeout`、`GetFrameHostForNavigation` 叫兩次、初始的 `SiteInstance`、paint holding 的條件、`file:` 的 site、WebUI 的規則、`about:` 改寫成 `chrome://`、`--single-process`）對著抓下來的檔案抽查過。
- renderer 啟動的時間：release、sandbox 開著，新分頁到跑出第一句 script 的中位數 12.3、12.4 ms（兩輪各 30 次），已有 renderer 的分頁導覽 1.3、1.4 ms（`details.md` 的「量的：啟動一個 renderer process 要多久」）。
- 沒有改程式。`python3 scripts/sanity.py docs` 通過。

### 第 1 段開始之前（2026-11-27，Linux）

- 臨時的測試（加在 `crates/browser/tests/it/visible_url.rs`，試完還原）：Browser 發起的導覽還在載入時 renderer 不在了（`RendererGone`），之後回應到了：commit 照樣送給不在了的那個 renderer（導覽在開始時就綁了它），分頁一直是 sad tab、什麼都沒 commit，網址列一直顯示要去的網址（那一次導覽一直在「送出 commit、還沒回報」）。RFC 0125 的已決定的事項第 3 條說要寫的測試，那時漏了；第 1 段改成 commit 時才定送給哪個 renderer 時一起處理。Chromium 的做法在 `details.md` 的「renderer 在導覽的途中不在了」。

### 第 1 段（2026-11-27，Linux：Fedora 45，x86-64，AMD Radeon 890M）

- **新的測試**：`browser` 的 `tests/it/renderer_gone.rs`（4 個；renderer 另一端沒有東西，三種結束的方式各跑一次），`shell` 的 `automation_pipe::a_navigation_on_its_way_as_the_renderer_goes_commits_in_a_new_one`。
- **改了的測試**：`windowless::a_renderer_gone_fails_what_waits_for_it` 原本預期 Browser 發起、還在路上的導覽的 `Machopo.waitForLoad` 也失敗：照第 13 條那次導覽繼續，它的 load 照樣等，其餘四種照舊失敗。
- **和原本的計畫不同**：`headless` 跟著換 sink、視窗清掉跟著舊 renderer 的 UI 狀態移到第 3 段（`README.md` 的「現在最接近的路：`relaunch`」）：這一段結束時換 renderer 只在 renderer 不在了之後，那時 `headless` 本來就以錯誤結束，視窗在 `RendererGone` 時就清掉了，做了也測不到。
- **mutation test**（`mutants-1.toml`，9 個）：全部抓到。
- **段落結束的檢查**：`cargo test --workspace` 5,042 個通過（1 個 ignored）；`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`（抓到測試裡一個太複雜的型別，改成型別別名）、`sanity.py` 的每一項都通過。改了啟動 renderer 的時機和 commit 怎麼送，跑了量測工具的測試：四個都通過。
- **WPT**：`html/browsers/history/the-location-interface/`、`browsing-the-web/overlapping-navigations-and-traversals/`、`browsing-the-web/history-traversal/` 共 153 個測試。不符預期的只有 `overlapping-navigations-and-traversals/cross-document-traversal-cross-document-nav.html`：第一個子測試穩定地通過（RFC 0125 的 `progress.md` 記過，`main` 上也是）；第二個（「fast cross-document traversal and then slow cross-document navigation」）在跑整個目錄時通過了一次，單獨跑這個 branch 11 次都照預期失敗，`main`（`c527c2ee`）8 次裡通過 1 次：`main` 上本來就有的偶發。兩個都照 2026-11-27 使用者的決定不追。

