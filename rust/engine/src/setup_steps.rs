//! The PC part of the setup: the data folder, the two configs from the templates, sing-box, the first nodes, the PC
//! tunnel, and the check of the VPN app (engine.prepare, dns_apply, api_lockdown, setup_pc, vpn_state, vpn_check).

use crate::activity_log::{dbg, say};
use crate::data_files::{Newlines, jsave, read_json, write_text};
use crate::paths::Paths;
use crate::router::Ssh;
use crate::settings::{self, Settings, router_only};
use serde_json::{Value, json};
use std::time::Duration;

/// The PC tunnel's config template (resources/pc-config.json).
pub const PC_TEMPLATE: &str = include_str!("../../resources/pc-config.json");
/// The router's config template (resources/router-config.json).
pub const ROUTER_TEMPLATE: &str = include_str!("../../resources/router-config.json");

/// PC/README.txt.
pub const PC_README: &str = r"Reality Hop: PC tunnel files

pc-config.json   the sing-box config of the PC tunnel: the 6 nodes (vless-1 to vless-6), the SOCKS5 proxy on
                 127.0.0.1:2080, DNS (Quad9 through the tunnel) and the local control API on 127.0.0.1:9090
rh-core.exe      sing-box from its official releases (github.com/SagerNet/sing-box), under Reality Hop's own name

You can read and edit pc-config.json; press Restart tunnel in the Reality Hop window to apply it. Reality Hop rewrites
the vless-1 to vless-6 entries whenever it replaces nodes. The logs are in ..\logs.
Do not share pc-config.json: it holds node keys and the local API secret. For a problem report use
Report a problem in the window; it takes everything personal out.
";

/// Router/README.txt.
pub const ROUTER_README: &str = r"Reality Hop: router tunnel files (GL.iNet, only used with router features on)

router-config.json   the sing-box config that Push to router sends to the router (/etc/sing-box/config.json); written
                     for GL.iNet's sing-box 1.9, converted on the push when the router runs sing-box 1.12 or newer
nft-devices.sh       the firewall rule sent to the router: it diverts only the chosen devices into sing-box
heal.sh              the router's self-repair, run there by cron every 30 minutes

Press Push to router in the Reality Hop window to apply changes. Do not share router-config.json: it holds node keys
and the router's local API secret.
";

// n random bytes from the system's secure random source, as hex (secrets.token_hex)
pub fn token_hex(n: usize) -> std::io::Result<String> {
    let mut b = vec![0u8; n];
    #[cfg(windows)]
    {
        use windows_sys::Win32::Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom};
        let len = u32::try_from(n).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        let status =
            unsafe { BCryptGenRandom(std::ptr::null_mut(), b.as_mut_ptr(), len, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
        if status != 0 {
            return Err(std::io::Error::other(format!("BCryptGenRandom failed: {status:#x}")));
        }
    }
    #[cfg(not(windows))]
    {
        use std::io::Read;
        std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    }
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

// True once the setup finished on this PC (engine.is_setup)
pub fn is_setup(paths: &Paths) -> bool {
    settings::this_pc(paths).get("setup_done").is_some_and(settings::py_truthy)
}

// The data folder, the two configs from the templates (each with a fresh random API secret), sing-box from the
// official releases. Leaves anything that already exists alone (engine.prepare)
pub fn prepare(paths: &Paths) -> Result<(), String> {
    let io = |e: std::io::Error| e.to_string();
    for d in [&paths.pc_dir, &paths.router_dir, &paths.logs] {
        std::fs::create_dir_all(d).map_err(io)?;
    }
    if !cfg!(windows) {
        crate::macos_system::lockdown(&paths.data);
        if paths.core.exists() {
            // sing-box downloaded before 1.7: signed with Reality Hop's id now
            crate::macos_system::core_sign(&paths.core, say);
        }
    }
    for (template, dst) in [(PC_TEMPLATE, &paths.pc_cfg), (ROUTER_TEMPLATE, &paths.router_cfg)] {
        if dst.exists() {
            continue;
        }
        let mut c: Value = serde_json::from_str(template).map_err(|e| e.to_string())?;
        c["experimental"]["clash_api"]["secret"] = json!(token_hex(16).map_err(io)?);
        if dst == &paths.pc_cfg {
            c["log"]["output"] = json!(paths.core_log.to_string_lossy());
        }
        jsave(dst, &c, Some(2)).map_err(io)?;
    }
    dns_apply(paths);
    write_text(&paths.pc_dir.join("README.txt"), PC_README, Newlines::Platform).map_err(io)?;
    write_text(&paths.router_dir.join("README.txt"), ROUTER_README, Newlines::Platform).map_err(io)?;
    if !paths.core.exists() {
        crate::pc_tunnel::core_update(paths, true)?;
    }
    api_lockdown(paths);
    Ok(())
}

// Both configs use the name server from Settings: Quad9, or Cloudflare for France, which Quad9 stops serving on 24
// November 2026 after a Paris court order. From the next tunnel restart and the next push. True when a config changed
// (engine.dns_apply)
pub fn dns_apply(paths: &Paths) -> bool {
    let want = Settings::read(paths).map(|s| s.dns).unwrap_or_default();
    let want = if want.is_empty() { "quad9".to_string() } else { want };
    let mut changed = false;
    for path in [&paths.pc_cfg, &paths.router_cfg] {
        let Ok(mut c) = read_json(path) else { continue };
        if crate::singbox_config::dns_apply(&mut c, &want) && jsave(path, &c, Some(2)).is_ok() {
            changed = true;
        }
    }
    if changed {
        let ip = settings::DNS_IPS.iter().find(|(k, _)| *k == want).map_or("", |(_, ip)| ip);
        say(&format!("name server: {want} ({ip})"));
    }
    changed
}

// Two pc-config.json fixes, each checked by rh-core first; from the next tunnel restart. Configs made before 1.1.0 let
// a script on any web page reach the control API (it still needed the secret): no web page at all now. Up to 1.4.1
// rh-core checked certificates against the Windows root store, which makes Windows add root certificates on a fresh PC
// (antivirus sandboxes flag that): sing-box's own Mozilla list instead (engine.api_lockdown)
pub fn api_lockdown(paths: &Paths) {
    let Ok(mut c) = read_json(&paths.pc_cfg) else { return };
    if c.pointer("/experimental/clash_api").is_none() || !paths.core.exists() {
        return;
    }
    let fix = crate::singbox_config::api_lockdown(&mut c, crate::pc_tunnel::store_ok(paths));
    if fix.is_empty() {
        return;
    }
    let mut tmp = paths.pc_cfg.as_os_str().to_owned();
    tmp.push(".check.json");
    let tmp = std::path::PathBuf::from(tmp);
    if jsave(&tmp, &c, Some(2)).is_err() {
        return;
    }
    let dir = paths.core.parent().unwrap_or(&paths.data);
    let args = [paths.core.as_os_str(), "check".as_ref(), "-c".as_ref(), tmp.as_os_str()];
    let ok = crate::process::run(&args, None, Duration::from_secs(30), Some(dir)).is_ok_and(|o| o.code == Some(0));
    if ok && std::fs::rename(&tmp, &paths.pc_cfg).is_ok() {
        say(&format!("{} (from the next tunnel restart)", fix.join("; ")));
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
}

// PIA read from its own settings file: (found, what is set wrong). Other VPN apps cannot be checked (engine.vpn_state)
pub fn vpn_state(paths: &Paths) -> (bool, Vec<&'static str>) {
    let st = match crate::data_files::read_json(std::path::Path::new(crate::pia_split_tunnel::PIA_SETTINGS)) {
        Ok(v @ Value::Object(_)) => v,
        _ => return (false, Vec::new()),
    };
    dbg(|| {
        let keys =
            ["method", "protocol", "remotePortTCP", "proxyType", "proxyEnabled", "killswitch", "splitTunnelEnabled"];
        let shown: Vec<String> = keys.iter().map(|k| format!("{k}={}", st.get(*k).unwrap_or(&Value::Null))).collect();
        let rules = st.get("splitTunnelRules").and_then(Value::as_array).map_or(0, Vec::len);
        format!("PIA settings: {} split tunnel rules: {rules}", shown.join(" "))
    });
    let mac = (!cfg!(windows)).then(|| {
        let kill_on = st.get("killswitch").and_then(Value::as_str).unwrap_or("auto") != "off";
        let dns_own = !st.get("overrideDNS").and_then(Value::as_str).unwrap_or("pia").is_empty();
        crate::pia::MacChecks {
            bypass_ok: !kill_on || crate::pia_split_tunnel::pia_bypass(paths, &[], true),
            dns_proxies: dns_own && !dns_proxies().is_empty(),
        }
    });
    (true, crate::pia::wrong_settings(&st, mac.as_ref()))
}

// macOS: DNS servers that are a local DNS app (NextDNS, AdGuard and the like); PIA blocks them while it is connected
// (macos.dns_proxies)
pub fn dns_proxies() -> Vec<String> {
    crate::process::run(&["/usr/sbin/scutil", "--dns"], None, Duration::from_secs(10), None)
        .map(|o| crate::macos::dns_proxies(&o.stdout))
        .unwrap_or_default()
}

// One line about the VPN app, in English (engine.vpn_check)
pub fn vpn_check(paths: &Paths) -> String {
    let (found, wrong) = vpn_state(paths);
    crate::pia::vpn_check(found, &wrong, |s, kw| {
        if kw.is_empty() { s.to_string() } else { crate::translations::format(s, kw).unwrap_or_else(|| s.to_string()) }
    })
}

// The PC part of the setup: configs, sing-box, the first 6 working nodes, the PC tunnel running. One line per step
// (engine.setup_pc)
pub fn setup_pc(paths: &Paths, ssh: &mut impl Ssh) -> Result<Vec<String>, String> {
    use crate::node_manager::{best_first, scan, write_configs, write_txt};
    let mut res = Vec::new();
    prepare(paths)?;
    let channel = Settings::read(paths).map(|s| s.singbox).unwrap_or_default();
    let ver = crate::pc_tunnel::core_version(paths).unwrap_or_else(|| "MISSING".into());
    res.push(format!("rh-core: sing-box {ver} (official releases, {channel} channel, updated daily)"));
    let have = crate::singbox_config::nodes_of(&paths.pc_cfg).map_err(|e| e.to_string())?.len();
    if have == 0 {
        say(
            "finding the first nodes: downloading the public node list and testing the fastest candidates on this PC...",
        );
        let nodes = scan(paths, ssh, None)?;
        let best: Vec<Value> = best_first(paths, &nodes).into_iter().take(6).collect();
        if best.len() < 2 {
            return Err(
                "fewer than 2 working nodes found; check that this PC has internet and press Install again".into()
            );
        }
        write_configs(paths, &best)?;
        write_txt(paths, &nodes, &best)?;
        let ms = crate::python_json::py_str(best[0].get("ms").unwrap_or(&Value::Null));
        res.push(format!("nodes: the {} fastest working ones are in use (best {ms} ms)", best.len()));
    } else {
        res.push(format!("nodes: {have} already in use, kept"));
    }
    api_lockdown(paths); // rh-core's check needs nodes in the config, so the empty template could not take it before
    if router_only(paths) {
        res.push("PC tunnel: off (router-only mode: the router tunnels your devices)".into());
    } else if let Some(owner) = crate::pc_tunnel::port_blocker(paths) {
        res.push(format!("PC tunnel: NOT started. {}", crate::pc_tunnel::blocker_hint(&owner)));
    } else if crate::pc_tunnel::core_restart(paths) {
        res.push("PC tunnel: running on 127.0.0.1:2080".into());
    } else {
        res.push("PC tunnel: rh-core did not come up, see logs/core.log".into());
    }
    if !router_only(paths) {
        res.push(vpn_check(paths));
    }
    for l in &res {
        say(l);
    }
    Ok(res)
}
