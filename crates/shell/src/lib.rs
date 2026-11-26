//! The browser shell (`//chrome/browser/ui` + `content_shell`) as a library,
//! so its end-to-end tests can drive the pipeline headless in-process. The `machopo` binary (`main.rs`) adds the window and argument
//! parsing.

#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "tests"
    )
)]

pub mod automation;
mod automation_cookies;
pub mod automation_session;
pub mod browser_view;
pub mod browser_view_layout;
pub mod browser_window;
pub mod chrome;
pub mod devtools_http_handler;
pub mod devtools_pipe_handler;
pub mod headless;
pub mod keyboard;
pub mod mouse;
pub mod omnibox;
pub mod popup_menu;
pub mod sad_tab;
pub mod scenario;
pub mod shutdown_signals;
pub mod sidebar;
pub mod system_colors;
pub mod tab_strip;
pub mod tab_strip_model;
pub mod theme;
pub mod toolbar;
pub mod trace;
pub mod url_fixup;
pub mod windowless;

/// The renderer's script engines (its `ScriptEngineFactory`): V8, and
/// wasmtime for a document that declares its scripts wasm; V8 for a
/// dedicated worker's.
pub const SCRIPT_ENGINES: browser::ScriptEngineFactory = browser::ScriptEngineFactory {
    engine: dom_js::V8Engine::boxed,
    wasm_engine: Some(dom_wasm::WasmEngine::boxed),
    worker_engine: Some(dom_js::V8WorkerEngine::boxed),
};

/// What tells the executable it is to be a renderer process (Chromium's
/// `--type=renderer`): its first argument, so that it knows what it is
/// before it reads anything else.
pub const RENDERER_PROCESS_SWITCH: &str = "--type=renderer";

/// What the executable exits with as a renderer process that had no
/// channel to a browser (it was started by hand).
pub const NO_CHANNEL_EXIT: u8 = 2;

/// Is a renderer process until it ends (Chromium's `RendererMain`): the
/// end of the channel its browser started it with, and the renderer run
/// on it with this executable's script engines. What the process exits
/// with.
pub fn renderer_main() -> u8 {
    let Some(channel) = ipc::platform::inherited() else {
        eprintln!(
            "machopo: {RENDERER_PROCESS_SWITCH} is how machopo starts its own renderers: \
             there is no browser to render for"
        );
        return NO_CHANNEL_EXIT;
    };
    match browser::run_child_process(channel, SCRIPT_ENGINES) {
        browser::ChildExit::Ended => 0,
        browser::ChildExit::NotStarted => 3,
        browser::ChildExit::Crashed => 4,
    }
}

/// How the tabs' renderers are run: each in a process of its own when
/// `process_per_tab` (this executable again, as a renderer:
/// [`RENDERER_PROCESS_SWITCH`]), or as threads of this process.
pub fn renderer_launcher(process_per_tab: bool) -> browser::RendererLauncher {
    if !process_per_tab {
        return browser::RendererLauncher::InProcess(SCRIPT_ENGINES);
    }
    browser::RendererLauncher::Process(std::sync::Arc::new(|| {
        // The executable as it was started: one replaced on disk since is
        // another build, which may speak otherwise. A path that can't be
        // had starts nothing, and the tab says its renderer didn't start.
        let executable = std::env::current_exe().unwrap_or_default();
        let mut command = std::process::Command::new(executable);
        command.arg(RENDERER_PROCESS_SWITCH);
        // Its input is nothing, as a Chromium child's (`LaunchProcess`); its
        // output and errors are the browser's, as a Chromium child's are.
        // The pipe isn't among them: the browser took its ends
        // close-on-exec (not inheritable on Windows), so a client hears the
        // pipe end as the browser does, not once the last renderer has.
        command.stdin(std::process::Stdio::null());
        command
    }))
}

/// URLs pass through; anything else is a file path, which becomes a
/// `file://` URL (relative paths are resolved against the working
/// directory).
pub fn to_url(arg: &str) -> String {
    if arg.starts_with("about:") || arg.contains("://") {
        return arg.to_owned();
    }
    let path = std::path::Path::new(arg);
    let absolute = std::fs::canonicalize(path)
        .or_else(|_| std::env::current_dir().map(|dir| dir.join(path)))
        .unwrap_or_else(|_| path.to_owned());
    browser::loader::file_url(&absolute).unwrap_or_else(|| arg.to_owned())
}
