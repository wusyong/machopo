<!-- 給 wasmtime 的 issue 草稿（README 的「已決定的事項」第 41 條）：2026-10-05 寫好，還沒送出，送出前要經過審核。照上游的語言寫成英文。 -->

# Dropping a `run_concurrent` future after a guest's `YIELD` loses the guest's next call

**Version:** wasmtime 49.0.2 (component-model-async).

## What happens

An embedder runs a store's event loop on demand: it polls `StoreContextMut::run_concurrent(async |_| pending().await)` until it returns `Pending`, then drops the future (the store has to be released, e.g. put back in a table) and polls a new one when one of its wakers fires.

If a guest task returns `CALLBACK_CODE_YIELD`, wasmtime queues a low-priority `WorkItem::GuestCall { DeliverEvent }` for it. In `poll_until`, that item is taken off the queue, then `yield_now().await` runs before `handle_work_item`. `yield_now` wakes the waker and returns `Pending`, so `run_concurrent` returns `Pending` while the item is held by the local `Dispose` guard.

If the future is dropped at that point, `Dispose::drop` handles only `WorkItem::ResumeFiber` and `WorkItem::PushFuture`. The `GuestCall` falls into `_ => {}` and is lost. The guest task is never called back: no event is pending for it and nothing is queued, so it hangs forever.

## How we hit it

A Rust guest (wit-bindgen 0.62) with an async export that awaits an async-lowered import (`wasi:clocks/monotonic-clock.wait-for`). When the host future completes, the guest's callback returns `YIELD` (wit-bindgen does this when its waker was signaled), and the embedder's next poll loses the work as above. `wit_bindgen::yield_async().await` triggers it directly.

## Workaround

We keep polling the same future while our waker was woken during the poll, and drop it only when a poll returns `Pending` without a self-wake.

## Suggestion

Either:
- re-queue a held `GuestCall` (at its priority) in `Dispose::drop`; or
- document that a `run_concurrent` future must not be dropped while it is pending after waking its own waker.

<!-- Minimal repro to attach: a component exporting `run: async func()` whose body is `yield_async().await` and then sets a flag the host can read. The host polls `run_concurrent` with a waker that only records the wake, drops the future after each `Pending`, and repeats. The flag is never set. -->
