# RFC 0124 的進度：跨網站的導覽換 renderer

開始日期：2026-11-27。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、盤點（`details.md` 的「盤點」「Chromium」）、量 renderer 啟動的時間、已決定的事項第 1 到 12 條 | 做完 | 2026-11-27 |
| 1. 鋪路，行為不變：commit 時才定送給哪個 renderer；「分頁換了 renderer」是明確的一件事，DevTools 的 session 和 `headless` 聽它 | 還沒開始 | |
| 2. DevTools 的 session 的狀態跨過換 renderer | 還沒開始 | |
| 3. 規則和換：跨網站的 commit 在新的 renderer；舊文件的 `unload`、逾時；thread 的 renderer 一樣換 | 還沒開始 | |
| 4. 收尾：量測工具、WPT、`--window` 的情境、文件、整個 workspace 的測試 | 還沒開始 | |

在別的機器上要做的：macOS 和 Windows 上量 renderer process 啟動的時間（已決定的事項第 5 條），明顯比這台 Linux 的 11 ms 慢時重新考慮提前啟動和預備的 renderer。

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |

各段做到時補上。

## 做過的檢查

### 第 0 步（2026-11-27，Linux：Fedora 45，x86-64，AMD Radeon 890M）

- 盤點讀的是 `main` 的 `acdd0704`；Chromium 的原始碼是當天的 `main`，由另一個 agent 抓下來讀、整理，關鍵的引文（`kUnloadTimeout`、`GetFrameHostForNavigation` 叫兩次、初始的 `SiteInstance`、paint holding 的條件、`file:` 的 site、WebUI 的規則、`about:` 改寫成 `chrome://`、`--single-process`）對著抓下來的檔案抽查過。
- renderer 啟動的時間：release、sandbox 開著，新分頁到跑出第一句 script 的中位數 12.3、12.4 ms（兩輪各 30 次），已有 renderer 的分頁導覽 1.3、1.4 ms（`details.md` 的「量的：啟動一個 renderer process 要多久」）。
- 沒有改程式。`python3 scripts/sanity.py docs` 通過。
