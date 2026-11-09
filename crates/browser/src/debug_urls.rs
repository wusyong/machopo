//! URLs that do something instead of loading a page
//! (`//content/browser/renderer_host/debug_urls.cc`): typed in the address
//! field, they act in the browser and nothing is navigated.

/// What a debug URL does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DebugUrl {
    /// `about:gpucrash`: loses the GPU device, as a driver reset would
    /// (`chrome://gpucrash`, which crashes the GPU process).
    GpuCrash,
    /// `about:crash`: ends the tab's renderer at once, which shows its sad
    /// tab (like `chrome://kill`; `chrome://crash` has the renderer crash
    /// itself, which a renderer thread unwinding through V8 can't).
    Crash,
}

/// `IsDebugURL`: what `url` does, if it is a debug URL.
pub(crate) fn debug_url(url: &str) -> Option<DebugUrl> {
    match url {
        "about:gpucrash" => Some(DebugUrl::GpuCrash),
        "about:crash" => Some(DebugUrl::Crash),
        _ => None,
    }
}
