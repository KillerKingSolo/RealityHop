//! Lines as the window shows them: a capital first letter, names in lower case kept

use engine::translations::cap_first;

#[test]
fn a_shown_line_starts_with_a_capital() {
    assert_eq!(cap_first("watcher started: node check every 60 min"), "Watcher started: node check every 60 min");
    assert_eq!(cap_first("done (58 s)"), "Done (58 s)");
    assert_eq!(cap_first("feed: 194 candidates"), "Feed: 194 candidates");
    assert_eq!(cap_first("error: no answer"), "Error: no answer");
    assert_eq!(cap_first("éxito"), "Éxito");
    assert_eq!(cap_first(""), "");
}

#[test]
fn names_tables_and_counts_stay_as_they_are() {
    for s in [
        "rh-core: sing-box 1.15.0",
        "sing-box 1.15.0-alpha.13 (testing) installed as rh-core.exe",
        "vless-1: node-2 -> node-3",
        "macOS 15 or newer",
        "iPhone",
        "19 of 50 pass",
        "  1 195.211.98.43",
        "\n # server",
        "PC tunnel: running",
    ] {
        assert_eq!(cap_first(s), s);
    }
}
