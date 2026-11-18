//! rh: the test command line of the engine. Phase 2 adds --check, --once and scan.

use std::process::ExitCode;

// Print the version, or the usage for anything else
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => {
            println!("{} {}", engine::NAME, engine::VERSION);
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: rh --version");
            ExitCode::from(2)
        }
    }
}
