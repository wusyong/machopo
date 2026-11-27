//! The renderer process's sandbox on macOS: a real renderer is in it by
//! the kernel's account and shows a page as one outside does, and what
//! its profile refuses is refused, tried from inside by the executable
//! itself (`--type=renderer-sandbox-probe`: it enters as a renderer
//! process does, then tries one thing).

use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::Command;

use crate::automation_pipe::{Browser, renderer_pids, script};
use crate::renderer_process::{MACHOPO, RunningRenderer};

#[test]
fn a_renderer_process_is_in_the_sandbox() {
    let renderer = RunningRenderer::start(&["--test-fonts"]);
    assert!(sunaba::macos::sandboxed(renderer.child.id()));
    assert_eq!(renderer.browser_goes(), Some(0));
}

#[test]
fn a_renderer_process_told_no_sandbox_is_in_none() {
    let renderer = RunningRenderer::start(&["--no-sandbox"]);
    assert!(!sunaba::macos::sandboxed(renderer.child.id()));
    assert_eq!(renderer.browser_goes(), Some(0));
}

/// In the sandbox a renderer has the system's fonts (its browser's, as
/// every renderer's), the font that has a character the page's lack, and
/// the time zone, as it does outside, and as a renderer that is a thread
/// of the browser's: though it may read no font and reach no font service
/// of the system's itself.
#[test]
fn a_sandboxed_renderer_shows_a_page_as_one_outside_does() {
    let page = "data:text/html;charset=utf-8,<span style='font: 40px serif'>Sandbox mmmm</span>\
        <span style='font: 40px monospace'>Sandbox mmmm</span>\
        <span style='font: 40px sans-serif'>中文字</span><span style='font: 40px serif'>日本語</span>\
        <span style='font: 40px serif'>한국어</span><span style='font: 40px serif'>العربية</span>\
        <span style='font: 40px serif'>ไทย</span><span style='font: 40px system-ui'>System</span>";
    let seen = |args: &[&str]| {
        let mut browser = Browser::start_with_system_fonts(args);
        let session = browser.attach();
        browser.navigate(&session, page);
        let read = "return [[...document.querySelectorAll('span')].map(s => s.offsetWidth), \
            Intl.DateTimeFormat().resolvedOptions().timeZone, new Date(0).getTimezoneOffset(), \
            Intl.DateTimeFormat().resolvedOptions().locale, new Intl.NumberFormat().format(1234.5)]";
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

/// What the probe did: its line (`ok N`, `error ERRNO`) or the signal that
/// ended it.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Ok(u64),
    Errno(i32),
    Signal(i32),
}

/// What the sandbox fails a call with.
const REFUSED: Outcome = Outcome::Errno(libc::EPERM);
/// launchd's answers: a service the sandbox doesn't let be looked up, and
/// one that it does and that isn't there.
const NOT_PRIVILEGED: Outcome = Outcome::Errno(1100);
const UNKNOWN_SERVICE: Outcome = Outcome::Errno(1102);

fn probe_in(home: Option<&str>, switches: &[&str], attempt: &[&str]) -> Outcome {
    let mut command = Command::new(MACHOPO);
    command
        .arg(shell::renderer_sandbox::PROBE_SWITCH)
        .args(switches)
        .arg("--")
        .args(attempt);
    if let Some(home) = home {
        command.env("HOME", home);
    }
    let output = command.output().expect("machopo runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(signal) = output.status.signal() {
        Outcome::Signal(signal)
    } else if let Some(number) = stdout.trim().strip_prefix("ok ") {
        Outcome::Ok(number.parse().unwrap())
    } else if let Some(errno) = stdout.trim().strip_prefix("error ") {
        Outcome::Errno(errno.parse().unwrap())
    } else {
        panic!(
            "the probe printed {stdout:?}, exited {:?} ({stderr})",
            output.status
        );
    }
}

/// With the system's fonts, as a renderer for a window.
fn probe(attempt: &[&str]) -> Outcome {
    probe_in(None, &[], attempt)
}

/// With the built-in fonts alone.
fn probe_with_test_fonts(attempt: &[&str]) -> Outcome {
    probe_in(None, &["--test-fonts"], attempt)
}

/// A home directory of this test's with a font file's stand-in in its
/// `Library/Fonts` and a document beside it: the directory, the font, the
/// document. By the paths the sandbox knows them by.
fn home(name: &str) -> (String, String, String) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("renderer-sandbox")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Library/Fonts")).unwrap();
    let dir = dir.canonicalize().unwrap();
    let font = dir.join("Library/Fonts/font.ttf");
    std::fs::write(&font, b"contents").unwrap();
    let document = dir.join("document.txt");
    std::fs::write(&document, b"private").unwrap();
    let text = |path: PathBuf| path.to_str().unwrap().to_owned();
    (text(dir), text(font), text(document))
}

#[test]
fn the_probe_enters_and_makes_a_thread() {
    for probe in [probe, probe_with_test_fonts] {
        assert_eq!(probe(&["nothing"]), Outcome::Ok(0));
        assert_eq!(probe(&["entered"]), Outcome::Ok(1));
        assert_eq!(probe(&["thread", "nothing"]), Outcome::Ok(0));
    }
}

#[test]
fn a_renderer_reads_no_file_of_the_users_nor_learns_of_one() {
    let (home, _, document) = home("read");
    for switches in [&[][..], &["--test-fonts"][..]] {
        let probe = |attempt: &[&str]| probe_in(Some(&home), switches, attempt);
        assert_eq!(probe(&["read", &document]), REFUSED);
        assert_eq!(probe(&["stat", &document]), REFUSED);
        assert_eq!(probe(&["list", &home]), REFUSED);
        assert_eq!(probe(&["read", "/etc/passwd"]), REFUSED);
        assert_eq!(probe(&["read", "/private/etc/passwd"]), REFUSED);
        assert_eq!(probe(&["list", "/"]), REFUSED);
        assert_eq!(probe(&["stat", "/Users"]), REFUSED);
        // Not a guarantee, a limit to know of: a path that isn't there is
        // said not to be, so whether one is there can be told.
        assert_eq!(
            probe(&["stat", "/Users/no-such-user-of-machopo"]),
            Outcome::Errno(libc::ENOENT)
        );
        assert_eq!(probe(&["read", MACHOPO]), REFUSED);
        // On a thread made after entering, too.
        assert_eq!(probe(&["thread", "read", &document]), REFUSED);
    }
    // The user's real home.
    if let Some(real) = std::env::var_os("HOME") {
        assert_eq!(probe(&["list", real.to_str().unwrap()]), REFUSED);
    }
}

/// The system's fonts are the browser's to open: a renderer reads none of
/// them, whichever fonts it uses, nor speaks to the system's font
/// service.
#[test]
fn a_renderer_reads_no_font_of_the_systems_nor_reaches_its_font_service() {
    let (home, font, _) = home("fonts");
    // Everyone's fonts, the ones the system keeps for itself, and the
    // ones it has downloaded (a directory for each, with what it is in
    // its name).
    let mut directories = vec![
        "/System/Library/Fonts".to_owned(),
        "/Library/Fonts".to_owned(),
        "/System/Library/PrivateFrameworks/FontServices.framework/Resources/Reserved".to_owned(),
    ];
    directories.extend(
        std::fs::read_dir("/System/Library/AssetsV2")
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                name.strip_prefix("com_apple_MobileAsset_Font")
                    .is_some_and(|rest| {
                        !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
                    })
            })
            .map(|path| path.to_str().unwrap().to_owned()),
    );
    let font_file = std::fs::read_dir("/System/Library/Fonts")
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.is_file())
        .unwrap();
    for switches in [&[][..], &["--test-fonts"][..]] {
        for directory in &directories {
            if std::path::Path::new(directory).exists() {
                assert_eq!(
                    probe_in(None, switches, &["list", directory]),
                    REFUSED,
                    "{directory} {switches:?}"
                );
            }
        }
        assert_eq!(
            probe_in(None, switches, &["read", font_file.to_str().unwrap()]),
            REFUSED
        );
        assert_eq!(probe_in(Some(&home), switches, &["read", &font]), REFUSED);
        assert_eq!(
            probe_in(None, switches, &["lookup", "com.apple.fonts"]),
            NOT_PRIVILEGED,
            "the font service"
        );
        assert_eq!(
            probe_in(None, switches, &["lookup", "com.apple.FontObjectsServer"]),
            NOT_PRIVILEGED
        );
    }
}

#[test]
fn a_renderer_reads_the_time_zone_and_what_the_cpus_are() {
    for probe in [probe, probe_with_test_fonts] {
        assert!(matches!(probe(&["read", "/etc/localtime"]), Outcome::Ok(bytes) if bytes > 0));
        assert_eq!(probe(&["sysctl", "hw.ncpu"]), Outcome::Ok(4));
        assert!(matches!(
            probe(&["sysctl", "hw.optional.arm.FEAT_LSE"]),
            Outcome::Ok(_) | Outcome::Errno(libc::ENOENT)
        ));
        assert_eq!(probe(&["sysctl", "kern.osproductversion"]), REFUSED);
        // Not what names the machine or its user.
        assert_eq!(probe(&["sysctl", "kern.hostname"]), REFUSED);
        assert_eq!(probe(&["sysctl", "kern.bootargs"]), REFUSED);
    }
}

#[test]
fn a_renderer_changes_nothing_in_the_file_system() {
    let (home, font, document) = home("write");
    // Where it can read as much as where it can't.
    let new = format!("{home}/Library/Fonts/new.ttf");
    let attempts: [&[&str]; 7] = [
        &["write", &new],
        &["write", &font],
        &["remove", &font],
        &["chmod", &font],
        &["write", &document],
        &["remove", &document],
        &["chmod", &document],
    ];
    for attempt in attempts {
        assert_eq!(probe_in(Some(&home), &[], attempt), REFUSED, "{attempt:?}");
    }
    assert!(!std::path::Path::new(&new).exists());
    assert_eq!(std::fs::read(&font).unwrap(), b"contents");
    assert_eq!(std::fs::read(&document).unwrap(), b"private");
    for file in [&font, &document] {
        let mode = std::fs::metadata(file).unwrap().permissions().mode();
        assert_ne!(mode & 0o777, 0o600, "its mode is as it was made");
    }
    assert_eq!(probe(&["write", "/tmp/machopo-renderer-sandbox"]), REFUSED);
}

#[test]
fn a_renderer_reaches_nothing_outside_its_process() {
    let shm = format!("/machopo-{}", std::process::id());
    let this = std::process::id().to_string();
    let attempts: [&[&str]; 10] = [
        &["listen"],
        &["connect", "127.0.0.1:9"],
        &["unix", "/var/run/syslog"],
        &["fork"],
        &["run", "/usr/bin/true"],
        &["run", MACHOPO],
        &["shm", &shm],
        // Its parent: signalled, and asked about.
        &["signal", &this],
        &["process", &this],
        &["process", "1"],
    ];
    for probe in [probe, probe_with_test_fonts] {
        for attempt in attempts {
            assert_eq!(probe(attempt), REFUSED, "{attempt:?}");
        }
    }
}

#[test]
fn a_renderer_that_gives_up_ends_as_one_outside_does() {
    // `abort` takes no permission to signal: the profile gives none.
    assert_eq!(
        probe_with_test_fonts(&["abort"]),
        Outcome::Signal(libc::SIGABRT)
    );
}

#[test]
fn a_renderer_looks_up_its_browsers_rendezvous_and_no_other_service() {
    // The probe's browser is this process: the name is let through, to
    // the server this process has if a test of it has started a renderer,
    // or to launchd's having none such.
    let name = |pid: u32| format!("org.machopo.MachPortRendezvousServer.{pid}");
    for probe in [probe, probe_with_test_fonts] {
        let own = probe(&["lookup", &name(std::process::id())]);
        assert!(own == Outcome::Ok(0) || own == UNKNOWN_SERVICE, "{own:?}");
        assert_eq!(probe(&["lookup", &name(1)]), NOT_PRIVILEGED);
        for service in [
            "com.apple.cfprefsd.daemon",
            "com.apple.cfprefsd.agent",
            "com.apple.logd",
            "com.apple.system.notification_center",
            "com.apple.system.opendirectoryd.libinfo",
            "com.apple.windowserver.active",
            "com.apple.pasteboard.1",
            "com.apple.coreservices.launchservicesd",
            "com.apple.FontObjectsServer",
        ] {
            assert_eq!(probe(&["lookup", service]), NOT_PRIVILEGED, "{service}");
        }
    }
}

/// A renderer process, in its sandbox or out of it, is given only the
/// listed part of its browser's environment: not a secret the browser
/// has, but machopo's switches, the locale, the time zone and the user's
/// text encoding, which the page then sees as a renderer on the
/// browser's thread does.
#[test]
fn a_renderer_process_has_only_the_listed_part_of_its_browsers_environment() {
    let secret = std::ffi::OsStr::new("not for a renderer");
    let switch = std::ffi::OsStr::new("given");
    let lang = std::ffi::OsStr::new("ja_JP.UTF-8");
    let tz = std::ffi::OsStr::new("Asia/Tokyo");
    let encoding = std::ffi::OsStr::new("0x1F5:0x0:0x0");
    let env = [
        ("SOME_API_TOKEN", secret),
        ("MACHOPO_TEST_SWITCH", switch),
        ("LANG", lang),
        ("TZ", tz),
        ("__CF_USER_TEXT_ENCODING", encoding),
    ];
    let read = "return [Intl.DateTimeFormat().resolvedOptions().locale, \
        Intl.DateTimeFormat().resolvedOptions().timeZone, new Date(0).getTimezoneOffset()]";
    let seen = |args: &[&str]| {
        let mut browser = Browser::start_in(args, &env, std::process::Stdio::null());
        let session = browser.attach();
        browser.navigate(&session, "about:blank");
        let seen = script(&mut browser, &session, read, serde_json::json!([])).unwrap();
        let environments: Vec<_> = renderer_pids(&mut browser)
            .into_iter()
            .map(|pid| sunaba::macos::environment(u32::try_from(pid).unwrap()))
            .collect();
        (seen, environments)
    };
    let (on_a_thread, none) = seen(&["--single-process"]);
    assert!(none.is_empty(), "{none:?}");
    assert_eq!(
        on_a_thread,
        serde_json::json!(["ja-JP", "Asia/Tokyo", -540])
    );
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
                .find(|(found, _)| found == name)
                .map(|(_, value)| value.as_os_str())
        };
        assert_eq!(value("SOME_API_TOKEN"), None, "{args:?}");
        assert_eq!(value("HOME"), None, "{args:?}");
        assert_eq!(value("MACHOPO_TEST_SWITCH"), Some(switch), "{args:?}");
        assert_eq!(value("LANG"), Some(lang), "{args:?}");
        assert_eq!(value("TZ"), Some(tz), "{args:?}");
        assert_eq!(value("__CF_USER_TEXT_ENCODING"), Some(encoding), "{args:?}");
    }
}
