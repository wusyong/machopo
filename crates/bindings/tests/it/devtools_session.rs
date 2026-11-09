//! A DevTools client of an isolate's inspector, for the tests.

use std::cell::RefCell;
use std::rc::Rc;

use bindings::V8PerIsolateData;
use serde_json::{Value, json};

/// A DevTools session of `data`'s inspector: `call` answers.
pub struct Session {
    answers: Rc<RefCell<Vec<Value>>>,
    next_id: i32,
}

impl Session {
    pub fn attach(data: &mut V8PerIsolateData) -> Self {
        let answers: Rc<RefCell<Vec<Value>>> = Rc::default();
        let sent = answers.clone();
        data.devtools_attach(
            "session".to_owned(),
            Box::new(move |message: String| {
                sent.borrow_mut()
                    .push(serde_json::from_str(&message).unwrap());
            }),
        );
        Self {
            answers,
            next_id: 0,
        }
    }

    pub fn call(&mut self, data: &mut V8PerIsolateData, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        let command = json!({ "id": id, "method": method, "params": params });
        assert!(data.dispatch_devtools_message("session", &command.to_string()));
        let answers = self.answers.borrow();
        let answer = answers.iter().find(|answer| answer["id"] == id).unwrap();
        assert!(answer.get("error").is_none(), "{method}: {answer}");
        answer["result"].clone()
    }
}
