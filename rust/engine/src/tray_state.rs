//! What the tray icon shows: an overall state and one ring segment per node.

/// The tray after a node check.
#[derive(Debug, Clone, PartialEq)]
pub struct TrayMark {
    /// "ok", "warn" (some dead) or "bad" (all dead).
    pub state: &'static str,
    /// Nodes working, out of total; the text is "{ok} of {n} nodes working" in the person's language.
    pub working: usize,
    pub total: usize,
    /// Per node: "bad" dead, "warn" slower than slow_ms, else "ok".
    pub segs: Vec<&'static str>,
}

// The tray state from test results (tag, ms or None); dead defaults to the nodes without a time. Counted out of all 6
// slots: an empty slot counts as a dead node, so 5 nodes that work read "5 of 6", not "5 of 5" (engine.tray_mark)
pub fn tray_mark(res: &[(String, Option<f64>)], dead: Option<&[String]>, slow_ms: f64) -> TrayMark {
    let dead: Vec<String> = match dead {
        Some(d) => d.to_vec(),
        None => res.iter().filter(|(_, ms)| ms.is_none()).map(|(t, _)| t.clone()).collect(),
    };
    let total = res.len().max(crate::node_manager::SLOTS);
    let working = res.len().saturating_sub(dead.len());
    let state = if working == 0 {
        "bad"
    } else if working < total {
        "warn"
    } else {
        "ok"
    };
    let mut segs: Vec<&'static str> = res
        .iter()
        .map(|(t, ms)| {
            if dead.contains(t) {
                "bad"
            } else if ms.unwrap_or(0.0) > slow_ms {
                "warn"
            } else {
                "ok"
            }
        })
        .collect();
    segs.resize(total, "bad");
    TrayMark { state, working, total, segs }
}
