//! Download speed of nodes: the floor a node must reach, the ranking, and curl's speed test output.

/// The fastest node speed seen, as this-pc.json keeps it: (when, Mbps).
pub type TopMbps = Option<(f64, f64)>;

// The speed a node must reach: min_mbps, or 70% of the fastest node of the last 24 hours when that is slower, never
// under half of min_mbps (engine.floor)
pub fn floor(min_mbps: i64, top: TopMbps, now: f64) -> i64 {
    match top {
        Some((t, top)) if top != 0.0 && now - t < 86400.0 => (min_mbps / 2).max(min_mbps.min((top * 0.7) as i64)),
        _ => min_mbps,
    }
}

/// Sort key of a working node: (group, value); group 0 fast enough by latency, 1 untested by latency, 2 slow by
/// speed, fastest first.
pub fn speed_rank(min_mbps: i64, floor: i64, mbps: Option<f64>, ms: Option<f64>) -> (u8, f64) {
    let ms = ms.filter(|m| *m != 0.0).unwrap_or(9e9);
    match mbps {
        Some(m) if min_mbps != 0 => {
            if m >= floor as f64 {
                (0, ms)
            } else {
                (2, -m)
            }
        }
        _ => (1, ms),
    }
}

/// One speed test as curl reported it.
#[derive(Debug, PartialEq)]
pub enum Speed {
    /// Mbps, timed from the first byte; 0 when nothing came through the node.
    Mbps(i64),
    /// The test file answered with an HTTP error or the output was unreadable: says nothing about the node.
    Unknown,
}

// curl's -w ' %{http_code} %{size_download} %{time_total} %{time_starttransfer}' output as a speed (engine.mbps)
pub fn parse_mbps(stdout: &str) -> Speed {
    let words: Vec<&str> = stdout.split(crate::python_compat::is_space).filter(|w| !w.is_empty()).collect();
    if words.len() < 4 {
        return Speed::Unknown;
    }
    let w = &words[words.len() - 4..];
    let num = |s: &str| s.parse::<f64>().ok();
    let (Some(size), Some(total), Some(start)) = (num(w[1]), num(w[2]), num(w[3])) else { return Speed::Unknown };
    match w[0] {
        "000" => Speed::Mbps(0),
        "200" if start == 0.0 => Speed::Mbps(0),
        "200" => Speed::Mbps((size * 8.0 / 1e6 / (total - start).max(0.1)).round_ties_even() as i64),
        _ => Speed::Unknown,
    }
}
