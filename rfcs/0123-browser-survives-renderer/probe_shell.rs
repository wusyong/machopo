// Stage 0's probe of what the browser does after the Viz thread panics:
// included at the end of `crates/shell/tests/it/automation_pipe.rs`
// (`include!("../../../../rfcs/0123-browser-survives-renderer/probe_shell.rs");`),
// with `crates/viz/src/thread.rs` panicking on a renderer's frame under
// `MACHOPO_PROBE_VIZ_PANIC`. Not part of the build.

/// What comes back for `id` within `wait`, the events before it dropped.
fn probe_answer(browser: &mut Browser, id: i32, wait: Duration) -> String {
    let until = std::time::Instant::now() + wait;
    loop {
        let left = until.saturating_duration_since(std::time::Instant::now());
        match browser.messages.recv_timeout(left) {
            Ok(Message::Response(response)) if response.id == id => {
                return format!("{:?}", response.result).chars().take(200).collect();
            }
            Ok(_) => {}
            Err(e) => return format!("nothing ({e:?})"),
        }
    }
}

#[test]
fn probe_viz_panics() {
    let server = server(&[(
        "/",
        Route::ok("text/html", "<!DOCTYPE html><p style='color:blue'>a page"),
    )]);
    let mut browser = Browser::start_in(
        &[],
        &[("MACHOPO_PROBE_VIZ_PANIC", std::ffi::OsStr::new("1"))],
        Stdio::inherit(),
    );
    let session = browser.attach();
    let loader = browser.start_navigation(&session, &server.url("/"));
    browser.wait_for(|event| match &event.event {
        EventKind::LifecycleEvent { loader_id, name, .. } if loader_id == &loader && name == "load" => Some(()),
        _ => None,
    });
    eprintln!("PROBE load heard");
    std::thread::sleep(Duration::from_secs(1));
    eprintln!("PROBE alive after 1 s: {:?}", browser.child.try_wait().unwrap());
    let id = browser.send(None, "Machopo.waitForLoad", json!({}));
    eprintln!("PROBE waitForLoad: {}", probe_answer(&mut browser, id, Duration::from_secs(10)));
    let id = browser.send(None, "Target.getTargets", json!({}));
    eprintln!("PROBE getTargets: {}", probe_answer(&mut browser, id, Duration::from_secs(10)));
    let id = browser.send(None, "Machopo.captureWindow", json!({}));
    eprintln!("PROBE captureWindow: {}", probe_answer(&mut browser, id, Duration::from_secs(10)));
    let id = browser.send(Some(&session), "Runtime.evaluate", json!({ "expression": "1 + 1" }));
    eprintln!("PROBE evaluate in the page: {}", probe_answer(&mut browser, id, Duration::from_secs(10)));
    eprintln!("PROBE alive: {:?}", browser.child.try_wait().unwrap());
    drop(browser.input.take());
    let until = std::time::Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = browser.child.try_wait().unwrap() {
            break Some(status);
        }
        if std::time::Instant::now() > until {
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    eprintln!("PROBE after its input closes: {status:?}");
}
