//! Where Reality Hop keeps its files. Everything lives in one data folder (engine.set_paths).

use std::path::{Path, PathBuf};

/// Program file names end in .exe on Windows.
pub const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

/// Every file in the data folder.
#[derive(Debug, Clone)]
pub struct Paths {
    pub data: PathBuf,
    pub pc_dir: PathBuf,
    pub router_dir: PathBuf,
    pub pc_cfg: PathBuf,
    pub router_cfg: PathBuf,
    /// Problem reports with everything personal taken out.
    pub reports: PathBuf,
    pub nodes_txt: PathBuf,
    /// The last node list download that worked, for when no source answers.
    pub feed_cache: PathBuf,
    /// Every node that ever passed a test: the rescue pool.
    pub vault: PathBuf,
    /// Nodes that just failed in the running PC tunnel.
    pub tunnel_fails: PathBuf,
    /// Nodes that broke PIA's connection again and again (pass the short tests, fail long connections).
    pub pia_breaks: PathBuf,
    pub logs: PathBuf,
    pub core_log: PathBuf,
    pub tray_icons: PathBuf,
    /// The official sing-box under its own name: runs the PC tunnel.
    pub core: PathBuf,
    /// The SSH key the setup makes for the router.
    pub router_key: PathBuf,
    /// The router's pinned SSH host key.
    pub known_hosts: PathBuf,
    pub this_pc: PathBuf,
    pub settings: PathBuf,
    pub history: PathBuf,
    pub last: PathBuf,
    pub tray_last: PathBuf,
    pub tray_gui: PathBuf,
}

impl Paths {
    // Every path inside one data folder
    pub fn new(data: &Path) -> Paths {
        let d = |name: &str| data.join(name);
        let (pc_dir, router_dir, logs) = (d("PC"), d("Router"), d("logs"));
        Paths {
            pc_cfg: pc_dir.join("pc-config.json"),
            router_cfg: router_dir.join("router-config.json"),
            core: pc_dir.join(format!("rh-core{EXE}")),
            core_log: logs.join("core.log"),
            reports: d("anonymous-logs"),
            nodes_txt: d("nodes.txt"),
            feed_cache: d("feed-cache.txt"),
            vault: d("vault.json"),
            tunnel_fails: d("tunnel-fails.json"),
            pia_breaks: d("pia-breaks.json"),
            tray_icons: d("tray-icons"),
            router_key: d("router_ed25519"),
            known_hosts: d("known_hosts"),
            this_pc: d("this-pc.json"),
            settings: d("settings.json"),
            history: d("history.json"),
            last: d("last.json"),
            tray_last: d("tray.json"),
            tray_gui: d("tray-window.json"),
            data: data.to_path_buf(),
            pc_dir,
            router_dir,
            logs,
        }
    }

    // The data folder to use when nothing else says: REALITYHOP_DATA if set; a development build never touches a
    // real install and uses RealityHop-dev in the user's local app data
    pub fn from_env() -> Option<Paths> {
        if let Some(d) = std::env::var_os("REALITYHOP_DATA").filter(|d| !d.is_empty()) {
            return Some(Paths::new(Path::new(&d)));
        }
        if cfg!(debug_assertions) {
            let base = std::env::var_os(if cfg!(windows) { "LOCALAPPDATA" } else { "HOME" })?;
            return Some(Paths::new(&PathBuf::from(base).join(format!("{}-dev", crate::APP))));
        }
        None
    }
}
