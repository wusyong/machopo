//! The browser shell (`//chrome/browser/ui` + `content_shell`) as a library,
//! so its end-to-end tests can drive headless mode in-process. The `machopo` binary (`main.rs`) adds the window and argument
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
pub mod automation_pipe;
pub mod automation_session;
pub mod browser_view;
pub mod browser_view_layout;
pub mod browser_window;
pub mod devtools_http_handler;
pub mod headless;
pub mod keyboard;
pub mod mouse;
pub mod omnibox;
pub mod popup_menu;
pub mod sad_tab;
pub mod scenario;
pub mod system_colors;
pub mod tab_strip;
pub mod tab_strip_model;
pub mod toolbar;
pub mod trace;
pub mod ui_text;
pub mod url_fixup;
pub mod windowless;

/// The renderer's script engine (its `ScriptEngineFactory`): V8.
pub const SCRIPT_ENGINES: browser::ScriptEngineFactory = browser::ScriptEngineFactory {
    engine: dom_js::V8Engine::boxed,
};

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
