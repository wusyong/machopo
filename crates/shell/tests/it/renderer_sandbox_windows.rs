//! The renderer process's sandbox on Windows: a real renderer is in it by
//! the system's account and shows a page as one outside does, and what it
//! refuses is refused, tried from inside by the executable itself
//! (`--type=renderer-sandbox-probe`: it starts itself as its browser
//! starts a renderer, and that process lowers itself as a renderer does,
//! then tries one thing).

use std::os::windows::io::AsHandle as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sunaba::windows::{LOW, Mitigation, UNTRUSTED, in_app_container, integrity, mitigation, open};

use crate::automation_pipe::{Browser, renderer_pids, script};
use crate::renderer_process::{MACHOPO, RunningRenderer};

/// The flags of mitigation `which` of the process `id`.
fn flags(id: u32, which: Mitigation) -> u32 {
    mitigation(open(id).unwrap().as_handle(), which).unwrap()
}

#[test]
fn a_renderer_process_is_in_the_sandbox() {
    let renderer = RunningRenderer::start(&["--test-fonts"]);
    let id = renderer.child.id();
    let process = open(id).unwrap();
    assert!(in_app_container(process.as_handle()).unwrap());
    // It has lowered itself by the time its threads are started.
    assert_eq!(integrity(process.as_handle()).unwrap(), UNTRUSTED);
    // No win32k, no file system control codes; no extension points; only
    // Microsoft's DLLs; only the system's fonts; no remote or
    // low-integrity images; no child processes; branch prediction kept to
    // itself; invalid handles raise.
    assert_eq!(flags(id, Mitigation::SystemCallDisable) & 0b101, 0b101);
    assert_eq!(flags(id, Mitigation::ExtensionPointDisable) & 1, 1);
    assert_eq!(flags(id, Mitigation::Signature) & 1, 1);
    assert_eq!(flags(id, Mitigation::FontDisable) & 1, 1);
    assert_eq!(flags(id, Mitigation::ImageLoad) & 0b11, 0b11);
    assert_eq!(flags(id, Mitigation::ChildProcess) & 1, 1);
    assert_eq!(flags(id, Mitigation::SideChannelIsolation) & 1, 1);
    assert_eq!(flags(id, Mitigation::StrictHandleCheck) & 0b11, 0b11);
    assert_eq!(renderer.browser_goes(), Some(0));
}

#[test]
fn a_renderer_process_told_no_sandbox_is_in_none() {
    let renderer = RunningRenderer::start(&["--no-sandbox"]);
    let id = renderer.child.id();
    let process = open(id).unwrap();
    assert!(!in_app_container(process.as_handle()).unwrap());
    assert!(integrity(process.as_handle()).unwrap() > LOW);
    assert_eq!(flags(id, Mitigation::SystemCallDisable) & 0b101, 0);
    assert_eq!(flags(id, Mitigation::Signature) & 1, 0);
    assert_eq!(renderer.browser_goes(), Some(0));
}

/// The processes whose parent is `parent`, as the system lists them.
fn children_of(parent: u32) -> Vec<u32> {
    let listed = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(format!(
            "(Get-CimInstance Win32_Process -Filter 'ParentProcessId={parent}').ProcessId"
        ))
        .stdin(Stdio::null())
        .output()
        .expect("powershell runs");
    String::from_utf8_lossy(&listed.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

/// The renderer process of a browser that has one tab, once it has
/// lowered itself or `expected` says it won't.
fn renderer_of(browser: &Browser, lowered: bool) -> u32 {
    let started = Instant::now();
    loop {
        for id in children_of(browser.child.id()) {
            let Ok(process) = open(id) else { continue };
            let level = integrity(process.as_handle()).unwrap_or(u32::MAX);
            if !lowered || level == UNTRUSTED {
                return id;
            }
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "no renderer process"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The browser itself starts its renderers in the sandbox, each in an app
/// container, and leaves Microsoft-signed DLLs alone only when told.
#[test]
fn a_browsers_renderer_is_in_the_sandbox_unless_the_browser_is_told_otherwise() {
    let mut browser = Browser::start_with_args_and_env(&["--process-per-tab"], &[]);
    let _session = browser.attach();
    let id = renderer_of(&browser, true);
    assert!(in_app_container(open(id).unwrap().as_handle()).unwrap());
    assert_eq!(flags(id, Mitigation::SystemCallDisable) & 1, 1);
    assert_eq!(flags(id, Mitigation::Signature) & 1, 1);
    drop(browser);

    let mut browser = Browser::start_with_args_and_env(
        &["--process-per-tab", "--allow-third-party-modules"],
        &[],
    );
    let _session = browser.attach();
    let id = renderer_of(&browser, true);
    assert!(in_app_container(open(id).unwrap().as_handle()).unwrap());
    assert_eq!(flags(id, Mitigation::Signature) & 1, 0);
    // The rest stays.
    assert_eq!(flags(id, Mitigation::SystemCallDisable) & 1, 1);
    drop(browser);

    let mut browser = Browser::start_with_args_and_env(&["--process-per-tab", "--no-sandbox"], &[]);
    let _session = browser.attach();
    let id = renderer_of(&browser, false);
    assert!(!in_app_container(open(id).unwrap().as_handle()).unwrap());
}

/// A page of the system's fonts: generic families, and scripts the
/// families named lack.
const FONTS_PAGE: &str = "data:text/html;charset=utf-8,<span style='font: 40px serif'>Sandbox mmmm</span>\
    <span style='font: 40px monospace'>Sandbox mmmm</span>\
    <span style='font: 40px sans-serif'>中文字</span><span style='font: 40px serif'>日本語</span>\
    <span style='font: 40px serif'>한국어</span><span style='font: 40px serif'>العربية</span>\
    <span style='font: 40px serif'>ไทย</span><span style='font: 40px system-ui'>System</span>";

/// In the sandbox a renderer has the system's fonts, the font that has a
/// character the page's lack, and the time zone, as it does outside, and
/// as a renderer that is a thread of the browser's.
#[test]
fn a_sandboxed_renderer_shows_a_page_as_one_outside_does() {
    let page = FONTS_PAGE;
    let seen = |args: &[&str]| {
        let mut browser = Browser::start_with_system_fonts(args);
        let session = browser.attach();
        browser.navigate(&session, page);
        let read = "return [[...document.querySelectorAll('span')].map(s => s.offsetWidth), \
            Intl.DateTimeFormat().resolvedOptions().timeZone, new Date(0).getTimezoneOffset(), \
            navigator.language, Intl.DateTimeFormat().resolvedOptions().locale, \
            new Intl.NumberFormat().format(1234.5)]";
        script(&mut browser, &session, read, serde_json::json!([])).unwrap()
    };
    let inside = seen(&["--process-per-tab"]);
    let widths = inside[0].as_array().unwrap();
    assert_eq!(widths.len(), 8);
    assert!(
        widths.iter().all(|width| width.as_f64().unwrap() > 50.0),
        "{inside}"
    );
    assert_eq!(inside, seen(&["--process-per-tab", "--no-sandbox"]));
    assert_eq!(inside, seen(&["--single-process"]));
}

/// A renderer process, in its sandbox or out of it, is given only the
/// listed part of its browser's environment: not a secret the browser
/// has, nor the user's own directories, but machopo's switches (whatever
/// their names' case) and where Windows is. The locale and the time zone
/// are passed on too, though on Windows the page has them from the
/// system's settings: it sees what a renderer on the browser's thread
/// does.
#[test]
fn a_renderer_process_has_only_the_listed_part_of_its_browsers_environment() {
    let secret = std::ffi::OsStr::new("not for a renderer");
    let switch = std::ffi::OsStr::new("given");
    let cased = std::ffi::OsStr::new("given too");
    let lang = std::ffi::OsStr::new("ja_JP.UTF-8");
    let tz = std::ffi::OsStr::new("Asia/Tokyo");
    let env = [
        ("SOME_API_TOKEN", secret),
        ("MACHOPO_TEST_SWITCH", switch),
        ("Machopo_Test_Cased", cased),
        ("LANG", lang),
        ("TZ", tz),
    ];
    let read = "return [Intl.DateTimeFormat().resolvedOptions().locale, \
        Intl.DateTimeFormat().resolvedOptions().timeZone, new Date(0).getTimezoneOffset(), \
        new Intl.NumberFormat().format(1234.5)]";
    let seen = |args: &[&str]| {
        let mut browser = Browser::start_in(args, &env, Stdio::null());
        let session = browser.attach();
        browser.navigate(&session, "about:blank");
        let seen = script(&mut browser, &session, read, serde_json::json!([])).unwrap();
        let environments: Vec<_> = renderer_pids(&mut browser)
            .into_iter()
            .map(|pid| sunaba::windows::environment(u32::try_from(pid).unwrap()))
            .collect();
        (seen, environments)
    };
    let (on_a_thread, none) = seen(&["--single-process"]);
    assert!(none.is_empty(), "{none:?}");
    let system = |name: &str| std::env::var_os(name).expect(name);
    for args in [
        &["--process-per-tab"][..],
        &["--process-per-tab", "--no-sandbox"],
    ] {
        let (seen, environments) = seen(args);
        assert_eq!(seen, on_a_thread, "{args:?}");
        let [Some(environment)] = &environments[..] else {
            panic!("{args:?}: {environments:?}");
        };
        let names: Vec<_> = environment.iter().map(|(name, _)| name).collect();
        for (name, _) in environment {
            assert!(
                shell::renderer_sandbox::passes_to_renderer(name),
                "{args:?}: {name:?} isn't listed: {names:?}"
            );
        }
        let value = |name: &str| {
            environment
                .iter()
                .find(|(found, _)| found.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.as_os_str())
        };
        for absent in [
            "SOME_API_TOKEN",
            "USERPROFILE",
            "PATH",
            "TEMP",
            "LOCALAPPDATA",
        ] {
            assert_eq!(value(absent), None, "{args:?}: {absent}");
        }
        assert_eq!(value("MACHOPO_TEST_SWITCH"), Some(switch), "{args:?}");
        assert_eq!(value("MACHOPO_TEST_CASED"), Some(cased), "{args:?}");
        assert_eq!(value("LANG"), Some(lang), "{args:?}");
        assert_eq!(value("TZ"), Some(tz), "{args:?}");
        for windows in ["SystemRoot", "SystemDrive"] {
            assert_eq!(
                value(windows),
                Some(system(windows).as_os_str()),
                "{args:?}"
            );
        }
        // The renderer takes its channel's out once it has the channel.
        assert_eq!(value("MACHOPO_IPC_CHANNEL"), None, "{args:?}");
    }
}

/// The names of the DLLs process `id` has loaded, as the system lists
/// them, in lower case.
fn modules_of(id: u32) -> Vec<String> {
    let listed = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(format!("(Get-Process -Id {id}).Modules.ModuleName"))
        .stdin(Stdio::null())
        .output()
        .expect("powershell runs");
    String::from_utf8_lossy(&listed.stdout)
        .lines()
        .map(|line| line.trim().to_ascii_lowercase())
        .filter(|line| !line.is_empty())
        .collect()
}

/// A renderer process asks its browser for the system's fonts, in the
/// sandbox and out of it: it lays text of them out without loading
/// DirectWrite, which is what finds them.
#[test]
fn a_renderer_process_has_the_systems_fonts_from_its_browser() {
    for (args, lowered) in [
        (&["--process-per-tab"][..], true),
        (&["--process-per-tab", "--no-sandbox"][..], false),
    ] {
        let mut browser = Browser::start_with_system_fonts(args);
        let session = browser.attach();
        browser.navigate(&session, FONTS_PAGE);
        let read = "return [...document.querySelectorAll('span')].map(s => s.offsetWidth)";
        let widths = script(&mut browser, &session, read, serde_json::json!([])).unwrap();
        assert!(
            widths
                .as_array()
                .unwrap()
                .iter()
                .all(|width| width.as_f64().unwrap() > 50.0),
            "{widths}"
        );
        let modules = modules_of(renderer_of(&browser, lowered));
        assert!(
            modules.iter().any(|name| name == "ntdll.dll"),
            "{modules:?}"
        );
        assert!(
            !modules.iter().any(|name| name == "dwrite.dll"),
            "{args:?}: {modules:?}"
        );
    }
}

/// What the probe did: its line (`ok N`, `error CODE`), or what the
/// process exited with when it printed none.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Ok(u64),
    Error(i32),
    Exit(i32),
}

/// `ERROR_ACCESS_DENIED`.
const DENIED: Outcome = Outcome::Error(5);

fn probe_with(switches: &[&str], attempt: &[&str]) -> Outcome {
    let output = Command::new(MACHOPO)
        .arg(shell::renderer_sandbox::PROBE_SWITCH)
        .args(switches)
        .arg("--")
        .args(attempt)
        .stdin(Stdio::null())
        .output()
        .expect("machopo runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().next().unwrap_or("").trim();
    if let Some(number) = line.strip_prefix("ok ") {
        Outcome::Ok(number.parse().unwrap())
    } else if let Some(code) = line.strip_prefix("error ") {
        Outcome::Error(code.parse().unwrap())
    } else if output.status.code() != Some(0) {
        Outcome::Exit(output.status.code().unwrap_or(-1))
    } else {
        panic!("the probe printed {stdout:?} ({stderr})");
    }
}

fn probe(attempt: &[&str]) -> Outcome {
    probe_with(&[], attempt)
}

/// A file of this test's under the user's own directories.
fn document(name: &str) -> (String, String) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("renderer-sandbox")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("document.txt");
    std::fs::write(&file, b"private").unwrap();
    (
        file.to_str().unwrap().to_owned(),
        dir.to_str().unwrap().to_owned(),
    )
}

fn system_root() -> String {
    std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())
}

#[test]
fn the_probe_is_started_as_a_renderer_is_and_lowers_itself() {
    for switches in [&[][..], &["--test-fonts"][..]] {
        assert_eq!(probe_with(switches, &["nothing"]), Outcome::Ok(0));
        assert_eq!(probe_with(switches, &["container"]), Outcome::Ok(1));
        assert_eq!(
            probe_with(switches, &["integrity"]),
            Outcome::Ok(u64::from(UNTRUSTED))
        );
        assert_eq!(probe_with(switches, &["thread", "nothing"]), Outcome::Ok(0));
    }
    // The executable itself, which links what its browser side draws
    // with, starts with win32k's calls turned off.
    assert_eq!(probe(&["mitigation", "4"]), Outcome::Ok(0b101));
    // With no sandbox there is nothing to probe.
    assert_eq!(
        probe_with(&["--no-sandbox"], &["nothing"]),
        Outcome::Exit(2)
    );
}

#[test]
fn a_renderer_reads_no_file_of_the_users() {
    let (file, dir) = document("read");
    assert_eq!(probe(&["read", &file]), DENIED);
    assert_eq!(probe(&["stat", &file]), DENIED);
    assert_eq!(probe(&["list", &dir]), DENIED);
    assert_eq!(probe(&["thread", "read", &file]), DENIED);
    if let Ok(home) = std::env::var("USERPROFILE") {
        assert_eq!(probe(&["list", &home]), DENIED);
    }
}

#[test]
fn a_renderer_makes_changes_and_removes_no_file() {
    let (file, dir) = document("write");
    let new = format!(r"{dir}\new.txt");
    assert_eq!(probe(&["write", &new]), DENIED);
    assert!(!PathBuf::from(&new).exists());
    assert_eq!(probe(&["write", &file]), DENIED);
    assert_eq!(probe(&["remove", &file]), DENIED);
    assert_eq!(std::fs::read(&file).unwrap(), b"private");
    let temp = std::env::temp_dir().join(format!("machopo-probe-{}.txt", std::process::id()));
    assert_eq!(probe(&["write", temp.to_str().unwrap()]), DENIED);
    assert!(!temp.exists());
}

/// Not kept from it: the system's fonts are read by the renderer itself,
/// and with them whatever the system lets every application read.
#[test]
fn a_renderer_reads_the_systems_fonts_and_what_every_application_may() {
    let fonts = format!(r"{}\Fonts", system_root());
    assert!(matches!(probe(&["list", &fonts]), Outcome::Ok(files) if files > 0));
    let dll = format!(r"{}\System32\kernel32.dll", system_root());
    assert!(matches!(probe(&["read", &dll]), Outcome::Ok(bytes) if bytes > 0));
}

#[test]
fn a_renderer_reaches_no_network_starts_no_program_and_opens_no_process() {
    // `WSAEACCES`, for an address no machine has.
    assert_eq!(probe(&["connect", "192.0.2.1:80"]), Outcome::Error(10013));
    let cmd = format!(r"{}\System32\cmd.exe", system_root());
    // `ERROR_CHILD_PROCESS_BLOCKED`.
    assert_eq!(probe(&["run", &cmd, "/c", "exit 0"]), Outcome::Error(367));
    let this = std::process::id().to_string();
    assert_eq!(probe(&["process", &this]), DENIED);
}
