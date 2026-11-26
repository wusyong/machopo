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

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {}
