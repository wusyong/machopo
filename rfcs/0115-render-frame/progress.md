# RFC 0115 的進度：每個 frame 的狀態照 content 和 Blink 的界線各有一個擁有者

開始日期：2026-11-25

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. renderer 那一側收成 `RenderFrame`（已決定的事項第 2 到 8 條） | 沒開始 | |
| 1a. `render_frame.rs`：`RenderFrames`、`RenderFrame`、`NavigationRequest`，`FrameDocument` 搬進來；最上層的 `document` 和 `ChildFrames::documents` 改成表上的 `document`（第 2、3、4 條） | 沒開始 | |
| 1b. 導覽的帳（`ChildFrames::navigations`、`main_origin_source`）、`owner_loads`、`devtools_main_frame_token` 搬進 `RenderFrame`，每種轉換一個方法（第 3、5 條）；frame 不見時一起拿掉（第 8 條） | 沒開始 | |
| 1c. debug build 在讀表時檢查沒分派的 `FrameRequest`（第 6 條） | 沒開始 | |
| 2 到 4. 方向（已決定的事項第 1 條）：第 1 段做完之後重新評估 | — | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |

## 做過的檢查

（第 1 段開始之後記。）
