//! The renderer process's sandbox: a real renderer's threads are in it by
//! the kernel's account, and what its policy refuses is refused, tried
//! from inside by the executable itself (`--type=renderer-sandbox-probe`:
//! it enters as a renderer process does, then tries one thing).

use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::Command;

use crate::automation_pipe::{Browser, renderer_pids, script};
use crate::renderer_process::{MACHOPO, RunningRenderer};

/// `name`'s number in every thread's `/proc/<pid>/task/<tid>/status`.
fn of_every_thread(pid: u32, name: &str) -> Vec<u32> {
    let tasks = std::fs::read_dir(format!("/proc/{pid}/task")).expect("the process's threads");
    tasks
        .map(|task| {
            let status = std::fs::read_to_string(task.unwrap().path().join("status")).unwrap();
            status
                .lines()
                .find_map(|line| {
                    line.strip_prefix(name)?
                        .strip_prefix(':')?
                        .trim()
                        .parse()
                        .ok()
                })
                .unwrap_or_else(|| panic!("no {name} in {status}"))
        })
        .collect()
}

#[test]
fn every_thread_of_a_renderer_process_is_in_the_sandbox() {
    let renderer = RunningRenderer::start(&["--test-fonts"]);
    let pid = renderer.child.id();
    let filtered = of_every_thread(pid, "Seccomp");
    // The main thread, the IO thread, the compositor, and V8's.
    assert!(filtered.len() > 3, "{filtered:?}");
    // 2: a filter.
    assert!(filtered.iter().all(|mode| *mode == 2), "{filtered:?}");
    let no_new_privs = of_every_thread(pid, "NoNewPrivs");
    assert!(no_new_privs.iter().all(|set| *set == 1), "{no_new_privs:?}");
    assert_eq!(renderer.browser_goes(), Some(0));
}

#[test]
fn a_renderer_process_told_no_sandbox_is_in_none() {
    let renderer = RunningRenderer::start(&["--no-sandbox"]);
    let filtered = of_every_thread(renderer.child.id(), "Seccomp");
    assert!(filtered.len() > 3, "{filtered:?}");
    assert!(filtered.iter().all(|mode| *mode == 0), "{filtered:?}");
    assert_eq!(renderer.browser_goes(), Some(0));
}

/// The system's fonts are the browser's to open and hand the renderer: in
/// the sandbox, which reads none of them, it lays text out as wide as
/// outside it and as a renderer on a thread of the browser's does, with
/// the same fonts, the same fallbacks and the same time zone. (Found
/// inside, there would be none: the system's list of them can't be read
/// there.)
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
            Intl.DateTimeFormat().resolvedOptions().locale, new Intl.NumberFormat().format(1234.5), \
            navigator.hardwareConcurrency]";
        script(&mut browser, &session, read, serde_json::json!([])).unwrap()
    };
    let on_a_thread = seen(&["--single-process"]);
    let widths = on_a_thread[0].as_array().unwrap();
    assert_eq!(widths.len(), 8);
    if widths[0].as_f64().unwrap() == 0.0 {
        // A machine with no fonts has nothing to tell them apart.
        return;
    }
    assert!(widths[0].as_f64().unwrap() > 100.0, "{on_a_thread}");
    assert!(widths[1].as_f64().unwrap() > 100.0, "{on_a_thread}");
    assert_eq!(seen(&["--process-per-tab"]), on_a_thread);
    assert_eq!(seen(&["--process-per-tab", "--no-sandbox"]), on_a_thread);
}

/// What the probe did: its line (`ok N`, `error ERRNO`) or the signal that
/// ended it.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Ok(u64),
    Errno(i32),
    Signal(i32),
}

const SIGSYS: Outcome = Outcome::Signal(libc::SIGSYS);
const EACCES: Outcome = Outcome::Errno(libc::EACCES);

fn probe_with(switches: &[&str], attempt: &[&str]) -> (Outcome, String) {
    probe_in_zone(std::env::var("TZ").ok().as_deref(), switches, attempt)
}

/// [`probe_with`], its `TZ` `tz` (none, with `None`), as a renderer
/// process's is.
fn probe_in_zone(tz: Option<&str>, switches: &[&str], attempt: &[&str]) -> (Outcome, String) {
    let mut command = Command::new(MACHOPO);
    command
        .arg(shell::renderer_sandbox::PROBE_SWITCH)
        .args(switches)
        .arg("--")
        .args(attempt);
    match tz {
        Some(tz) => command.env("TZ", tz),
        None => command.env_remove("TZ"),
    };
    let output = command.output().expect("machopo runs");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let outcome = if let Some(signal) = output.status.signal() {
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
    };
    (outcome, stderr)
}

/// With the built-in fonts alone: no font directory.
fn probe(attempt: &[&str]) -> Outcome {
    probe_with(&["--test-fonts"], attempt).0
}

fn syscall(call: libc::c_long, arguments: &[i64]) -> Outcome {
    let mut attempt = vec!["syscall".to_owned(), call.to_string()];
    attempt.extend(arguments.iter().map(i64::to_string));
    let attempt: Vec<&str> = attempt.iter().map(String::as_str).collect();
    probe(&attempt)
}

/// A directory of this test's, with `file.txt` in it.
fn directory(name: &str) -> (PathBuf, String) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("renderer-sandbox")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("file.txt");
    std::fs::write(&file, b"contents").unwrap();
    (dir, file.to_str().unwrap().to_owned())
}

#[test]
fn the_probe_enters_and_makes_a_thread() {
    assert_eq!(probe(&["nothing"]), Outcome::Ok(0));
    assert_eq!(probe(&["thread", "nothing"]), Outcome::Ok(0));
    // With the system's fonts, as a renderer for a window.
    assert_eq!(probe_with(&[], &["nothing"]).0, Outcome::Ok(0));
}

#[test]
fn a_renderer_reads_no_file_of_the_users() {
    let (dir, file) = directory("read");
    assert_eq!(probe(&["read", &file]), EACCES);
    assert_eq!(probe(&["list", dir.to_str().unwrap()]), EACCES);
    assert_eq!(probe(&["read", "/etc/passwd"]), EACCES);
    assert_eq!(probe(&["list", "/"]), EACCES);
    if let Some(home) = std::env::var_os("HOME") {
        assert_eq!(probe(&["list", home.to_str().unwrap()]), EACCES);
    }
    assert_eq!(probe(&["read", "/proc/self/environ"]), EACCES);
    // On a thread made after entering, too.
    assert_eq!(probe(&["thread", "read", &file]), EACCES);
}

/// The first file found under `directory`, depth first.
fn a_file_under(directory: &std::path::Path) -> Option<PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(directory).ok()?.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::path);
    entries.into_iter().find_map(|entry| {
        let path = entry.path();
        if path.is_file() {
            Some(path)
        } else {
            a_file_under(&path)
        }
    })
}

#[test]
fn a_renderer_reads_no_font_of_the_systems() {
    let home = std::env::var("HOME").unwrap_or_default();
    // Everyone's fonts and the user's, fontconfig's configuration and its
    // caches: what fontconfig reads to find them.
    let directories = [
        "/usr/share/fonts".to_owned(),
        "/usr/local/share/fonts".to_owned(),
        format!("{home}/.local/share/fonts"),
        format!("{home}/.fonts"),
        "/etc/fonts".to_owned(),
        "/var/cache/fontconfig".to_owned(),
        "/usr/lib/fontconfig/cache".to_owned(),
        format!("{home}/.cache/fontconfig"),
    ];
    let font = a_file_under(std::path::Path::new("/usr/share/fonts"));
    for switches in [&[][..], &["--test-fonts"][..]] {
        for directory in &directories {
            if std::path::Path::new(directory).is_dir() {
                assert_eq!(
                    probe_with(switches, &["list", directory]).0,
                    EACCES,
                    "{directory} {switches:?}"
                );
            }
        }
        if let Some(font) = &font {
            assert_eq!(
                probe_with(switches, &["read", font.to_str().unwrap()]).0,
                EACCES,
                "{font:?}"
            );
        }
    }
}

/// A renderer reads no file: not the number of CPUs (glibc counts the ones
/// it may run on), not `/dev/urandom` (V8's seeds are given it), and not
/// the time zone's when its `TZ` names one that ICU takes; only where it
/// doesn't, or checks it against glibc's (a short name), the zone's files
/// are open: `/etc/localtime` and the zone files.
#[test]
fn a_renderer_reads_no_file_but_the_time_zones_it_has_to_find() {
    let zone_files = ["/etc/localtime", "/usr/share/zoneinfo/UTC"];
    // `CET`: a short name, which ICU checks against glibc's offset, read
    // from the zone's file.
    for tz in [None, Some("Asia/Tokyo"), Some("AST4ADT"), Some("CET")] {
        for path in ["/sys/devices/system/cpu/online", "/dev/urandom"] {
            assert_eq!(
                probe_in_zone(tz, &[], &["open", path]).0,
                EACCES,
                "{path} {tz:?}"
            );
        }
        let finds_its_own = tz != Some("Asia/Tokyo");
        for path in zone_files {
            if !std::path::Path::new(path).exists() {
                continue;
            }
            let opened = probe_in_zone(tz, &[], &["open", path]).0;
            if finds_its_own {
                assert_eq!(opened, Outcome::Ok(0), "{path} {tz:?}");
            } else {
                assert_eq!(opened, EACCES, "{path} {tz:?}");
            }
        }
    }
}

#[test]
fn a_renderer_changes_nothing_in_the_file_system() {
    let (dir, file) = directory("write");
    // With the system's fonts as much as with the built-in ones.
    for switches in [&[][..], &["--test-fonts"][..]] {
        let new = dir.join("new.txt");
        let attempts: [&[&str]; 5] = [
            &["write", new.to_str().unwrap()],
            &["write", &file],
            &["remove", &file],
            &["chmod", &file],
            &["truncate", &file],
        ];
        for attempt in attempts {
            assert_eq!(probe_with(switches, attempt).0, EACCES, "{attempt:?}");
        }
        assert!(!new.exists());
        assert_eq!(std::fs::read(&file).unwrap(), b"contents");
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_ne!(mode & 0o777, 0o600, "its mode is as it was made");
    }
    // By the calls themselves, for where the standard library doesn't go.
    for call in [
        libc::SYS_mkdirat,
        libc::SYS_renameat2,
        libc::SYS_symlinkat,
        libc::SYS_linkat,
        libc::SYS_fchownat,
        libc::SYS_utimensat,
        libc::SYS_truncate,
    ] {
        assert_eq!(syscall(call, &[]), EACCES, "{call}");
    }
    // Its flags are in memory, where a filter can't read them.
    assert_eq!(
        syscall(libc::SYS_openat2, &[]),
        Outcome::Errno(libc::ENOSYS)
    );
}

#[test]
fn a_renderer_that_reaches_outside_its_process_is_ended() {
    let (outcome, stderr) = probe_with(
        &["--test-fonts"],
        &["syscall", &libc::SYS_socket.to_string(), "2", "1", "0"],
    );
    assert_eq!(outcome, SIGSYS);
    assert!(
        stderr.contains("(socket) isn't allowed in this sandbox"),
        "{stderr}"
    );
    for call in [
        libc::SYS_socket,
        libc::SYS_socketpair,
        libc::SYS_connect,
        libc::SYS_bind,
        libc::SYS_execve,
        libc::SYS_execveat,
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_io_uring_setup,
        libc::SYS_bpf,
        libc::SYS_mount,
        libc::SYS_unshare,
        libc::SYS_setns,
        libc::SYS_chroot,
        libc::SYS_chdir,
        libc::SYS_setuid,
        libc::SYS_seccomp,
        libc::SYS_landlock_restrict_self,
        libc::SYS_perf_event_open,
        libc::SYS_userfaultfd,
        libc::SYS_clock_settime,
        libc::SYS_kexec_load,
        libc::SYS_pidfd_open,
    ] {
        assert_eq!(syscall(call, &[]), SIGSYS, "{call}");
    }
}

#[test]
fn a_renderer_makes_threads_and_no_process() {
    assert_eq!(syscall(libc::SYS_clone3, &[]), Outcome::Errno(libc::ENOSYS));
    assert_eq!(
        syscall(libc::SYS_clone, &[i64::from(libc::SIGCHLD)]),
        SIGSYS
    );
    #[cfg(target_arch = "x86_64")]
    for call in [libc::SYS_fork, libc::SYS_vfork] {
        assert_eq!(syscall(call, &[]), SIGSYS, "{call}");
    }
}

#[test]
fn a_renderer_signals_itself_alone() {
    let other = i64::from(std::process::id());
    assert_eq!(syscall(libc::SYS_kill, &[other, 0]), SIGSYS);
    assert_eq!(syscall(libc::SYS_tgkill, &[other, other, 0]), SIGSYS);
    // Every process and its own group.
    assert_eq!(syscall(libc::SYS_kill, &[-1, 0]), SIGSYS);
    assert_eq!(syscall(libc::SYS_kill, &[0, 0]), SIGSYS);
    // Itself: `abort`, which ends it as `abort` does anywhere.
    let (outcome, _) = probe_with(&["--test-fonts"], &["abort"]);
    assert_eq!(outcome, Outcome::Signal(libc::SIGABRT));
}

#[test]
fn a_renderers_calls_are_allowed_for_the_arguments_it_uses() {
    // `prctl`: a crash's core dump turned off, and nothing else of it.
    let dumpable = i64::from(libc::PR_SET_DUMPABLE);
    assert_eq!(syscall(libc::SYS_prctl, &[dumpable, 0]), Outcome::Ok(0));
    assert_eq!(syscall(libc::SYS_prctl, &[dumpable, 1]), SIGSYS);
    assert_eq!(
        syscall(libc::SYS_prctl, &[i64::from(libc::PR_SET_PDEATHSIG), 0]),
        SIGSYS
    );
    assert_eq!(
        syscall(libc::SYS_prctl, &[i64::from(libc::PR_SET_SECCOMP), 0]),
        SIGSYS
    );
    // `ioctl`: whether a descriptor is a terminal (its standard input
    // here isn't: the call is made and says so).
    let tcgets = i64::try_from(libc::TCGETS).unwrap();
    assert!(matches!(
        syscall(libc::SYS_ioctl, &[0, tcgets, 0]),
        Outcome::Errno(_)
    ));
    let tiocsti = i64::try_from(libc::TIOCSTI).unwrap();
    assert_eq!(syscall(libc::SYS_ioctl, &[0, tiocsti, 0]), SIGSYS);
    // `madvise`: the advice the allocators give, not `MADV_REMOVE`.
    assert!(matches!(
        syscall(libc::SYS_madvise, &[0, 0, i64::from(libc::MADV_DONTNEED)]),
        Outcome::Ok(_) | Outcome::Errno(_)
    ));
    assert_eq!(
        syscall(libc::SYS_madvise, &[0, 0, i64::from(libc::MADV_REMOVE)]),
        SIGSYS
    );
    // `prlimit64`: its own limits read, not set (a pointer the filter
    // refuses before the kernel reads it), nor another process's.
    let stack = i64::from(libc::RLIMIT_STACK as i32);
    assert!(matches!(
        syscall(libc::SYS_prlimit64, &[0, stack, 0, 0]),
        Outcome::Ok(0)
    ));
    assert_eq!(syscall(libc::SYS_prlimit64, &[0, stack, 1, 0]), SIGSYS);
    assert_eq!(syscall(libc::SYS_prlimit64, &[1, stack, 0, 0]), SIGSYS);
}

/// A process's environment as it was started (`/proc/<pid>/environ`).
fn environment_of(pid: u64) -> Vec<(String, String)> {
    let environ = std::fs::read(format!("/proc/{pid}/environ")).expect("its environment");
    environ
        .split(|&byte| byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let entry = String::from_utf8_lossy(entry);
            let (name, value) = entry.split_once('=').unwrap_or((&entry, ""));
            (name.to_owned(), value.to_owned())
        })
        .collect()
}

/// A renderer process, in its sandbox or out of it, is given only the
/// listed part of its browser's environment: not a secret the browser
/// has, but machopo's switches, the locale and the time zone, which the
/// page then sees as a renderer on the browser's thread does.
#[test]
fn a_renderer_process_has_only_the_listed_part_of_its_browsers_environment() {
    let secret = std::ffi::OsStr::new("not for a renderer");
    let switch = std::ffi::OsStr::new("given");
    let lang = std::ffi::OsStr::new("ja_JP.UTF-8");
    // Not this machine's: the user's zone is the one that stands.
    let (zone, offset) = match shell::renderer_sandbox::linux::renderer_time_zone().as_deref() {
        Some("Asia/Tokyo") => ("America/New_York", 300),
        _ => ("Asia/Tokyo", -540),
    };
    let tz = std::ffi::OsStr::new(zone);
    let env = [
        ("SOME_API_TOKEN", secret),
        ("MACHOPO_TEST_SWITCH", switch),
        ("LANG", lang),
        ("TZ", tz),
    ];
    let read = "return [Intl.DateTimeFormat().resolvedOptions().locale, \
        Intl.DateTimeFormat().resolvedOptions().timeZone, new Date(0).getTimezoneOffset()]";
    let seen = |args: &[&str]| {
        let mut browser = Browser::start_in(args, &env, std::process::Stdio::null());
        let session = browser.attach();
        browser.navigate(&session, "about:blank");
        let seen = script(&mut browser, &session, read, serde_json::json!([])).unwrap();
        (seen, renderer_pids(&mut browser))
    };
    let (on_a_thread, none) = seen(&["--single-process"]);
    assert!(none.is_empty(), "{none:?}");
    assert_eq!(on_a_thread, serde_json::json!(["ja-JP", zone, offset]));
    for args in [
        &["--process-per-tab"][..],
        &["--process-per-tab", "--no-sandbox"],
    ] {
        let (seen, pids) = seen(args);
        assert_eq!(seen, on_a_thread, "{args:?}");
        assert_eq!(pids.len(), 1, "{args:?}: {pids:?}");
        let environment = environment_of(pids[0]);
        let names: Vec<&str> = environment.iter().map(|(name, _)| name.as_str()).collect();
        for (name, _) in &environment {
            assert!(
                shell::renderer_sandbox::passes_to_renderer(name.as_ref()),
                "{args:?}: {name} isn't listed: {names:?}"
            );
        }
        let value = |name: &str| {
            environment
                .iter()
                .find(|(found, _)| found == name)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(value("SOME_API_TOKEN"), None, "{args:?}");
        assert_eq!(value("HOME"), None, "{args:?}");
        assert_eq!(value("MACHOPO_TEST_SWITCH"), Some("given"), "{args:?}");
        assert_eq!(value("LANG"), Some("ja_JP.UTF-8"), "{args:?}");
        assert_eq!(value("TZ"), Some(zone), "{args:?}");
        // Set after the environment is limited: the channel's.
        assert!(value("MACHOPO_IPC_FD").is_some(), "{args:?}: {names:?}");
    }
}

/// A renderer process whose browser was told no `TZ` is told this
/// machine's zone by name (where `/etc/localtime` links), and sees the
/// time zone a renderer on the browser's thread finds itself.
#[test]
fn a_renderer_process_is_told_this_machines_time_zone_by_name() {
    if std::env::var_os("TZ").is_some() {
        // The user's stands, and is passed on as it is.
        return;
    }
    let zone = shell::renderer_sandbox::linux::renderer_time_zone();
    let read = "return [Intl.DateTimeFormat().resolvedOptions().timeZone, \
        new Date(2026, 0, 1).getTimezoneOffset(), new Date(2026, 6, 1).getTimezoneOffset()]";
    let seen = |args: &[&str]| {
        let mut browser = Browser::start_in(args, &[], std::process::Stdio::null());
        let session = browser.attach();
        browser.navigate(&session, "about:blank");
        let seen = script(&mut browser, &session, read, serde_json::json!([])).unwrap();
        (seen, renderer_pids(&mut browser))
    };
    let (on_a_thread, _) = seen(&["--single-process"]);
    for args in [
        &["--process-per-tab"][..],
        &["--process-per-tab", "--no-sandbox"],
    ] {
        let (seen, pids) = seen(args);
        assert_eq!(seen, on_a_thread, "{args:?}");
        let environment = environment_of(pids[0]);
        let tz = environment
            .iter()
            .find(|(name, _)| name == "TZ")
            .map(|(_, value)| value.clone());
        assert_eq!(tz, zone, "{args:?}");
    }
    if let Some(zone) = zone {
        assert_eq!(on_a_thread[0], zone);
    }
}

/// A short zone name (`CET`), which ICU checks against glibc's offset, is
/// the same zone in a renderer process as on the browser's thread: its
/// offset in winter and in summer, not a zone of no offset.
#[test]
fn a_short_zone_name_is_the_zone_in_a_renderer_process_too() {
    let env = [("TZ", std::ffi::OsStr::new("CET"))];
    let read = "return [Intl.DateTimeFormat().resolvedOptions().timeZone, \
        new Date(2026, 0, 1).getTimezoneOffset(), new Date(2026, 6, 1).getTimezoneOffset()]";
    let seen = |args: &[&str]| {
        let mut browser = Browser::start_in(args, &env, std::process::Stdio::null());
        let session = browser.attach();
        browser.navigate(&session, "about:blank");
        script(&mut browser, &session, read, serde_json::json!([])).unwrap()
    };
    let on_a_thread = seen(&["--single-process"]);
    // By the name ICU gives it (`Europe/Brussels`).
    assert_eq!(
        (&on_a_thread[1], &on_a_thread[2]),
        (&serde_json::json!(-60), &serde_json::json!(-120)),
        "{on_a_thread}"
    );
    assert_eq!(seen(&["--process-per-tab"]), on_a_thread);
}

/// Which namespace of `kind` process `pid` is in (`/proc/<pid>/ns/<kind>`,
/// `user:[4026531837]`).
fn namespace_of(pid: &str, kind: &str) -> String {
    let link = std::fs::read_link(format!("/proc/{pid}/ns/{kind}")).expect("its namespace");
    link.to_string_lossy().into_owned()
}

/// A renderer process in its sandbox is in user and network namespaces of
/// its own, where this machine allows them: its only network interface is
/// loopback, and it has no capability in them; it is the same user. Where
/// the machine doesn't, it is in this process's, as before. Out of its
/// sandbox (`--no-sandbox`), in this process's either way.
#[test]
fn a_renderer_process_has_namespaces_of_its_own_where_the_machine_allows() {
    // Asked of another program, not of machopo's own way of trying: one
    // that fails would have the renderers go without, unseen.
    let allowed = Command::new("unshare")
        .args(["--user", "--net", "true"])
        .status()
        .is_ok_and(|status| status.success());
    assert_eq!(
        shell::renderer_sandbox::linux::namespaces().is_some(),
        allowed
    );
    let ours = |kind| namespace_of("self", kind);
    for (args, own) in [
        (&["--process-per-tab"][..], allowed),
        (&["--process-per-tab", "--no-sandbox"][..], false),
    ] {
        let mut browser = Browser::start_in(args, &[], std::process::Stdio::null());
        let session = browser.attach();
        browser.navigate(&session, "about:blank");
        let pids = renderer_pids(&mut browser);
        assert_eq!(pids.len(), 1, "{args:?}: {pids:?}");
        let pid = pids[0].to_string();
        for kind in ["user", "net"] {
            assert_eq!(
                namespace_of(&pid, kind) != ours(kind),
                own,
                "{args:?}: {kind}"
            );
        }
        // Namespaces it isn't given are this process's.
        for kind in ["pid", "mnt"] {
            assert_eq!(namespace_of(&pid, kind), ours(kind), "{args:?}: {kind}");
        }
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
        let field = |name: &str| {
            status
                .lines()
                .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))
                .map(str::trim)
                .unwrap_or_else(|| panic!("no {name}"))
                .to_owned()
        };
        // The same user (as this process sees it).
        let our_status = std::fs::read_to_string("/proc/self/status").unwrap();
        let our_uid = our_status
            .lines()
            .find_map(|line| line.strip_prefix("Uid:"))
            .unwrap()
            .trim()
            .to_owned();
        assert_eq!(field("Uid"), our_uid, "{args:?}");
        if own {
            for capabilities in ["CapPrm", "CapEff", "CapInh", "CapAmb"] {
                assert_eq!(field(capabilities), "0000000000000000", "{capabilities}");
            }
            let interfaces: Vec<String> = std::fs::read_to_string(format!("/proc/{pid}/net/dev"))
                .unwrap()
                .lines()
                .skip(2)
                .filter_map(|line| Some(line.split_once(':')?.0.trim().to_owned()))
                .collect();
            assert_eq!(interfaces, ["lo"], "{args:?}");
        }
    }
}

/// Where the machine lets no program make user namespaces, a renderer
/// process goes without (in its browser's, as before) and shows its page,
/// and the browser says why once. Made here by a user namespace of the
/// test's own whose processes may make none (where the machine allows
/// one at all; where it doesn't, every other test here is this one).
#[test]
fn a_renderer_process_goes_without_namespaces_where_the_machine_forbids_them() {
    let made = Command::new("unshare")
        .args(["--user", "--map-root-user", "true"])
        .status()
        .is_ok_and(|status| status.success());
    if !made {
        return;
    }
    let forbidding = [
        "unshare",
        "--user",
        "--map-root-user",
        "sh",
        "-c",
        "echo 0 > /proc/sys/user/max_user_namespaces && exec \"$@\"",
        "sh",
    ];
    let log = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("renderer-without-namespaces.log");
    let mut browser = Browser::start_wrapped(
        &forbidding,
        &["--process-per-tab"],
        std::process::Stdio::from(std::fs::File::create(&log).unwrap()),
    );
    let session = browser.attach();
    browser.navigate(&session, "data:text/html,<span id=s>mmm</span>");
    let width = script(
        &mut browser,
        &session,
        "return s.offsetWidth",
        serde_json::json!([]),
    )
    .unwrap();
    assert!(width.as_f64().unwrap() > 0.0, "{width}");
    let pids = renderer_pids(&mut browser);
    assert_eq!(pids.len(), 1, "{pids:?}");
    let (renderer, machopo) = (pids[0].to_string(), browser.child.id().to_string());
    for kind in ["user", "net"] {
        assert_eq!(
            namespace_of(&renderer, kind),
            namespace_of(&machopo, kind),
            "{kind}"
        );
    }
    drop(browser);
    let said = std::fs::read_to_string(&log).unwrap();
    assert_eq!(
        said.matches("without user and network namespaces").count(),
        1,
        "{said}"
    );
}
