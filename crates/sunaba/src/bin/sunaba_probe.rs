//! `sunaba`'s own tests' process: the probe's baseline policy and what the
//! arguments before `--` add to it, then the attempt after `--`.
//!
//! ```text
//! sunaba_probe [--read-only PATH]... [--allow NUMBER]... [--fail NUMBER ERRNO]...
//!              [--allow-int NUMBER INDEX VALUE]... [--allow-long NUMBER INDEX VALUE]...
//!              [--fail-masked NUMBER ERRNO INDEX MASK VALUE]... [--thread-first] -- ATTEMPT...
//! ```
//!
//! `--thread-first` has a second thread running when the sandbox is
//! entered, which is refused.
//!
//! On macOS the profile is `(deny default)` and the rules the arguments
//! add, in order:
//!
//! ```text
//! sunaba_probe [--rule RULE]... [--parameter NAME VALUE]... [--profile SOURCE]
//!              [--thread-first] [--again SOURCE] -- ATTEMPT...
//! ```
//!
//! `--profile` is the whole profile in place of that; `--thread-first`
//! has a second thread running when the sandbox is entered, which is
//! inside it all the same; `--again` enters a second time, with that
//! profile, once inside the first.
//!
//! On Windows the sandbox is given by whoever starts the process, so the
//! probe starts itself: under the whole policy, less what the arguments
//! say, and the process it starts lowers itself and makes the attempt.
//!
//! ```text
//! sunaba_probe [--no-policy] [--allow-win32k] [--allow-third-party-modules]
//!              [--no-lower] [--env NAME VALUE]... [--env-remove NAME]...
//!              [--abandon] -- ATTEMPT...
//! ```
//!
//! `--no-policy` starts it with no sandbox, which still inherits only
//! what is named; `--no-lower` leaves the started process as it was
//! started; `--abandon` lets go of the started process and prints whether
//! it runs on (`ok 1`). `{named}` and `{unnamed}` in the attempt are the values of
//! two handles the started process could inherit, of which it is given
//! the first.

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

#[cfg(target_os = "linux")]
fn main() {
    use sunaba::linux::{Arg, Test, probe};

    fn number<T: std::str::FromStr>(word: Option<String>) -> T {
        word.and_then(|word| word.parse().ok()).unwrap_or_else(|| {
            eprintln!("sunaba_probe: a number is missing");
            std::process::exit(2);
        })
    }

    let mut policy = probe::baseline();
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--" => break,
            "--read-only" => {
                policy.read_only(arguments.next().unwrap_or_default());
            }
            "--allow" => {
                policy.allow(&[number(arguments.next())]);
            }
            "--fail" => {
                let call = number(arguments.next());
                policy.fail(call, number(arguments.next()));
            }
            "--allow-int" => {
                let call = number(arguments.next());
                let index = number(arguments.next());
                let value = number(arguments.next());
                policy.allow_when(call, &[Arg::int(index, Test::Equals(value))]);
            }
            "--allow-long" => {
                let call = number(arguments.next());
                let index = number(arguments.next());
                let value = number(arguments.next());
                policy.allow_when(call, &[Arg::long(index, Test::Equals(value))]);
            }
            "--fail-masked" => {
                let call = number(arguments.next());
                let errno = number(arguments.next());
                let index = number(arguments.next());
                let mask = number(arguments.next());
                let value = number(arguments.next());
                let args = [Arg::int(index, Test::MaskedEquals { mask, value })];
                policy.fail_when(call, &args, errno);
            }
            "--thread-first" => {
                // Parked for good: the process ends under it.
                std::thread::spawn(|| {
                    loop {
                        std::thread::park();
                    }
                });
            }
            other => {
                eprintln!("sunaba_probe: no such option: {other}");
                std::process::exit(2);
            }
        }
    }
    probe::run(&policy, arguments);
}

#[cfg(target_os = "macos")]
fn main() {
    use sunaba::macos::{Profile, enter, probe};

    let mut source = String::from("(version 1)\n(deny default)\n");
    let mut parameters = Vec::new();
    let mut again = None;
    let mut thread = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--" => break,
            "--rule" => {
                source.push_str(&arguments.next().unwrap_or_default());
                source.push('\n');
            }
            "--profile" => source = arguments.next().unwrap_or_default(),
            "--parameter" => {
                let name = arguments.next().unwrap_or_default();
                parameters.push((name, arguments.next().unwrap_or_default()));
            }
            "--again" => again = arguments.next(),
            "--thread-first" => {
                // Running before the process enters; it makes the attempt
                // once it is told what it is.
                let (send, receive) = std::sync::mpsc::channel::<Vec<String>>();
                std::thread::spawn(move || {
                    if let Ok(attempt) = receive.recv() {
                        probe::attempt_and_exit(attempt);
                    }
                });
                thread = Some(send);
            }
            other => {
                eprintln!("sunaba_probe: no such option: {other}");
                std::process::exit(2);
            }
        }
    }
    let mut profile = Profile::new(source);
    for (name, value) in parameters {
        profile.parameter(name, value);
    }
    if let Err(error) = enter(&profile) {
        eprintln!("sunaba: {error}");
        std::process::exit(2);
    }
    if let Some(source) = again
        && let Err(error) = enter(&Profile::new(source))
    {
        eprintln!("sunaba: again: {error}");
        std::process::exit(3);
    }
    let attempt: Vec<String> = arguments.collect();
    match thread {
        Some(send) => {
            let _ = send.send(attempt);
            loop {
                std::thread::park();
            }
        }
        None => probe::attempt_and_exit(attempt),
    }
}

#[cfg(windows)]
fn main() {
    use std::os::windows::io::{AsHandle as _, AsRawHandle as _};

    use sunaba::windows::{Command, Policy, lower, probe};

    let mut arguments = std::env::args().skip(1).peekable();
    // The started process: lowers itself as a sandboxed process does,
    // unless told not to, and makes the attempt.
    if arguments.peek().map(String::as_str) == Some("--inside") {
        arguments.next();
        if arguments.peek().map(String::as_str) == Some("--no-lower") {
            arguments.next();
        } else if let Err(error) = lower() {
            eprintln!("sunaba: {error}");
            std::process::exit(2);
        }
        probe::attempt_and_exit(arguments);
    }

    // Its starter: the policy the arguments before `--` leave of the
    // whole one, and this program again under it.
    let mut policy = Some(Policy::new(format!("sunaba.probe.{}", std::process::id())));
    let mut command = Command::new(std::env::current_exe().unwrap_or_default());
    command.arg("--inside");
    // Two handles a child could inherit, of which only the first is
    // named: `{named}` and `{unnamed}` in the attempt are their values.
    let handles = [std::fs::File::open("NUL"), std::fs::File::open("NUL")];
    let mut values = Vec::new();
    let mut abandon = false;
    for handle in handles.iter().flatten() {
        if probe::set_inheritable(handle.as_handle(), true).is_ok() {
            values.push(probe::value(handle.as_raw_handle()).to_string());
        }
    }
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--" => break,
            "--no-policy" => policy = None,
            "--no-lower" => {
                command.arg("--no-lower");
            }
            "--env" => {
                let name = arguments.next().unwrap_or_default();
                command.env(name, arguments.next().unwrap_or_default());
            }
            "--env-remove" => {
                command.env_remove(arguments.next().unwrap_or_default());
            }
            "--abandon" => abandon = true,
            "--allow-win32k" => {
                if let Some(policy) = &mut policy {
                    policy.allow_win32k();
                }
            }
            "--allow-third-party-modules" => {
                if let Some(policy) = &mut policy {
                    policy.allow_third_party_modules();
                }
            }
            other => {
                eprintln!("sunaba_probe: no such option: {other}");
                std::process::exit(2);
            }
        }
    }
    if let Ok(named) = handles[0].as_ref() {
        command.inherit(named.as_handle());
    }
    for word in arguments {
        let word = match (word.as_str(), values.as_slice()) {
            ("{named}", [named, ..]) => named.clone(),
            ("{unnamed}", [_, unnamed]) => unnamed.clone(),
            _ => word,
        };
        command.arg(word);
    }
    match command.spawn(policy.as_ref()) {
        // Whether the process runs on once nothing here has it: `ok 1`
        // when it does.
        Ok(child) if abandon => {
            let running = sunaba::windows::open(child.id());
            std::thread::sleep(std::time::Duration::from_millis(300));
            drop(child);
            std::thread::sleep(std::time::Duration::from_millis(300));
            match running {
                Ok(process) => {
                    println!(
                        "ok {}",
                        u8::from(sunaba::windows::running(process.as_handle()))
                    );
                }
                Err(error) => println!("error {}", error.raw_os_error().unwrap_or(0)),
            }
        }
        Ok(mut child) => {
            let status = child.wait().ok().and_then(|status| status.code());
            std::process::exit(status.unwrap_or(3));
        }
        Err(error) => {
            eprintln!("sunaba: not started: {error}");
            std::process::exit(2);
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn main() {}
