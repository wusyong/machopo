# RFC 0110 的細節：renderer 和 engine 的狀態只有一個擁有者

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 盤點的起點

第 0 階段要做完整的盤點。下面是寫草稿時（2026-11-23）已經知道的，照程式看的，沒有逐一對照 Blink。

### 為什麼會有副本

`RenderThread` 擁有 engine（`Box<dyn ScriptEngine>`），engine 擁有 renderer 交給它的 `Frame`（`set_layout`）和 `RendererLoadSink`（`set_load_sink`）。呼叫 engine 時借出的是 `&mut` engine，`RenderThread` 的其他欄位在這段期間借不到：engine 裡的 DOM API（經過 `FrameLayout`、`LoadSink` 的 hook）碰得到的只有 engine 擁有的那兩個物件。其他 renderer 的狀態只能在呼叫結束之後由 renderer 自己改，或用 `Rc`、`RefCell` 和 hook 共享。

### 已知的延後和副本

| 東西 | 在哪裡 | 怎麼延後或共享 | 0109 的哪一個 | 照 Blink 應該 |
| --- | --- | --- | --- | --- |
| frame 的建立、移除 | `FrameRequest::Created`、`Removed`；renderer 的 `ChildFrames` | 便條，外圈或收尾點處理；engine 的 `frames` 和 renderer 的 `children` 各一份 | A5 | 插入、移除的當下完成 |
| 子 frame 的導覽 | `FrameRequest::Navigate`、`Navigation` | 便條 | | 同一份文件內的當下 commit；跨文件的交給 Browser |
| 同一份文件內的導覽的捲動 | `frame_navigated` 的 `scroll_to_fragment` | 便條 | A4 | 當下捲動 |
| `fetch()` 在 DevTools 的登記 | `FrameRequest::FetchSent`、`CancelFetch`；Network agent | 請求當下由 `LoadSink` 送出，登記和取時間是便條 | A2、A3 | 送出的當下告訴 Network agent |
| blob URL 的登記和撤銷 | `FrameRequest::RegisterBlobUrl`、`RevokeBlobUrl`；`BlobUrls` | 便條；`BlobUrls` 是 `Rc<RefCell>`，和 `LoadSink` 共享 | | 還沒對照 |
| DevTools 的 frame 清單 | DOM agent；`tell_frames` | renderer 自己的副本，外圈才更新 | A1 | agent 直接讀 frame 的樹 |
| DOM 的改變告訴 DevTools | `tell_dom_changes` | 外圈 | | 改變的當下（`probe::`）；還沒對照 |
| intersection 的通知 | `run_intersection_task` | 外圈，不是真的 task | A6（推論） | 排成 task |
| 請求的編號、進行中的請求、文件的 loader | `RequestIds`、`DocumentLoaders`（`crates/renderer/src/loader.rs`） | `Rc<RefCell>`，和 `LoadSink` 共享 | | 一個擁有者 |
| DevTools 的 agent | `DevToolsQueueHandle`（`render_thread/devtools.rs`） | `Rc`：inspector 在 `RenderThread` 借不到時也要用 | | 還沒對照 |
| 下拉選單 | `handle_popup_requests` | 外圈 | | 還沒對照 |

`handle_frame_requests` 在 `render_thread.rs` 有 4 處、`render_thread/documents.rs` 有 15 處，另外在外圈一次。

### 已知要留的

- 跨 process 的：Browser 的 session history 和 renderer 記的 index、length；導覽交給 Browser 的訊息。Chromium 也是兩份、靠 mojo 的訊息。

## 0109 找到的

在 `0109` branch 上（`cb5930d`）：

- `details.md` 的 A1 到 A6 和「三種原因」：每一個測試壞在哪裡、原因有多確定。
- `progress.md` 的實驗：「每個 task 之後收尾」修得掉放大的測試；五個探針（設了 `location.hash` 之後，同一個 task、下一個 timer、同一次 rendering、idle callback 之後讀不讀得到新的捲動位置）Chromium 156 都是新的，machopo 和「每個 task 之後收尾」都是舊的，當下捲動的原型和 Chromium 一樣。
- `progress.md` 的最後一節：別的 origin 的 `javascript:` 網址、延後的導覽套到新文件的兩個試驗（沒有重現）。

## 參考資料

- Chromium 152.0.7977.82：`third_party/blink/renderer/core/inspector/inspector_css_agent.cc`（sheet 直接從 document 列）、`third_party/blink/renderer/core/loader/resource_load_observer_for_frame.cc`（送出的當下告訴 DevTools）、`third_party/blink/renderer/core/loader/frame_loader.cc`（`StartNavigation`、同一份文件內的導覽同步 commit）、`third_party/blink/renderer/core/frame/local_frame.cc`（`CanNavigate`）。
- machopo：`crates/dom_api/src/frame.rs`（`FrameRequest`）、`crates/dom_api/src/layout.rs`（`FrameLayout`、`LayoutCell`、`LentLayout`）、`crates/dom_api/src/engine.rs`（`LoadSink`、`ScriptEngine`）、`crates/renderer/src/render_thread.rs`（`run`）、`crates/renderer/src/render_thread/documents.rs`（`handle_frame_requests`、`RendererLoadSink`）、`crates/renderer/src/loader.rs`、`crates/bindings/src/isolate_state.rs`。
