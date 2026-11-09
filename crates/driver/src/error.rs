//! A command's failure, as WebDriver answers it: an HTTP status and
//! `{"value": {"error", "message", "stacktrace"}}`. The spec's errors come
//! from the `webdriver` crate's `ErrorStatus`; chromedriver's `tab crashed`
//! and `disconnected`, which the spec doesn't have, are the driver's own:
//! wptrunner records a test as a crash on either.

use std::borrow::Cow;

use hyper::StatusCode;
use serde_json::json;
use webdriver::error::{ErrorStatus, WebDriverError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub status: StatusCode,
    pub error: Cow<'static, str>,
    pub message: String,
}

pub type Result<T> = std::result::Result<T, Failure>;

impl Failure {
    pub fn new(error: ErrorStatus, message: impl Into<String>) -> Self {
        // The crate's `http` is the one hyper uses. It has `unable to
        // capture screen` as 400; the spec's is 500.
        let status = match error {
            ErrorStatus::UnableToCaptureScreen => StatusCode::INTERNAL_SERVER_ERROR,
            _ => error.http_status(),
        };
        Self {
            status,
            error: Cow::Borrowed(error.error_code()),
            message: message.into(),
        }
    }

    /// The tab's renderer has gone (the browser stays): chromedriver's.
    pub fn tab_crashed() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: Cow::Borrowed("tab crashed"),
            message: "the tab's renderer has gone".into(),
        }
    }

    /// The browser has gone: chromedriver's.
    pub fn disconnected() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: Cow::Borrowed("disconnected"),
            message: "machopo has ended".into(),
        }
    }

    /// The answer's body.
    pub fn body(&self) -> String {
        json!({
            "value": {
                "error": self.error,
                "message": self.message,
                "stacktrace": "",
            }
        })
        .to_string()
    }
}

impl From<WebDriverError> for Failure {
    fn from(e: WebDriverError) -> Self {
        Self::new(e.error, e.message.into_owned())
    }
}
