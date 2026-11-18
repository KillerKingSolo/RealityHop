//! Private Internet Access (PIA), the VPN app Reality Hop checks: its settings must send OpenVPN over TCP 443 through
//! the PC tunnel's SOCKS5 port.

use serde_json::Value;

/// What to set in PIA by hand.
pub const PIA_HOW: &str = "PIA > Settings > Protocols: OpenVPN, Transport TCP, Remote Port 443. Then Settings > Multi-Hop: tick \"Multi-hop and Obfuscation\", choose SOCKS5 Proxy, EDIT: host 127.0.0.1, port 2080, no username or password";

/// The two macOS-only checks.
pub struct MacChecks {
    /// pia_bypass() worked: the tunnel is excluded from PIA's kill switch.
    pub bypass_ok: bool,
    /// A local DNS app (NextDNS, AdGuard) answers on this Mac.
    pub dns_proxies: bool,
}

// What is set wrong in PIA's settings.json; empty when it is right (engine.vpn_state)
pub fn wrong_settings(st: &Value, mac: Option<&MacChecks>) -> Vec<&'static str> {
    let pc = st.get("proxyCustom").filter(|p| p.is_object()).cloned().unwrap_or_default();
    let mut wrong = Vec::new();
    if st.get("method") != Some(&"openvpn".into()) {
        wrong.push("protocol is not OpenVPN");
    }
    if st.get("protocol") != Some(&"tcp".into()) {
        wrong.push("transport is not TCP");
    }
    if st.get("remotePortTCP").and_then(Value::as_f64) != Some(443.0) {
        wrong.push("remote port is not 443");
    }
    let proxy_on = st.get("proxyEnabled").is_none_or(|v| v.as_bool().unwrap_or(!v.is_null()));
    let proxy_ok = st.get("proxyType") == Some(&"custom".into())
        && proxy_on
        && pc.get("host") == Some(&"127.0.0.1".into())
        && pc.get("port").and_then(Value::as_f64) == Some(2080.0);
    if !proxy_ok {
        wrong.push("the SOCKS5 proxy is not 127.0.0.1:2080");
    }
    if let Some(m) = mac {
        if st.get("killswitch").and_then(Value::as_str).unwrap_or("auto") != "off" && !m.bypass_ok {
            wrong.push(
                "the kill switch is on: on a Mac it blocks the tunnel (Settings → Privacy, untick both kill switches)",
            );
        }
        if !st.get("overrideDNS").and_then(Value::as_str).unwrap_or("pia").is_empty() && m.dns_proxies {
            wrong.push("another DNS app (such as NextDNS) is on: with PIA connected no name resolves (turn it off, or PIA → Settings → Network → Name Servers: Use Existing DNS)");
        }
    }
    wrong
}

// One line about the VPN app; t translates a text with {name} fields (engine.vpn_check)
pub fn vpn_check(found: bool, wrong: &[&str], t: impl Fn(&str, &[(&str, String)]) -> String) -> String {
    if !found {
        return t(
            "VPN app: PIA not found. Point your VPN app at SOCKS5 127.0.0.1:2080 and use OpenVPN over TCP port 443 (see the VPN step in the setup)",
            &[],
        );
    }
    if wrong.is_empty() {
        return t("VPN app: PIA uses OpenVPN over TCP 443 through SOCKS5 127.0.0.1:2080, correct", &[]);
    }
    let list = wrong.iter().map(|w| t(w, &[])).collect::<Vec<_>>().join(", ");
    t("VPN app: SET THIS IN PIA BY HAND ({wrong}): ", &[("wrong", list)]) + &t(PIA_HOW, &[])
}
