<!-- 給 V8（issues.chromium.org，V8 的 GC component）的 issue 草稿（README 的「已決定的事項」第 7 條）：2026-10-06 寫好，還沒送出，送出前要經過審核。照上游的語言寫成英文。 -->

# cppgc: a standalone `cppgc::Heap` posts only the first incremental marking task of a GC

**Version:** V8 15.2.124 (the same code is on `main` as of 2026-10-06), `src/heap/cppgc-internal/marker.cc`.

## What happens

A standalone `cppgc::Heap` (not a `v8::CppHeap`) created with `MarkingType::kIncremental` and a platform whose foreground task runner supports non-nestable tasks starts incremental marking from `HeapGrowing` when allocation passes `limit_for_incremental_gc`. `MarkerBase::StartMarking` calls `ScheduleIncrementalMarkingTask`, which posts an `IncrementalMarkingTask` and keeps its handle in `incremental_marking_handle_`.

When that task runs, `IncrementalMarkingTask::Run` performs one step and, if marking is done, finalizes the GC. It never cancels or resets its handle. If the step doesn't finish marking, `AdvanceMarkingWithLimitsEpilogue` calls `ScheduleIncrementalMarkingTask` again, but `incremental_marking_handle_` is still non-empty and not cancelled, so it returns without posting. The same happens when marking finishes on allocation (`AdvanceMarkingOnAllocationImpl`). The handle is only cancelled in `EnterAtomicPause`.

So each GC gets exactly one marking task. A GC whose first step (64 KB by `IncrementalMarkingSchedule`, or 2 ms) doesn't finish marking stays in the marking phase, write barriers on and garbage kept, until allocation passes `limit_for_atomic_gc` and `GCInvoker` posts an atomic GC task. An application that goes idle after starting a GC never finishes it.

A `CppHeap` doesn't run into this: V8's `IncrementalMarkingJob` drives its marking (`CppHeap::AdvanceMarking`), and that job clears its pending flag when its task runs and reschedules after each step. As far as we can tell, nothing in V8's tests runs a standalone heap's marking tasks to completion (the cppgc unit tests step the marker directly).

## How to reproduce

1. Create a `cppgc::Heap` with `StackSupport::kNoConservativeStackScan`, `MarkingType::kIncremental`, and a `cppgc::Platform` whose `GetForegroundTaskRunner()` returns a runner that queues tasks (non-nestable ones included) for the test to run.
2. Allocate a live object graph of a few hundred kilobytes, held by `Persistent`s.
3. Allocate small objects one at a time until the heap posts a task (incremental marking has started). Stop allocating.
4. Run posted tasks until the queue is empty: one `IncrementalMarkingTask` runs, marks 64 KB, and posts nothing. `cppgc::subtle::HeapState::IsMarking` stays true.

(Before the heap's first GC, `limit_for_incremental_gc` is only 64 KB below `limit_for_atomic_gc`, and new pages are reported 128 KB at a time, so the first GC often starts atomic and finishes in one task; a later GC, after the limits are set by allocation rate, reliably starts incremental marking.)

## Suggested fix

Let a running `IncrementalMarkingTask` clear the marker's handle before it steps, so that the epilogue can post the next step, the way `IncrementalMarkingJob` does; for example, cancel the task's own handle at the start of `Run()` after the `IsCanceled()` check (the marker's copy then reads as inactive). `EnterAtomicPause` already tolerates a cancelled handle (`CancelIfNonEmpty`), and `LeaveAtomicPause` only checks that it is inactive.

## Workaround

The embedder posts the following steps itself: after running the heap's tasks, while `HeapState::IsMarking` is true, it posts a task that calls `StandaloneTestingHeap::PerformMarkingStep(kNoHeapPointers)` and, once that returns true, finalizes the GC with the heap's sweeping type (`internal::Heap::FinalizeIncrementalGarbageCollectionIfRunning`, since the public API can only finalize with atomic sweeping).
