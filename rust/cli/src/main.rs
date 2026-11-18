//! realityhop-cli: the engine's command line, for tests in a VM. It uses the data folder in REALITYHOP_DATA (a debug
//! build: RealityHop-dev in the user's local app data), never an installed Reality Hop's.
//!   realityhop-cli --check               test the nodes in use through the live tunnels (engine.py --check)
//!   realityhop-cli scan [--top N] [--apply N]   rank the nodes in use and the fastest candidates (engine.py)
//!   realityhop-cli --once                one watcher check, fixing what is broken
//!   realityhop-cli --watch [--logon]     the watcher, without a tray icon
//!   realityhop-cli --version

use engine::activity_log::say;
use engine::node_manager::{apply, check_all, inuse, scan};
use engine::paths::Paths;
use engine::router_ssh::RouterSsh;
use engine::settings::pc_only;
use engine::watcher::{Watch, WatchHooks, WatchState, watch_cycle, watch_lock, watch_loop};
use serde_json::{Value, json};
use std::io::{BufRead, IsTerminal, Write};
use std::process::ExitCode;

/// The watcher's tray and notifications, on the console.
struct Console;

impl WatchHooks for Console {
    // Print the notification
    fn notify(&self, text: &str) {
        println!("notification: {text}");
    }

    // Print the tray state
    fn show(&self, state: &str, tip: &str, segs: &[String]) {
        println!("tray: {state} [{}] {tip}", segs.join(" "));
    }

    // There is no window
    fn window_open(&self) -> bool {
        false
    }
}

// The value after a flag, as a number
fn number(args: &[String], flag: &str) -> Result<Option<usize>, String> {
    match args.iter().position(|a| a == flag) {
        None => Ok(None),
        Some(i) => {
            let v = args.get(i + 1).ok_or(format!("{flag} needs a number"))?;
            v.parse().map(Some).map_err(|_| format!("{flag}: not a number: {v}"))
        }
    }
}

// Test the nodes in use through the live tunnels and print the table (engine.py --check)
fn check(paths: &Paths) -> Result<(), String> {
    let mut nodes = inuse(paths)?;
    say(&format!("checking the {} nodes in use through the live tunnels...", nodes.len()));
    let mut ssh = RouterSsh { paths };
    if pc_only(paths) {
        engine::node_tests::geolocate(paths, &mut nodes);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs_f64();
        engine::clash_api::check_pc(paths, &mut nodes, false, now).map_err(|_| "not a JSON object".to_string())?;
        for n in nodes.iter_mut() {
            let (ms, why) = (n["pc_ms"].clone(), n["pc_why"].clone());
            n["phone_ms"] = ms;
            n["phone_why"] = why;
        }
        for l in engine::node_tables::status_table(&nodes, &json!({"service": false, "now": null}), &Value::Null) {
            say(&l);
        }
        return Ok(());
    }
    let (ph, pc) = check_all(paths, &mut ssh, &mut nodes)?;
    let ph = json!({"service": ph.service, "now": ph.now, "cfg": ph.cfg});
    let pc = pc.map_or(Value::Null, |p| json!({"now": p.now, "exit": p.exit}));
    for l in engine::node_tables::status_table(&nodes, &ph, &pc) {
        say(&l);
    }
    let dead: Vec<&str> = nodes.iter().filter(|n| n["phone_ms"].is_null()).filter_map(|n| n["tag"].as_str()).collect();
    if !dead.is_empty() {
        say(&format!("\ndead on the router side: {}  ->  replace them in the Reality Hop window", dead.join(", ")));
    }
    Ok(())
}

// Rank the nodes in use and the fastest candidates; apply the best N when asked (engine.py)
fn ranking(paths: &Paths, args: &[String]) -> Result<(), String> {
    let mut ssh = RouterSsh { paths };
    let nodes = scan(paths, &mut ssh, number(args, "--top")?)?;
    let ok = nodes.iter().any(|n| n["ok"] == true);
    if let Some(k) = number(args, "--apply")? {
        return apply(paths, &mut ssh, &nodes, k);
    }
    if ok && std::io::stdin().is_terminal() {
        print!("apply the best N to both configs and deploy? (number, or Enter to skip): ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        let _ = std::io::stdin().lock().read_line(&mut line);
        if let Ok(k) = line.trim().parse::<usize>()
            && k > 0
        {
            apply(paths, &mut ssh, &nodes, k)?;
        }
    }
    Ok(())
}

// One watcher check
fn once(paths: &Paths) -> Result<(), String> {
    let mut st = WatchState::default();
    let out = watch_cycle(paths, &mut RouterSsh { paths }, &mut st)?;
    if let Some((state, text, at)) = &st.last {
        println!("tray: {state} {text} {at}");
    }
    println!("{}", out.as_deref().unwrap_or("nothing changed"));
    Ok(())
}

// The watcher until Ctrl+C
fn watch(paths: &Paths, args: &[String]) -> Result<(), String> {
    let _lock = watch_lock().ok_or("the watcher is already running")?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let cmd = vec![exe.to_string_lossy().into_owned(), "--watch".into()];
    watch_loop(paths, &Watch::new(), &Console, &cmd, args.iter().any(|a| a == "--logon"));
    Ok(())
}

// Run one command; an error ends the program with its message, as engine.py does
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--version") {
        println!("{} {}", engine::NAME, engine::VERSION);
        return ExitCode::SUCCESS;
    }
    let Some(paths) = Paths::from_env() else {
        eprintln!("set REALITYHOP_DATA to the data folder to use");
        return ExitCode::from(2);
    };
    let r = match args.first().map(String::as_str) {
        Some("--check") => check(&paths),
        Some("scan") => ranking(&paths, &args[1..]),
        Some("--once") => once(&paths),
        Some("--watch") => watch(&paths, &args[1..]),
        _ => {
            eprintln!(
                "usage: realityhop-cli --check | scan [--top N] [--apply N] | --once | --watch [--logon] | --version"
            );
            return ExitCode::from(2);
        }
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
