//! Reality Hop engine. The Rust port of src/engine.py, install.py, report.py and i18n.py.
//! Every pure function is checked against the Python output in rust/parity (tests/parity.rs).

/// File, folder and repository name.
pub const APP: &str = "RealityHop";
/// The name people see.
pub const NAME: &str = "Reality Hop";
/// GitHub owner/repo: app updates come from its Releases.
pub const REPO: &str = "KillerKingSolo/RealityHop";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
