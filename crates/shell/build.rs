//! On Windows, machopo's executable loads the DLLs only its browser side
//! calls (windows, drawing, the shell, the network) when one of them is
//! first called, not as the process starts (the linker's `/DELAYLOAD`;
//! Chromium delay-loads `user32.dll` and the like the same way). A
//! renderer process calls none of them, so it never has `user32.dll` or
//! `gdi32.dll`, and can be started with the calls into win32k they make
//! as they load turned off: a process with them loaded at its start
//! doesn't start that way.

/// What the executable links that a renderer process doesn't call.
/// `dwrite.dll` a renderer does call, for the system's fonts: it is
/// loaded then, and brings in neither `user32.dll` nor `gdi32.dll`.
const DELAY_LOADED: &[&str] = &[
    "user32", "gdi32", "imm32", "ole32", "dwmapi", "uxtheme", "shell32", "oleaut32", "setupapi",
    "dxgi", "opengl32", "winmm", "dbghelp", "crypt32", "ws2_32", "userenv", "dwrite",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let target = |name: &str| std::env::var(format!("CARGO_CFG_TARGET_{name}")).unwrap_or_default();
    if target("OS") == "windows" && target("ENV") == "msvc" {
        for dll in DELAY_LOADED {
            println!("cargo:rustc-link-arg-bin=machopo=/DELAYLOAD:{dll}.dll");
        }
        // The stub that loads one when it is first called.
        println!("cargo:rustc-link-arg-bin=machopo=delayimp.lib");
    }
}
