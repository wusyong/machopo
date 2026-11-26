# RFC 0116 的進度：automation pipe 改用 fd 3、4 和 Chrome 的開關

開始日期：2026-11-26

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. `ipc` 的兩個入口：`inherited_pipe`、`spawn_with_pipe`（Unix、Windows）和它們的測試（已決定的事項第 4 到 8 條） | 沒開始 | |
| 2. 換過去：`shell` 的開關（第 1 條）、reader 和 writer、renderer 的 stdout、所有啟動 machopo 的程式、文件和限制 | 沒開始 | |
| 3. Windows：在使用者的裝置上跑第 1、2 段的測試和 bench | 沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |

## 做過的檢查

（第 1 段開始之後記。）
