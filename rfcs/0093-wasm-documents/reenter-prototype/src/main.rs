//! Whether a host function may call an export of the component instance
//! that is calling it (a DOM member dispatching an event to the guest's
//! own listener), in a sync store. Prints what each case did.

use wasmtime::component::{Component, Instance, Linker, TypedFunc};
use wasmtime::{Config, Engine, Store, StoreContextMut};

/// The guest: `run(depth)` calls the host's `dispatch(depth)`, then returns
/// its counter; `listener(depth)` adds one to the counter, calls `dispatch`
/// again while `depth` is above 1, and traps at depth 99.
const GUEST: &str = r#"
(component
  (import "dispatch" (func $dispatch (param "depth" u32)))
  (core func $dispatch_lowered (canon lower (func $dispatch)))
  (core module $m
    (import "host" "dispatch" (func $dispatch (param i32)))
    (global $count (mut i32) (i32.const 0))
    (func (export "run") (param $depth i32) (result i32)
      (call $dispatch (local.get $depth))
      (global.get $count))
    (func (export "listener") (param $depth i32)
      (if (i32.eq (local.get $depth) (i32.const 99)) (then unreachable))
      (global.set $count (i32.add (global.get $count) (i32.const 1)))
      (if (i32.gt_u (local.get $depth) (i32.const 1))
        (then (call $dispatch (i32.sub (local.get $depth) (i32.const 1))))))
    (func (export "count") (result i32) (global.get $count)))
  (core instance $i (instantiate $m
    (with "host" (instance (export "dispatch" (func $dispatch_lowered))))))
  (func (export "run") (param "depth" u32) (result u32)
    (canon lift (core func $i "run")))
  (func (export "listener") (param "depth" u32)
    (canon lift (core func $i "listener")))
  (func (export "count") (result u32)
    (canon lift (core func $i "count"))))
"#;

#[derive(Default)]
struct Data {
    listener: Option<TypedFunc<(u32,), ()>>,
    /// What the host saw of each call back, outermost first.
    log: Vec<String>,
    /// Whether the host swallows a listener's trap (as a dispatch reports
    /// an exception and goes on) or passes it up.
    swallow: bool,
}

fn instantiate(engine: &Engine, swallow: bool) -> wasmtime::Result<(Store<Data>, Instance)> {
    let component = Component::new(engine, GUEST)?;
    let mut linker = Linker::<Data>::new(engine);
    linker.root().func_wrap(
        "dispatch",
        |mut cx: StoreContextMut<'_, Data>, (depth,): (u32,)| -> wasmtime::Result<()> {
            let listener = cx.data().listener.expect("set after instantiation");
            let result = listener.call(&mut cx, (depth,));
            let swallow = cx.data().swallow;
            match result {
                Ok(()) => {
                    cx.data_mut().log.push(format!("listener({depth}) returned"));
                    Ok(())
                }
                Err(error) => {
                    cx.data_mut().log.push(format!("listener({depth}) failed: {error}"));
                    if swallow { Ok(()) } else { Err(error) }
                }
            }
        },
    )?;
    let mut store = Store::new(engine, Data { swallow, ..Data::default() });
    let instance = linker.instantiate(&mut store, &component)?;
    let listener = instance.get_typed_func::<(u32,), ()>(&mut store, "listener")?;
    store.data_mut().listener = Some(listener);
    Ok((store, instance))
}

fn case(engine: &Engine, name: &str, depth: u32, swallow: bool) -> wasmtime::Result<()> {
    println!("== {name}");
    let (mut store, instance) = instantiate(engine, swallow)?;
    let run = instance.get_typed_func::<(u32,), (u32,)>(&mut store, "run")?;
    match run.call(&mut store, (depth,)) {
        Ok((count,)) => println!("run({depth}) returned count {count}"),
        Err(error) => println!("run({depth}) failed: {error}"),
    }
    for line in std::mem::take(&mut store.data_mut().log) {
        println!("  host: {line}");
    }
    // Whether the instance can still be entered after.
    let count = instance.get_typed_func::<(), (u32,)>(&mut store, "count")?;
    match count.call(&mut store, ()) {
        Ok((count,)) => println!("after: count() = {count}"),
        Err(error) => println!("after: count() failed: {error}"),
    }
    Ok(())
}

/// How the host's `dispatch` is defined, and how it calls the listener.
#[derive(Clone, Copy, Debug)]
enum Host {
    /// `func_wrap`, `TypedFunc::call`.
    Sync,
    /// `func_wrap_async`, `TypedFunc::call_async`.
    Async,
    /// `func_wrap_concurrent`, `TypedFunc::call_concurrent`.
    Concurrent,
}

/// How the embedder calls `run`.
#[derive(Clone, Copy, Debug)]
enum Entry {
    /// `TypedFunc::call`.
    Sync,
    /// `TypedFunc::call_async`.
    Async,
    /// `Store::run_concurrent` and `TypedFunc::call_concurrent`.
    Concurrent,
}

/// One combination, as `cargo run -- matrix <host> <entry> <depth>` (one a
/// process: some panic in wasmtime).
fn matrix(engine: &Engine, host: Host, entry: Entry, depth: u32) -> wasmtime::Result<()> {
    println!("== host {host:?}, entry {entry:?}, depth {depth}");
    // A concurrent host function only satisfies an import typed `async
    // func`; the guest lowers it without `async` all the same (it blocks).
    let guest = match host {
        Host::Concurrent => GUEST.replace(
            r#"(func $dispatch (param "depth" u32))"#,
            r#"(func $dispatch async (param "depth" u32))"#,
        ),
        _ => GUEST.to_owned(),
    };
    let component = Component::new(engine, &guest)?;
    let mut linker = Linker::<Data>::new(engine);
    match host {
        Host::Sync => linker.root().func_wrap(
            "dispatch",
            |mut cx: StoreContextMut<'_, Data>, (depth,): (u32,)| -> wasmtime::Result<()> {
                let listener = cx.data().listener.expect("set");
                listener.call(&mut cx, (depth,))?;
                cx.data_mut().log.push(format!("listener({depth}) returned"));
                Ok(())
            },
        )?,
        Host::Async => linker.root().func_wrap_async(
            "dispatch",
            |mut cx: StoreContextMut<'_, Data>, (depth,): (u32,)| {
                Box::new(async move {
                    let listener = cx.data().listener.expect("set");
                    listener.call_async(&mut cx, (depth,)).await?;
                    cx.data_mut().log.push(format!("listener({depth}) returned"));
                    Ok(())
                })
            },
        )?,
        Host::Concurrent => linker.root().func_wrap_concurrent(
            "dispatch",
            |accessor: &wasmtime::component::Accessor<Data>, (depth,): (u32,)| {
                Box::pin(async move {
                    let listener = accessor.with(|mut access| access.data_mut().listener.expect("set"));
                    listener.call_concurrent(accessor, (depth,)).await?;
                    accessor.with(|mut access| {
                        access.data_mut().log.push(format!("listener({depth}) returned"));
                    });
                    Ok(())
                })
            },
        )?,
    }
    let mut store = Store::new(engine, Data::default());
    let instance = futures::executor::block_on(linker.instantiate_async(&mut store, &component))?;
    let listener = instance.get_typed_func::<(u32,), ()>(&mut store, "listener")?;
    store.data_mut().listener = Some(listener);
    let run = instance.get_typed_func::<(u32,), (u32,)>(&mut store, "run")?;
    let result = match entry {
        Entry::Sync => run.call(&mut store, (depth,)),
        Entry::Async => futures::executor::block_on(run.call_async(&mut store, (depth,))),
        Entry::Concurrent => futures::executor::block_on(
            store.run_concurrent(async |accessor| run.call_concurrent(accessor, (depth,)).await),
        )
        .and_then(|inner| inner),
    };
    match result {
        Ok((count,)) => println!("run({depth}) returned count {count}"),
        Err(error) => println!("run({depth}) failed: {error:#}"),
    }
    for line in std::mem::take(&mut store.data_mut().log) {
        println!("  host: {line}");
    }
    Ok(())
}

/// Returns `Pending` once, waking itself: lets `run_concurrent` turn its
/// event loop while its closure isn't done.
async fn yield_now() {
    let mut yielded = false;
    std::future::poll_fn(|cx| {
        if std::mem::replace(&mut yielded, true) {
            std::task::Poll::Ready(())
        } else {
            cx.waker().wake_by_ref();
            std::task::Poll::Pending
        }
    })
    .await
}

/// `cargo run -- wait`: whether a guest that waits survives its
/// `run_concurrent` future being dropped (a store run only when there is
/// something for it to do, lent for one turn at a time). The host's
/// `dispatch` waits for a channel before it calls the listener.
fn wait(engine: &Engine) -> wasmtime::Result<()> {
    use futures::channel::oneshot;
    use std::sync::Mutex;
    println!("== a waiting guest, its run_concurrent dropped and made again");
    // `run` lifted `async` (stackful: it may block), returning through
    // `task.return`.
    let guest = GUEST
        .replace(
            r#"(func $dispatch (param "depth" u32))"#,
            r#"(func $dispatch async (param "depth" u32))"#,
        )
        .replace(
            r#"(core module $m"#,
            r#"(core func $task_return (canon task.return (result u32)))
  (core module $m
    (import "host" "task-return" (func $task_return (param i32)))"#,
        )
        .replace(
            r#"(func (export "count") (result i32) (global.get $count)))"#,
            r#"(func (export "count") (result i32) (global.get $count))
    (func (export "run-async") (param $depth i32)
      (call $dispatch (local.get $depth))
      (call $task_return (global.get $count))))"#,
        )
        .replace(
            r#"(with "host" (instance (export "dispatch" (func $dispatch_lowered))))"#,
            r#"(with "host" (instance (export "dispatch" (func $dispatch_lowered))
                                  (export "task-return" (func $task_return))))"#,
        )
        .replace(
            r#"(func (export "run") (param "depth" u32) (result u32)
    (canon lift (core func $i "run")))"#,
            r#"(func (export "run") async (param "depth" u32) (result u32)
    (canon lift (core func $i "run-async") async))"#,
        );
    let component = Component::new(engine, &guest)?;
    let (send, receive) = oneshot::channel::<()>();
    let receive = std::sync::Arc::new(Mutex::new(Some(receive)));
    let mut linker = Linker::<Data>::new(engine);
    linker.root().func_wrap_concurrent(
        "dispatch",
        move |accessor: &wasmtime::component::Accessor<Data>, (depth,): (u32,)| {
            let receive = receive.lock().expect("not poisoned").take();
            Box::pin(async move {
                accessor.with(|mut access| access.data_mut().log.push("dispatch waits".into()));
                if let Some(receive) = receive {
                    let _ = receive.await;
                }
                accessor.with(|mut access| access.data_mut().log.push("dispatch woke".into()));
                let listener = accessor.with(|mut access| access.data_mut().listener.expect("set"));
                listener.call_concurrent(accessor, (depth,)).await?;
                accessor.with(|mut access| access.data_mut().log.push("listener returned".into()));
                Ok(())
            })
        },
    )?;
    let mut store = Store::new(engine, Data::default());
    let instance = futures::executor::block_on(linker.instantiate_async(&mut store, &component))?;
    let listener = instance.get_typed_func::<(u32,), ()>(&mut store, "listener")?;
    store.data_mut().listener = Some(listener);
    let run = instance.get_typed_func::<(u32,), (u32,)>(&mut store, "run")?;
    let count = instance.get_typed_func::<(), (u32,)>(&mut store, "count")?;

    // Turn 1: start `run`, poll until nothing moves, drop the future.
    futures::executor::block_on(async {
        let mut turn = std::pin::pin!(store.run_concurrent(async |accessor| {
            run.call_concurrent(accessor, (1,)).await
        }));
        for _ in 0..5 {
            if let std::task::Poll::Ready(result) = futures::poll!(turn.as_mut()) {
                println!("turn 1 finished early: {:?}", result.map(|inner| inner.map_err(|e| format!("{e:#}"))).map_err(|e| format!("{e:#}")));
                return;
            }
        }
        println!("turn 1: pending, dropped");
    });
    println!("  log: {:?}", std::mem::take(&mut store.data_mut().log));

    // What the guest waits for arrives while no future of the store lives.
    let _ = send.send(());

    // Turn 2: a new `run_concurrent` with nothing of its own to do.
    let result = futures::executor::block_on(store.run_concurrent(async |_accessor| {
        for _ in 0..10 {
            yield_now().await;
        }
    }));
    println!("turn 2: {:?}", result.map_err(|e| format!("{e:#}")));
    println!("  log: {:?}", std::mem::take(&mut store.data_mut().log));
    match futures::executor::block_on(count.call_async(&mut store, ())) {
        Ok((count,)) => println!("after: count() = {count}"),
        Err(error) => println!("after: count() failed: {error:#}"),
    }
    Ok(())
}

/// `cargo run -- mixed <depth>`: as a page's store is, with an `async func`
/// import too (WASI's clock), which makes the store one that must be
/// entered with the `*_async` functions; the host's `dispatch` is a plain
/// `func_wrap` that calls the listener with `call`.
fn mixed(engine: &Engine, depth: u32) -> wasmtime::Result<()> {
    println!("== a store with an async import too, host Sync, entry Async, depth {depth}");
    let guest = GUEST
        .replace(
            r#"(import "dispatch" (func $dispatch (param "depth" u32)))"#,
            r#"(import "dispatch" (func $dispatch (param "depth" u32)))
  (import "tick" (func $tick async))
  (core func $tick_lowered (canon lower (func $tick)))"#,
        )
        .replace(
            r#"(import "host" "dispatch" (func $dispatch (param i32)))"#,
            r#"(import "host" "dispatch" (func $dispatch (param i32)))
    (import "host" "tick" (func $tick))"#,
        )
        .replace(
            r#"(func (export "run") (param $depth i32) (result i32)
      (call $dispatch (local.get $depth))"#,
            r#"(func (export "run") (param $depth i32) (result i32)
      (call $tick)
      (call $dispatch (local.get $depth))"#,
        )
        .replace(
            r#"(with "host" (instance (export "dispatch" (func $dispatch_lowered))))"#,
            r#"(with "host" (instance (export "dispatch" (func $dispatch_lowered))
                                  (export "tick" (func $tick_lowered))))"#,
        );
    let component = Component::new(engine, &guest)?;
    let mut linker = Linker::<Data>::new(engine);
    linker.root().func_wrap(
        "dispatch",
        |mut cx: StoreContextMut<'_, Data>, (depth,): (u32,)| -> wasmtime::Result<()> {
            let listener = cx.data().listener.expect("set");
            listener.call(&mut cx, (depth,))?;
            cx.data_mut().log.push(format!("listener({depth}) returned"));
            Ok(())
        },
    )?;
    linker.root().func_wrap_concurrent(
        "tick",
        |_: &wasmtime::component::Accessor<Data>, (): ()| Box::pin(async move { Ok(()) }),
    )?;
    let mut store = Store::new(engine, Data::default());
    let instance = futures::executor::block_on(linker.instantiate_async(&mut store, &component))?;
    let listener = instance.get_typed_func::<(u32,), ()>(&mut store, "listener")?;
    store.data_mut().listener = Some(listener);
    let run = instance.get_typed_func::<(u32,), (u32,)>(&mut store, "run")?;
    match futures::executor::block_on(run.call_async(&mut store, (depth,))) {
        Ok((count,)) => println!("run({depth}) returned count {count}"),
        Err(error) => println!("run({depth}) failed: {error:#}"),
    }
    for line in std::mem::take(&mut store.data_mut().log) {
        println!("  host: {line}");
    }
    Ok(())
}

fn main() -> wasmtime::Result<()> {
    let mut config = Config::new();
    // For `wait`'s guest only: its export blocks on its own stack.
    config.wasm_component_model_async_stackful(true);
    let engine = Engine::new(&config)?;
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("mixed") {
        return mixed(&engine, args[2].parse().expect("a depth"));
    }
    if args.get(1).map(String::as_str) == Some("wait") {
        return wait(&engine);
    }
    if args.get(1).map(String::as_str) == Some("matrix") {
        let host = match args[2].as_str() {
            "sync" => Host::Sync,
            "async" => Host::Async,
            _ => Host::Concurrent,
        };
        let entry = match args[3].as_str() {
            "sync" => Entry::Sync,
            "async" => Entry::Async,
            _ => Entry::Concurrent,
        };
        return matrix(&engine, host, entry, args[4].parse().expect("a depth"));
    }
    case(&engine, "one level: run -> dispatch -> listener", 1, false)?;
    case(&engine, "three levels nested", 3, false)?;
    case(&engine, "the listener traps, the host passes it up", 99, false)?;
    // A host function that swallows the trap and returns `Ok` panics in
    // wasmtime ("BUG: current thread is not a host thread"): a trap has to
    // go all the way up. Run with `swallow` to see it.
    if std::env::args().any(|arg| arg == "swallow") {
        case(&engine, "the listener traps, the host swallows it", 99, true)?;
    }
    Ok(())
}
