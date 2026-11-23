# 第 0 階段的原型：`fetch()` 的登記

不合併（`README.md` 的已決定的事項第 3 條）。比較和結論在 `details.md` 的「原型」，測試的結果在 `progress.md`。

都從 `87fc238` 開始，照順序套：

```sh
git checkout --detach 87fc238
git apply --index rfcs/0110-renderer-engine-ownership/prototypes/tests.diff
git apply --index rfcs/0110-renderer-engine-ownership/prototypes/v0-shared.diff
# 再套其中一個（三個都以 V0 為底）：
git apply --index rfcs/0110-renderer-engine-ownership/prototypes/v1-sink-owns.diff
git apply --index rfcs/0110-renderer-engine-ownership/prototypes/v2-lent.diff
git apply --index rfcs/0110-renderer-engine-ownership/prototypes/v3-dom-owns.diff
# 換 engine 時 unload 裡的 fetch()，哪一個上面都可以套：
git apply rfcs/0110-renderer-engine-ownership/prototypes/tests-unload.diff
```

| patch | 內容 |
| --- | --- |
| `tests.diff` | A2、A3 的必壞測試（`devtools_network` 的兩個 `lab_` 測試：`fetch()` 之後 script 忙 200 ms，redirect 和回應先到） |
| `v0-shared.diff` | 不動擁有權：engine 送出 `fetch()` 時把 frame 的 token、文件的網址和 script 的 stack 交給 sink（`dom_api::FetchOrigin`），sink 當下登記 Network agent、記下 `fetches`；`fetches` 和其他的表一樣用 `Rc<RefCell>` 和 sink 共享；`FetchSent` 只留沒送出的那一種。`ScriptEngine::load_sink_mut` 和 `LoadSink::as_any_mut` 給 V1、V2 用 |
| `v1-sink-owns.diff` | 做法一：請求的編號和 `fetches`（`LoadState`）由 engine 的 sink 擁有，renderer 經過 `RenderThread::load_state` 取；沒有 sink 的時候（還沒有 engine、wasm 的 engine、換 engine 的期間）由 render thread 拿著 |
| `v2-lent.diff` | 做法二：`LoadState` 由 render thread 擁有，放在包著 engine 的 `EngineSlot`；取 engine（`as_mut`）時借進 sink，取狀態（`state_mut`）時拿回來。換 engine 時 unload 裡的 `fetch()` 會 panic（`tests-unload.diff`） |
| `v3-dom-owns.diff` | 做法三：`fetch()` 送出的請求記在 DOM（`Dom::note_fetch_sent`，dom_js 在 sink 送出之後記），renderer 的回應經過 engine 查；請求的編號是每個 renderer thread 一個計數器（`loader::next_request_id`）；renderer 自己送的請求的網址留在 renderer（`RequestUrls`） |
| `tests-unload.diff` | `wasm_documents` 的 `lab_` 測試：JS 的文件在 `pagehide`、`unload` 裡呼叫 `fetch()`，同時被 wasm 的文件換掉 |
