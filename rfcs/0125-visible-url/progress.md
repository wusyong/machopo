# RFC 0125 的進度：網址列只顯示已經 commit 的網址（頁面發起的導覽）

開始日期：2026-11-27。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、盤點、用臨時的測試確認偽造（`details.md` 的「確認」）、對照 Chromium 和 chromedriver、已決定的事項第 1 到 3 條 | 做完 | 2026-11-27 |
| 1. 最後 commit 的和顯示的網址；用到的地方照第 2 條；測試、mutation test、文件 | 還沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |

做到時補上。

## 做過的檢查

### 第 0 步（2026-11-27，Linux：Fedora 45，x86-64）

- 臨時的測試（加在 `crates/browser/tests/it/navigation.rs`，試完還原）：頁面每 50 ms 執行 `location = '/victim'`、`/victim` 延遲 5 秒回應，3 秒內 `WebContents::url()` 一直是 `/victim`，frame 樹的 root 一直是頁面 `/`、標題一直是它的，沒有 commit。
- 沒有改程式。`python3 scripts/sanity.py docs` 通過。
