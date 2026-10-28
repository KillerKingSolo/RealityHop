#!/bin/sh
# fw4 include, re-run on every firewall reload and at boot.
# Diverts only the chosen LAN devices (RealityHop fills in the list) into sing-box tproxy (7895). DNS (53) is left to the router.
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
    ip saddr @tunneled meta l4proto { tcp, udp } th dport 53 counter return
    ip daddr { 127.0.0.0/8, 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 169.254.0.0/16, 224.0.0.0/4, 240.0.0.0/4 } counter return
    ip saddr @tunneled meta l4proto { tcp, udp } tproxy ip to 127.0.0.1:7895 meta mark set 1 counter accept
  }
}
NFT
ip rule list | grep -q 'fwmark 0x1 lookup 100' || ip rule add fwmark 1 table 100
ip route show table 100 | grep -q 'local default' || ip route add local default dev lo table 100
