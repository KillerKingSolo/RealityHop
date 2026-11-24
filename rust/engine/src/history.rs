//! history.json: per-node tunnel ping and events (drops, replacements, restarts) for the window's graphs, 48 h kept.

use crate::data_files::{jload, jsave};
use serde_json::{Map, Value, json};
use std::path::Path;
use std::sync::Mutex;

/// One write at a time: the watcher, the window and the Clear button all change history.json in one process.
static WRITE: Mutex<()> = Mutex::new(());

/// How long history is kept, in seconds.
const KEEP: f64 = 48.0 * 3600.0;

// The history as {pings: {server: [[t, ms]]}, events: [[t, kind, text]]} (engine.hist_load)
pub fn load(path: &Path) -> Map<String, Value> {
    let h = jload(path);
    let mut out = Map::new();
    out.insert("pings".into(), h.get("pings").cloned().unwrap_or_else(|| json!({})));
    out.insert("events".into(), h.get("events").cloned().unwrap_or_else(|| json!([])));
    out
}

// Whether an entry [t, ...] is newer than cut
fn newer(x: &Value, cut: f64) -> bool {
    x.get(0).and_then(Value::as_f64).is_some_and(|t| t > cut)
}

// Add pings and an event at time now, drop what is older than 48 h, keep at most 2000 events (engine.hist_add)
pub fn add(h: &mut Map<String, Value>, pings: &[(String, Option<i64>)], event: Option<(&str, &str)>, now: f64) {
    let t = now as i64;
    let p = h.entry("pings").or_insert_with(|| json!({}));
    if let Some(p) = p.as_object_mut() {
        for (server, ms) in pings {
            if let Some(list) = p.entry(server.clone()).or_insert_with(|| json!([])).as_array_mut() {
                list.push(json!([t, ms]));
            }
        }
    }
    if let Some((kind, text)) = event
        && let Some(e) = h.entry("events").or_insert_with(|| json!([])).as_array_mut()
    {
        e.push(json!([t, kind, text]));
    }
    let cut = now - KEEP;
    let pings: Map<String, Value> = h["pings"]
        .as_object()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(k, v)| {
            let kept: Vec<Value> = v.as_array()?.iter().filter(|x| newer(x, cut)).cloned().collect();
            (!kept.is_empty()).then(|| (k, Value::Array(kept)))
        })
        .collect();
    let events: Vec<Value> =
        h["events"].as_array().cloned().unwrap_or_default().into_iter().filter(|e| newer(e, cut)).collect();
    let skip = events.len().saturating_sub(2000);
    h.insert("pings".into(), Value::Object(pings));
    h.insert("events".into(), Value::Array(events.into_iter().skip(skip).collect()));
}

// Load, add, prune and save; a failure only costs the graph point (engine.hist_add)
pub fn record(path: &Path, pings: &[(String, Option<i64>)], event: Option<(&str, &str)>, now: f64) {
    let _w = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = load(path);
    add(&mut h, pings, event, now);
    let _ = jsave(path, &Value::Object(h), None);
}

// Remove the events of the last 24 hours (the chart's Clear button); older events and node pings stay
pub fn clear_day(path: &Path, now: f64) -> Result<(), String> {
    let _w = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = load(path);
    let cut = now - 24.0 * 3600.0;
    let kept: Vec<Value> =
        h["events"].as_array().cloned().unwrap_or_default().into_iter().filter(|e| !newer(e, cut)).collect();
    h.insert("events".into(), Value::Array(kept));
    jsave(path, &Value::Object(h), None).map_err(|e| e.to_string())
}

// How long each PIA drop lasted (seconds), oldest first: a drop to the next "back". A disconnect by hand, a drop with
// no return yet and one over an hour (the app was off, so the return was not seen) are not counted
pub fn drop_lengths(events: &[Value]) -> Vec<f64> {
    let (mut out, mut open) = (Vec::new(), None);
    for e in events {
        let t = e.get(0).and_then(Value::as_f64).unwrap_or(0.0);
        match e.get(1).and_then(Value::as_str) {
            Some("drop") => open = (e.get(2).and_then(Value::as_str) != Some("PIA disconnected")).then_some(t),
            Some("back") => {
                if let Some(d) = open.take().filter(|d| t - d <= 3600.0) {
                    out.push(t - d);
                }
            }
            _ => {}
        }
    }
    out
}
