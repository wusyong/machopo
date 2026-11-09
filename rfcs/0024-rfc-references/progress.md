# RFC 0024 的進度：RFC 的引用只放在 `rfcs/`

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-09。完成：2026-10-09，Windows 11（Rust 1.99.0）。三段都在 `0024` 上做：第 1、2 段一個 commit（d821510），第 3 段和收尾一個 commit。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. 規則和檢查 | 完成 | 2026-10-09 |
| 2. 遷移 | 完成 | 2026-10-09 |
| 3. 啟用 | 完成 | 2026-10-09 |

## 驗收

**規則**

| 項目 | 怎麼驗證 |
| --- | --- |
| 規則寫進 `docs/README.md` 的「每份文件放什麼」最後的其他規則（取代原本的預告規則；程式碼註解不引用 `docs/` 留成一條），`CLAUDE.md` 的「怎麼做決定」指過去；`rfcs/README.md` 和範本：已決定的事項只讓其他 RFC 引用，「動機」列出要解決的限制；「一份 RFC 做完時」第 4 步從「動機」找要刪的限制 | 讀 |
| `CLAUDE.md` 的 V8 和 `unsafe` 的規則寫進 `docs/architecture.md` 的「V8 和 `unsafe` 的位置」，`CLAUDE.md` 指過去（「已決定的事項」第 7 條） | 讀 |

**檢查**

| 項目 | 怎麼驗證 |
| --- | --- |
| `sanity.py docs` 讀 `rfcs/` 以外追蹤的文字檔，每個引用是一個問題（跳過含 NUL 或不是 UTF-8 的檔案、`4ff8d6e:tests/wpt/baselines/`、`tests/sites/` 和 `sanity.py`）；例外的三個檔案只允許四個路徑；程式碼註解的 `DOC_CITATION` 只剩 `docs/` | `python3 scripts/sanity.py docs` 是 0 個問題；在追蹤的檔案裡放一個引用，它失敗 |
| 檢查的 fixture 和 mutation test | `python3 rfcs/0024-rfc-references/citation_check_test.py scripts/sanity.py`：真的腳本是 `ok`，20 個 mutant 都是 `caught` |

**遷移**

| 項目 | 怎麼驗證 |
| --- | --- |
| `rfcs/` 以外沒有引用（開始時 852 處、152 個檔案），也改了沒帶編號的「第 N 段」「stage N」和路線圖的代號 | `python3 scripts/sanity.py docs` |
| 預告指向還沒完成的 RFC 的，那份 RFC 的「動機」列出要解決的限制（0004、0013、0023；0016 的範圍本來就不含 `CSS.supports`，0012 是路線圖，那些功能還沒有自己的 RFC） | 讀 |
| fork 的 `README.machopo.md`（`../third_party/rusty_v8`）照同樣的規則改（第 6 條），在 fork 另外 commit | `grep -nP 'RFC\|\bP[0-4]\b\|pre-rfcs\|rfcs/' ../third_party/rusty_v8/README.machopo.md` 沒有輸出 |
| 程式只改了註解和兩個 assert 訊息，行為不變；頁面上改了的文字和情境比對的字串一致 | 下面的測試 |

## 做過的檢查

全部在 2026-10-09、Windows 11 上做。

- **檢查的測試**：`sanity.py` 沒有測試的 harness，所以寫了 `citation_check_test.py`：在暫時的 git repo 裡放 21 個 fixture 檔案（每種樣式、IETF 的 RFC、`rust-lang/rfcs` 的網址、`LayerTreeHost<P>` 和 `GP3`、`P5`、`P10` 這些不算的、例外的檔案寫允許和不允許的路徑、`README.md.bak` 這種名字像例外的檔案、排除的目錄、含 NUL 和不是 UTF-8 的檔案、沒有追蹤的檔案），比對 `rfc_citations` 回報的檔案。`docs/test-gaps.md` 的「只做過手動確認」記了 `sanity.py` 沒有自動跑的測試。
- **mutation test**（在 `sanity.py` 的複本上改，工作目錄的檔案沒有動到）：20 個 mutant 全部抓到：RFC 編號不要求前導零、不認 `RFC-0021`、`rfcs/` 沒有 lookbehind、拿掉 `rfcs/` 路徑、`pre-rfcs`、MVP 的文件名、階段代號的樣式，階段代號沒有 lookbehind、沒有 lookahead、放寬成 `P[0-9]`，例外的路徑在任何檔案都允許、例外的檔案什麼都允許、例外的檔案用後綴比對，不去掉句尾的 `.`，不跳過含 NUL 和不是 UTF-8 的檔案，`rfcs/`、`4ff8d6e:tests/wpt/baselines/`、`tests/sites/`、`sanity.py` 不排除。第一輪沒抓到兩個（「`rfcs/` 沒有 lookbehind」：fixture 的網址後面沒有接 `/`；「不去掉句尾的 `.`」：fixture 的路徑包在 backtick 裡），補了 fixture 之後抓到，檢查本身沒有改。啟用之後另外在三個追蹤的檔案暫時放了引用（RFC 編號、程式碼註解的 `docs/` 和 `rfcs/`），`sanity.py docs` 三個都報、結束碼 1；拿掉「把引用加進問題」和拿掉 `DOC_CITATION` 兩個 mutant 都抓到；檔案跑完還原。
- **實測**：`main`（974666f）上數出 852 處、152 個檔案，比草稿的約 790 多，多的是 MVP 的階段代號；逐一看過不是 RFC 編號的樣式（`P0` 到 `P4`、`rfcs/pre-rfcs/` 底下的檔案、`MVP.md`），都是真的引用；誤判只有 `4ff8d6e:tests/wpt/baselines/` 裡的一處。
- **遷移的審查**：八批各由一個 agent 改寫（「遷移」），每批的 diff 逐處讀過，寫在原處的理由和 RFC 原文對過（例如 `Listener` 和 `EdgeChunks` 不用 `HeapVector`、`AbortSignal.any()` 的強參照、滾輪交給 main thread 的條件；最後一個改寫過一次，原本的濃縮漏了「compositor 只知道 layer 的範圍」）。`.rs` 的 diff 機械式確認過：兩個 assert 訊息以外，改到的行都是註解；WPT 的清單只改了 `#` 的行；`about:demo` 的 script 寫出的文字和 `demo-script.txt` 的 `wait-text` 一起改（7 處）。
- **順便改掉的過時說法**（引用拿掉之後才看得出來，都對過程式）：`Cargo.toml` 的「Only block layout in P1」、`computed_style` 的 `overflow` 的 `visible` 和 `hidden` 並用、`bindings` 的 crate 說明、`dom_api/Cargo.toml` 指向 `ci.py` 的依賴規則（在 `sanity.py`）、`layout` 測試的 UA stylesheet、兩頁 iframe 情境的「The iframes show nothing yet.」；`CLAUDE.md` 裡要新階段先寫 `Pn.md` 的那一條，改成先寫 RFC 的草稿；`tests/wpt/README.md` 原本就重複的一段刪掉一份。
- **遷移時看到、沒有確認的可能過時說法**（不在這份的範圍，留給之後）：`docs/testing.md` 的「CI」描述的 `.github/workflows/ci.yml` 已經不在 repo 裡；「只做過手動確認」說視窗模式只在 macOS 上做過，`rfcs/README.md` 的索引寫 RFC 0006 在 Windows 上驗收；「要點的東西在視窗外時才失敗」可能在 `click` 和 `wheel` 先捲進畫面之後不再成立；「`MouseEvent.view`、`window.event`」的條件，`view` 已經有了；`4ff8d6e:tests/wpt/testharness-skip.txt` 跳過 `modulepreload-as.html` 的理由是 `dataset`，已經有了；`style` 的 selector 把每份文件當成 HTML 文件，現在有 `XMLDocument`；`shell` 的 `automation_pipe` 測試的「連結開的分頁」現在可能測得到。
- **跑過的檢查**：`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（1590 個通過）；`sanity.py docs`、`deps`、`boundary`、`unsafe` 通過；`test.py scenarios`（50 runs，0 個失敗）、`reftest`（542 個通過、67 個預期的失敗、0 個不符）、`smoke`、`wdspec`（1941 個結果都符合預期）、`automation`（25 份情境和 `pipe --window` 通過，`about:demo` 的截圖版面正常）。第 3 段只改了 `sanity.py` 和文件，沒有重跑 Rust 的檢查。
- **`test.py wpt`：1666 個裡 2 個不符，都和這份 RFC 無關**（這個分支沒有改到行為）：`css/cssom-view/screenLeftTop.html` 現在通過了，baseline 還是失敗（`main` 的 37d5373 加了 `screenLeft` / `screenTop`，沒有更新 baseline）；`html/browsers/browsing-the-web/history-traversal/001.html` 每次都 timeout（重跑三次都一樣，baseline 是一個 `[FAIL]`），原因還沒查，在 `main` 上就是這樣。
