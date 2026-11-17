# 打斷 worker 的 script 的原型

不合併。「已決定的事項」第 12 條要先確認的事：從另一條 thread 打斷（V8 的 `TerminateExecution`）正在呼叫 DOM 的 script 之後，從 bindings 一路退出來不會 crash，engine 和 DOM 的 heap 放得掉。結果在 `details.md` 的「1b 的原型：打斷 script」。

- `prototype_terminate.rs`：`dom_js` 的整合測試的一個模組（`crates/dom_js/tests/it/`），9 種呼叫 DOM 的迴圈，每種在 6 個時間點從另一條 thread 打斷，各 3 次。
- `engine.diff`：給它用的 `V8Engine::prototype_terminator` 和 `main.rs` 的 `mod` 那一行。

重跑：把 `engine.diff` 套到 branch 上（`git apply`）、把 `prototype_terminate.rs` 放進 `crates/dom_js/tests/it/`，`cargo test -p dom_js --test it prototype_terminate -- --nocapture`。
