for pid in $(pgrep -f 'sb-test[.]json'); do kill $pid; done; sleep 1
mkdir -p /etc/sing-box; mv /tmp/sb-new.json /etc/sing-box/config.json
tr -d '\r' < /tmp/nft-devices.sh > /etc/sing-box/nft-devices.sh; chmod +x /etc/sing-box/nft-devices.sh
uci set sing-box.main.enabled='1'; uci set sing-box.main.user='root'; uci set sing-box.main.conffile='/etc/sing-box/config.json'; uci commit sing-box
uci -q delete firewall.sbphone; uci -q get firewall.realityhop >/dev/null || uci set firewall.realityhop=include
uci set firewall.realityhop.type='script'; uci set firewall.realityhop.path='/etc/sing-box/nft-devices.sh'; uci set firewall.realityhop.reload='1'
uci set firewall.realityhop.enabled='1'; uci set firewall.realityhop.fw4_compatible='1'; uci commit firewall
/etc/init.d/sing-box enable; /etc/init.d/sing-box restart; sleep 7; fw4 reload >/dev/null 2>&1; sleep 2
pgrep -af 'sing-box run' | grep -q '/etc/sing-box/config.json' && echo 'sing-box service: running' || { echo 'sing-box service: NOT RUNNING'; logread | grep -iE 'sing-box' | tail -5; }
netstat -ln 2>/dev/null | grep -q ':7895' && echo 'tproxy: listening' || echo 'tproxy: NOT listening'
nft list tables 2>/dev/null | grep -q singbox && echo 'nft: loaded' || echo 'nft: absent'
ip rule list | grep -q 'fwmark 0x1' && echo 'policy route: ok' || echo 'policy route: MISSING'
