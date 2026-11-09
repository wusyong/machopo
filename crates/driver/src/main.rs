//! `machopodriver`: machopo's WebDriver server. `machopodriver --help`
//! describes the options.

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

use std::net::{Ipv4Addr, TcpListener};
use std::path::PathBuf;
use std::process::ExitCode;

const DEFAULT_PORT: u16 = 4444;

const USAGE: &str = "usage: machopodriver [--port N] [--binary MACHOPO] [--binary-arg ARG]...";

fn help() -> String {
    format!(
        "\
machopodriver: machopo's WebDriver (classic) server.

{USAGE}

Serves WebDriver on 127.0.0.1. Each session starts a machopo of its own, with
--automation-pipe (nothing shows), and ends it with the session.

Options:
  --port N            The port (default {DEFAULT_PORT}); 0 picks a free one, which
                      it prints.
  --binary MACHOPO        The machopo to start (default: the machopo next to machopodriver).
  --binary-arg ARG    An argument for machopo, after --automation-pipe; once per
                      argument, e.g. --binary-arg --test-fonts.
  -h, --help          Print this help.

Environment:
  RUST_LOG            Log filter (default: info). machopodriver=debug logs every
                      request and answer."
    )
}

#[derive(Debug, PartialEq)]
enum Command {
    Help,
    Run {
        port: u16,
        binary: Option<PathBuf>,
        binary_args: Vec<String>,
    },
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut port = DEFAULT_PORT;
    let mut binary = None;
    let mut binary_args = Vec::new();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--port" => {
                let v = value("--port")?;
                port = v
                    .parse()
                    .map_err(|_| format!("--port wants a port number, not {v:?}"))?;
            }
            "--binary" => binary = Some(PathBuf::from(value("--binary")?)),
            "--binary-arg" => binary_args.push(value("--binary-arg")?),
            "-h" | "--help" => return Ok(Command::Help),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(Command::Run {
        port,
        binary,
        binary_args,
    })
}

/// `machopo` next to this program.
fn default_binary() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.with_file_name(format!("machopo{}", std::env::consts::EXE_SUFFIX))
}

fn main() -> ExitCode {
    let (port, binary, binary_args) = match parse_args(std::env::args().skip(1)) {
        Ok(Command::Run {
            port,
            binary,
            binary_args,
        }) => (port, binary.unwrap_or_else(default_binary), binary_args),
        Ok(Command::Help) => {
            println!("{}", help());
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("machopodriver: {e}\n\n{USAGE}\n\nRun machopodriver --help for the details.");
            return ExitCode::from(2);
        }
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("machopodriver: listening on port {port}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let options = driver::Options {
        binary,
        binary_args,
    };
    match driver::start(listener, options) {
        Ok(server) => {
            println!("machopodriver listening on {}", server.addr());
            server.join();
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("machopodriver: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Command, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn arguments() {
        assert_eq!(
            parse(&[]),
            Ok(Command::Run {
                port: DEFAULT_PORT,
                binary: None,
                binary_args: vec![],
            })
        );
        assert_eq!(
            parse(&[
                "--port",
                "0",
                "--binary",
                "k",
                "--binary-arg",
                "--test-fonts",
                "--binary-arg",
                "--host-resolver-rules",
                "--binary-arg",
                "MAP * 127.0.0.1",
            ]),
            Ok(Command::Run {
                port: 0,
                binary: Some("k".into()),
                binary_args: vec![
                    "--test-fonts".into(),
                    "--host-resolver-rules".into(),
                    "MAP * 127.0.0.1".into(),
                ],
            })
        );
        assert_eq!(parse(&["-h"]), Ok(Command::Help));
        assert!(parse(&["--port", "x"]).unwrap_err().contains("port number"));
        assert!(parse(&["--port"]).unwrap_err().contains("needs a value"));
        assert!(parse(&["--fly"]).unwrap_err().contains("unknown argument"));
    }

    #[test]
    fn help_mentions_every_option() {
        let help = help();
        for option in ["--port", "--binary", "--binary-arg", "--help", "RUST_LOG"] {
            assert!(help.contains(option), "{option}");
        }
    }
}
