//! sing-box's Clash API on 127.0.0.1:9090: reading its answers.

use serde_json::Value;

/// What Python raises on an answer that is JSON but not an object.
#[derive(Debug, PartialEq, Eq)]
pub struct NotAnObject;

// A /delay answer as (ms, why not): {"delay": n} -> (n, ""), else (None, its message)
pub fn parse_delay(text: &str) -> Result<(Option<Value>, Value), NotAnObject> {
    let Ok(d) = serde_json::from_str::<Value>(text) else {
        return Ok((None, Value::from("no response")));
    };
    let Value::Object(m) = d else { return Err(NotAnObject) };
    Ok(match m.get("delay") {
        Some(v) => (Some(v.clone()), Value::from("")),
        None => (None, m.get("message").cloned().unwrap_or_else(|| Value::from("failed"))),
    })
}
