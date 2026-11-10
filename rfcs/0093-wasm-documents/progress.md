# RFC 0093 的進度：wasm 文件接回 renderer：用 `dom_wasm` 做 TodoMVC

開始日期：2026-11-10

下面的「決定 N」是 `README.md` 的「已決定的事項」第 N 條。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a guest 和 `dom_wasm` 的測試換成 WASI 0.3 和 async 的 Store（決定 5、6） | 完成 | 2026-11-10 |
| 1b WASI 0.3 的 linker：自己的時鐘，stdout 和 stderr 印到 console（決定 7） | 還沒開始 | |
| 1c `dom_wasm` 的 engine：`ScriptEngine`、`ScriptHost`、需要時才跑的 event loop、`start(document)`（決定 3、4、8、9） | 還沒開始 | |
| 1d renderer：prescan `<meta>`、選 engine、換頁時換 engine、載入 `application/wasm` 的 script（決定 2） | 還沒開始 | |
| 1e 靜態的 TodoMVC 的 guest、情境、renderer 的測試 | 還沒開始 | |
| 2 節點以外的物件的 handle、放開凍結、TodoMVC 用到的成員 | 還沒開始 | |
| 3 事件和 callback | 還沒開始 | |
| 4 TodoMVC 的 guest、情境 | 還沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 開始前（2026-11-10，macOS，Apple Silicon）

- 重入和等待的原型（`reenter-prototype/`，結果在 `details.md` 的「重入的原型」）：手寫的 WAT component，wasmtime 的 fork 50.0.1，沒有經過 `StoreSet` 和 elise 的 collector。
- `wit-bindgen` 0.62.0 的 C binding 對 `async func` 產生的 header 看過（`details.md` 的「C 的 guest 和 WASI 0.3 的工具鏈」）。

### 1a（2026-11-10，macOS，Apple Silicon）

- wasi-sdk 34.0（macOS arm64，release 的 sha256 核對過）放在 `../third_party/wasi-sdk-34.0`，`wit-bindgen` 0.62.0。
- `scripts/ci.py` 的 guest 的 target 從 `wasm32-wasip2` 改成 `wasm32-wasip3`，`python3 scripts/ci.py guests` 重編 `crates/dom_wasm/tests/guests/dom.wasm`（原始碼 `dom.c` 沒有改；之前那份是 Linux 上編的）。
- 測試的 harness（`crates/dom_wasm/tests/it/guest.rs`）：linker 換成 wasmtime-wasi 的 p3（只要 p3，不用再加 p2），`instantiate_async`、`call_async`，在 `StoreSet` 的一次借出裡 poll 到完成（`block_on`：不靠 waker、不用 executor，一百萬次 poll 還沒完成就失敗）。
- 分兩步確認，各跑一次 `cargo test -p dom_wasm`，都全部通過：先只改 harness（guest 還是 WASI 0.2 的）、再換 guest 和 linker。`shared` 的 GC 的 case 也通過：guest 的呼叫現在在 fiber 上跑，GC 經過 Store、`StoreSet` 交出別的 Store 的 root（開始前的原型沒涵蓋的那一塊）。
- mutation test：沒有跑。這一步沒有改實作（`crates/dom_wasm/src` 沒動），改的是測試的 harness、編 guest 的 target 和編好的 guest。
- 還沒確認的：guest 沒有印任何東西，wasmtime-wasi 的 stdout、stderr 在沒有 tokio 的 runtime 時能不能用要到 1b 才知道。
- `sanity.py` 的 `tests`、`unsafe`、`numerics`、`docs` 通過。
