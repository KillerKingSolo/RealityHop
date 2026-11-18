//! Reality Hop engine. The Rust port of src/engine.py, install.py, report.py and i18n.py.
//! Every pure function is checked against the Python output in rust/parity (tests/parity.rs).

pub mod clash_api;
pub mod node;
pub mod node_feed;
pub mod node_tables;
pub mod powershell;
pub mod python_compat;
pub mod python_json;
pub mod settings;
pub mod singbox_config;
pub mod speed_test;
pub mod validate;
pub mod vault;
pub mod version;

/// File, folder and repository name.
pub const APP: &str = "RealityHop";
/// The name people see.
pub const NAME: &str = "Reality Hop";
/// GitHub owner/repo: app updates come from its Releases.
pub const REPO: &str = "KillerKingSolo/RealityHop";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
