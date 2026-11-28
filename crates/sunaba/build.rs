//! On Windows, the probe loads `user32.dll` when it is first called, not
//! as the process starts (the linker's `/DELAYLOAD`), as any program that
//! links the crate and is started under a policy is to: the crate calls it to make the window station and
//! desktop it starts a process under a policy on, and a process with it
//! loaded at its start doesn't start with win32k's calls turned off.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let target = |name: &str| std::env::var(format!("CARGO_CFG_TARGET_{name}")).unwrap_or_default();
    if target("OS") == "windows" && target("ENV") == "msvc" {
        println!("cargo:rustc-link-arg-bin=sunaba_probe=/DELAYLOAD:user32.dll");
        // The stub that loads it when it is first called.
        println!("cargo:rustc-link-arg-bin=sunaba_probe=delayimp.lib");
    }
}
