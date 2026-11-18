//! Checks on addresses, names and node fields that come from files, the feed or the person.

use crate::node::Node;
use crate::node_feed::FPS;
use crate::python_compat;

// A router address or name, safe on a command line and in the .cmd file that adds the key: [A-Za-z0-9][A-Za-z0-9.-]*
pub fn host_ok(h: &str) -> bool {
    let b = h.as_bytes();
    !b.is_empty() && b[0].is_ascii_alphanumeric() && b.iter().all(|c| c.is_ascii_alphanumeric() || b".-".contains(c))
}

// A plain IPv4 internet address: a node list must never point the tunnel or the pings at this PC or the LAN
pub fn public_ip(h: &str) -> bool {
    // Python's is_global also passes multicast (224.0.0.0/4), which no node can be; 1.7.x let it through
    python_compat::ipv4(h).is_some_and(|a| python_compat::is_global(a) && !a.is_multicast())
}

// A valid address of a device on the local network (what the router tunnel goes by)
pub fn lan_ip(x: &str) -> bool {
    python_compat::ipv4(x).is_some_and(python_compat::in_lan_nets)
}

// A VLESS user id as the feed writes it: [0-9a-fA-F-]{36}
pub fn uuid_ok(s: &str) -> bool {
    s.len() == 36 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

// A REALITY public key: [A-Za-z0-9_-]{43}
pub fn pbk_ok(s: &str) -> bool {
    s.len() == 43 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

// A REALITY short id: 0 to 8 hex byte pairs
pub fn sid_ok(s: &str) -> bool {
    s.len() <= 16 && s.len().is_multiple_of(2) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

// The checks the feed makes, for a node that came from elsewhere (the router, the vault)
pub fn node_ok(n: &Node) -> bool {
    public_ip(&n.server)
        && uuid_ok(&n.uuid)
        && pbk_ok(&n.pbk)
        && sid_ok(&n.sid)
        && host_ok(&n.sni)
        && FPS.contains(&n.fp.as_str())
}
