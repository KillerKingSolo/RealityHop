//! Reality Hop engine. The Rust port of src/engine.py, install.py, report.py and i18n.py.
//! Every pure function is checked against the Python output in rust/parity (tests/parity.rs).

pub mod activity_log;
pub mod app_install;
pub mod clash_api;
pub mod data_files;
pub mod downloads;
pub mod history;
pub mod installer;
pub mod macos;
pub mod macos_system;
pub mod node;
pub mod node_feed;
pub mod node_location;
pub mod node_manager;
pub mod node_tables;
pub mod node_tests;
pub mod paths;
pub mod pc_tunnel;
pub mod pia;
pub mod pia_split_tunnel;
pub mod powershell;
pub mod problem_report;
pub mod problem_report_file;
pub mod process;
pub mod processes_and_ports;
pub mod python_compat;
pub mod python_json;
pub mod router;
pub mod router_actions;
pub mod router_ssh;
pub mod settings;
pub mod setup_steps;
pub mod singbox_config;
pub mod speed_test;
pub mod startup_tasks;
pub mod translations;
pub mod tray_state;
pub mod validate;
pub mod vault;
pub mod version;
pub mod watcher;
pub mod windows_registry;
pub mod windows_system;
pub mod windows_tasks;

/// File, folder and repository name.
pub const APP: &str = "RealityHop";
/// The name people see.
pub const NAME: &str = "Reality Hop";
/// GitHub owner/repo: app updates come from its Releases.
pub const REPO: &str = "KillerKingSolo/RealityHop";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
