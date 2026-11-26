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

#[cfg(not(target_os = "linux"))]
fn main() {}
