//! A WebDriver server that answers Status, New Session, Delete Session and
//! Set Timeouts, and `unknown command` to everything else: enough for
//! wptrunner to start it, make a session and run WPT's
//! `webdriver/tests/classic/status`. It says `setWindowRect: false`, so the
//! wdspec `session` fixture doesn't set the window's size.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use serde_json::json;
use webdriver::command::{WebDriverCommand, WebDriverMessage};
use webdriver::error::{ErrorStatus, WebDriverError, WebDriverResult};
use webdriver::response::{NewSessionResponse, ValueResponse, WebDriverResponse};
use webdriver::server::{Session, SessionTeardownKind, WebDriverHandler};

#[derive(Default)]
struct Handler {
    sessions: u32,
}

impl WebDriverHandler for Handler {
    fn handle_command(
        &mut self,
        session: &Option<Session>,
        msg: WebDriverMessage,
    ) -> WebDriverResult<WebDriverResponse> {
        match msg.command {
            WebDriverCommand::Status => {
                let ready = session.is_none();
                let message = if ready { "" } else { "a session is running" };
                Ok(WebDriverResponse::Generic(ValueResponse(
                    json!({ "ready": ready, "message": message }),
                )))
            }
            WebDriverCommand::NewSession(_) => {
                self.sessions += 1;
                Ok(WebDriverResponse::NewSession(NewSessionResponse::new(
                    format!("step0-{}", self.sessions),
                    json!({
                        "browserName": "machopo",
                        "browserVersion": "0",
                        "platformName": std::env::consts::OS,
                        "acceptInsecureCerts": false,
                        "pageLoadStrategy": "normal",
                        "setWindowRect": false,
                        "strictFileInteractability": false,
                        "timeouts": { "implicit": 0, "pageLoad": 300000, "script": 30000 },
                        "unhandledPromptBehavior": "dismiss and notify",
                    }),
                )))
            }
            WebDriverCommand::DeleteSession => Ok(WebDriverResponse::DeleteSession),
            WebDriverCommand::SetTimeouts(_) => Ok(WebDriverResponse::Void),
            other => Err(WebDriverError::new(
                ErrorStatus::UnknownCommand,
                format!("not in the prototype: {other:?}"),
            )),
        }
    }

    fn teardown_session(&mut self, _kind: SessionTeardownKind) {}
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut port = 4444;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => port = args.next().and_then(|p| p.parse().ok()).expect("--port N"),
            other => eprintln!("step0-prototype: ignoring {other}"),
        }
    }
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let listener = webdriver::server::start::<_, webdriver::httpapi::VoidWebDriverExtensionRoute>(
        address,
        vec![],
        vec![],
        Handler::default(),
        vec![],
    )
    .expect("listen");
    eprintln!("step0-prototype: listening on {}", listener.socket);
    // `Listener`'s drop joins the server's threads, which run until killed.
    drop(listener);
}
