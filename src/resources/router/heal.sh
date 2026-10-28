#!/bin/sh
# Router self-repair, run by cron every 30 minutes (installed by RealityHop on every push to the router).
# If no vless node in the running config can fetch a page, now and again a minute later, swap in the next 6 nodes from
# vault.json (verified nodes pushed from the PC), restart sing-box, and keep going until a set works.
# Log: /etc/sing-box/heal.log (last 200 lines).  Off: remove the heal.sh line from /etc/crontabs/root.
C=/etc/sing-box/config.json; V=/etc/sing-box/vault.json; L=/etc/sing-box/heal.log
API=http://127.0.0.1:9090; URL=https://www.gstatic.com/generate_204
log() { echo "$(date '+%F %T') $*" >> $L; tail -n 200 $L > $L.t && mv $L.t $L; }

pgrep -f '/etc/sing-box/config[.]json' >/dev/null || exit 0     # service off on purpose: leave it alone
[ $(( $(date +%s) - $(cat /tmp/pc-alive 2>/dev/null || echo 0) )) -lt 7200 ] && exit 0   # the PC watcher checked within 2 h: it decides
mkdir /tmp/heal.lock 2>/dev/null || exit 0; trap 'rmdir /tmp/heal.lock' EXIT
ping -c 1 -W 3 9.9.9.9 >/dev/null 2>&1 || ping -c 1 -W 3 1.1.1.1 >/dev/null 2>&1 || exit 0   # router offline: nodes are not the problem

alive() {
    H="Authorization: Bearer $(jq -r '.experimental.clash_api.secret' $C)"
    for t in $(jq -r '.outbounds[] | select(.type == "vless") | .tag' $C); do
        curl -s -m 12 -H "$H" "$API/proxies/$t/delay?url=$URL&timeout=8000" | grep -q delay && return 0
    done
    return 1
}

alive && { rm -f /tmp/heal.giveup; exit 0; }
[ $(( $(date +%s) - $(cat /tmp/heal.giveup 2>/dev/null || echo 0) )) -lt 1800 ] && exit 0   # last full try failed: wait
sleep 60; alive && { log "every node failed once, fine again a minute later"; exit 0; }
[ -s $V ] || { log "every node is dead and the vault is empty; push from the PC"; exit 0; }

log "every node failed twice, a minute apart; trying vault nodes"
cp $C /tmp/heal-orig.json; n=$(jq length $V); i=0
while [ $i -lt $n ]; do
    jq --slurpfile v $V --argjson i $i '
        ($v[0][$i:$i + 6] | to_entries | map(.value + {tag: "vless-\(.key + 1)"})) as $new
        | .outbounds = $new + [.outbounds[] | select(.type != "vless")
                               | if .type == "urltest" then .outbounds = [$new[].tag] else . end]' /tmp/heal-orig.json > /tmp/heal-new.json
    if sing-box check -c /tmp/heal-new.json >/dev/null 2>&1; then
        cp /tmp/heal-new.json $C; /etc/init.d/sing-box restart; sleep 8
        if alive; then
            log "HEALED with vault $((i + 1))-$((i + 6)): $(jq -r '[.outbounds[] | select(.type == "vless") | .server] | join(" ")' $C)"
            date +%s > /etc/sing-box/heal.state; exit 0
        fi
        log "vault $((i + 1))-$((i + 6)) dead too"
    fi
    i=$((i + 6))
done
cp /tmp/heal-orig.json $C; /etc/init.d/sing-box restart; date +%s > /tmp/heal.giveup
log "no vault set works; previous config restored; next try in 30 minutes"
