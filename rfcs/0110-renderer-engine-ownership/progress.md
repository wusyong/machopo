# RFC 0110 的進度：renderer 和 engine 的狀態只有一個擁有者

開始日期：2026-11-23

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0a. 盤點：副本、便條、外圈的收尾、共享的 `Rc`、`RefCell`，對照 Blink 和 content 層 | 做完，待審查（標「沒對」的幾項沒對原始碼） | 2026-11-23 |
| 0b. 原型：`fetch()` 的登記，做法一 | 做完，待審查 | 2026-11-23 |
| 0c. 原型：`fetch()` 的登記，做法二 | 做完，待審查；換 engine 時會 panic | 2026-11-23 |
| 0c'. 獨立的審查；原型：`fetch()` 的登記，做法三 | 做完，待審查 | 2026-11-23 |
| 0d. 交給使用者決定做法和範圍 | 做完：做法第 5 條，DevTools agent 第 6 條，範圍第 7 條 | 2026-11-23 |
| 1. 請求和 loader 交給 `dom`（已決定的事項第 7 條的第 1 步，第 10 條） | 做完，待審查 | 2026-11-23 |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 盤點 | `details.md` 的表；不改程式，所以沒有測試 |
| 原型 | 各自的 patch 在 `prototypes/`；A2、A3 的放大測試必過，比較改動、借用、`unsafe` |

## 做過的檢查

### 第 0 階段（2026-11-23，Linux：Fedora 45、debug build，`87fc238` 的複本）

**盤點**：照程式讀；Blink 對了 152.0.7977.82 的十一個檔（`details.md` 的「參考資料」），標「沒對」的沒有對。

**原型**（patch 在 `prototypes/`；編譯用 `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`，`MACHOPO_GPU_ADAPTER=cpu`）：

| | A2 的必壞測試 | A3 的必壞測試 | `devtools_network` | `renderer` | `shell` 整組 | `browser` | `dom_js` 整組 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 沒改（`tests.diff` 而已） | 3 次壞 3 次 | 3 次壞 3 次 | | | | | |
| V0 | 5 次壞 0 次 | 5 次壞 0 次 | 12 個通過 | 128 個通過 | 643 個通過 | 191 個通過 | 550 個通過，1 個 ignored |
| V1 | 5 次壞 0 次 | 5 次壞 0 次 | 12 個通過 | 128 個通過 | 643 個通過 | 191 個通過 | （和 V0 相同的 `dom_js`） |
| V2 | 5 次壞 0 次 | 5 次壞 0 次 | 12 個通過 | 128 個通過 | 643 個通過 | 191 個通過 | （和 V0 相同的 `dom_js`） |
| V3 | 5 次壞 0 次 | 5 次壞 0 次 | （在 `shell` 整組裡） | 128 個通過 | 643 個通過 | 191 個通過 | 550 個通過，1 個 ignored；`dom` 254 個通過 |

**換 engine 時 unload 裡的 `fetch()`**（`tests-unload.diff`，審查之後加的）：沒改的、V0、V1 各 3 次都通過；V2 3 次都在 sink 的 `expect`（`documents.rs:5114`）panic，panic 在不能 unwind 的地方，測試的 process 中止；V3 5 次都通過。V1 的請求編號從 1 重來，這個測試看不出來（讀程式確認）。

- `shell` 整組包括 `wasm_documents`：V1 裡 wasm 的 engine 沒有 sink，狀態留在 render thread 的那條路有跑到。
- 原型不是要合併的程式，所以沒有 mutation test，也沒有跑 clippy 和 WPT；效能沒有量。
- patch 照順序套回 `87fc238` 之後，和複本裡的 branch 一樣（V2 的新檔要 `--index`）。
- 做法二的第一版（有 `Drop` 的 guard）沒有編過，見 `details.md` 的「原型」；沒有留它的 patch。
- **審查**：獨立的 subagent 唯讀地審了盤點和三個原型（`details.md` 的「審查」）。它的主要說法我對過程式；V2 的 panic 用上面的測試重現了。
- **第二次審查**：另一個 subagent 唯讀地審了 V3、做法三用在 frame 的樹上和範圍「乙」（`details.md` 的「第二次審查」），沒有執行任何東西；它的關鍵說法我對過程式。

### 第 1 步：請求和 loader 交給 `dom`（2026-11-23，Linux：Fedora 45、debug build，`9e7d93d` 之上）

照原型 V3（`prototypes/v3-dom-owns.diff`）做，另外：拿掉 V0 給 V1、V2 用的 `ScriptEngine::load_sink_mut` 和 `LoadSink::as_any_mut`；拿掉 `FrameRequest` 的 `Fetch`、`FetchSent`、`CancelFetch`（已決定的事項第 10 條），送不出去的 `fetch()`（撤銷了的 blob URL）改由 sink 排一個 `MainThreadTask::FetchFailed`；取消是 `LoadSink::cancel_fetch`；文件的 loader 和帶來它的導覽記在 DOM（`Cx::document_loader`，子 frame 初始的 `about:blank` 走它的 creator 的），cookie 和 worker 的 hook 由 engine 帶 loader，renderer 的 `DocumentLoaders` 和 `FrameDocument.loader_id` 拿掉；頂層的文件換掉時（同一個 engine，和換 engine）取消它們還在路上的 `fetch()`。`sanity.py boundary` 的 `STATICS_ALLOWED` 加了每個 thread 的請求編號。

**新的測試**：`shell` 的 `devtools_network::a_fetch_is_told_as_it_is_sent_before_its_redirect`（A2）、`a_fetch_is_stamped_as_it_is_sent`（A3）、`a_replaced_documents_fetches_are_cancelled`（同一個 engine 和換 engine）、`documents::a_new_frames_fetch_goes_through_its_creators_loader`、`wasm_documents::a_js_documents_unload_may_fetch_as_a_wasm_document_replaces_it`；`dom_js` 的 `fetch.rs` 改用測試的 load sink（`Sent`、`Sink`），`tasks.rs`、`devtools.rs` 跟著改。新的四個 `shell` 測試各跑 3 次都通過。

**一個量不出東西的測試**（沒有留下）：原本用 TestServer 看 `fetch()` 的請求有沒有被放棄來驗證取消，但 `run`、`render` 回來之後 machopo 就結束、關掉每一條連線，沒有取消照樣被記成放棄（拿掉取消時兩個測試都還是通過）。改成 DevTools 的 `loadingFailed`（`canceled`）：拿掉任一條路的取消，請求都照樣完成（`loadingFinished`），測試失敗。這也確認了「頂層的文件換掉時不取消」是原本就有的問題（`docs/limitations.md` 那一條刪了）。

**Mutation test**（`mutants-1.toml`）：16 條都抓到。`a cookie set through no loader` 第一次只跑 `shell`，沒抓到；寫入再讀回的測試在 `browser` 的 `cookies::document_cookie_and_cookie_enabled`，spec 改成跑它之後抓到。跑完檔案都還原了（`target/mutate/originals` 是空的）。

**測試**：`dom` 254 個、`dom_api` 201 個、`dom_js` 550 個（1 個 ignored）、`renderer` 128 個、`browser` 191 個、`shell` 649 個都通過（加上新的測試之後 `shell` 是 650 個，新的那一個另外跑過）。WPT：`fetch/api/basic/`、`abort/`、`redirect/`、`cors/`、`cookies/`、`document-cookie.html`，3742 項，非預期的 0 個。`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs 通過。`cargo test --workspace`、`cargo fmt --check`、clippy 照規定在整個階段做完時跑。

