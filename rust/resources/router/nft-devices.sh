#!/bin/sh
# fw4 include, re-run on every firewall reload and at boot.
# Diverts the chosen LAN devices, or every device on the main network (Reality Hop fills this in), into sing-box tproxy
# (7895). DNS (53) is left to the router.
# If the sing-box service is disabled, removes the diversion instead, so no device is ever cut off.
if [ "$(uci -q get sing-box.main.enabled)" != "1" ]; then
  nft list tables 2>/dev/null | grep -q 'inet singbox' && nft delete table inet singbox
  ip rule del fwmark 1 table 100 2>/dev/null; exit 0
fi
modprobe nft_tproxy 2>/dev/null; modprobe nft_socket 2>/dev/null
nft list tables 2>/dev/null | grep -q 'inet singbox' && nft delete table inet singbox
nft -f - <<'NFT'
table inet singbox {
  set tunneled { type ipv4_addr; @ELEMENTS@ }
  chain prerouting {
    type filter hook prerouting priority mangle; policy accept;
    meta mark 0xff counter return
    meta l4proto { tcp, udp } socket transparent 1 meta mark set 1 counter accept
    @WHO@ meta l4proto { tcp, udp } th dport 53 counter return
    ip daddr { 127.0.0.0/8, 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 169.254.0.0/16, 224.0.0.0/4, 240.0.0.0/4 } counter return
    @WHO@ meta l4proto { tcp, udp } tproxy ip to 127.0.0.1:7895 meta mark set 1 counter accept
  }
  # sing-box listens for the diverted traffic on every address, so any device could connect to the router's port 7895
  # and make sing-box dial itself. Diverted packets keep their real port (443, ...), so nothing may reach 7895 directly.
  chain input {
    type filter hook input priority filter - 1; policy accept;
    meta l4proto { tcp, udp } th dport 7895 counter drop
  }
  # with every device tunnelled, their IPv6 would go around the tunnel: refused, so they use the tunnelled IPv4
  chain forward {
    type filter hook forward priority filter - 1; policy accept;
@V6@
  }
}
NFT
ip rule list | grep -q 'fwmark 0x1 lookup 100' || ip rule add fwmark 1 table 100
ip route show table 100 | grep -q 'local default' || ip route add local default dev lo table 100
