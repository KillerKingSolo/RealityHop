trap '' HUP PIPE INT TERM   # a dropped ssh connection must not stop this halfway through the swap
N=/tmp/rh-core.new; B=/usr/bin/sing-box; O=/tmp/rh-core.old
fail() { echo "ROUTER BUILD: $*"; rm -f $N /tmp/sb-new.json; rmdir /tmp/heal.lock 2>/dev/null; exit 1; }
mkdir /tmp/heal.lock 2>/dev/null || fail 'the self-repair is running; try again in a few minutes'
[ "$(sha256sum $N | cut -c1-64)" = "@SHA@" ] || fail 'the upload is damaged; nothing changed'
chmod +x $N; $N version 2>&1 | head -1 | grep -qF 'version @VER@' || fail 'the new build does not run on this router; nothing changed'
@CFG@$N check -c /tmp/sb-new.json || fail 'the new build rejects the router config; nothing changed'
cp $B $O 2>/dev/null || fail 'not enough free RAM to keep the old build; nothing changed'
cp /etc/sing-box/config.json /tmp/rh-config.old
/etc/init.d/sing-box stop; sleep 1; rm -f $B
if cp $N $B 2>/dev/null && chmod +x $B; then
  cp /tmp/sb-new.json /etc/sing-box/config.json; /etc/init.d/sing-box start; sleep 8
  if pgrep -f '/etc/sing-box/config[.]json' >/dev/null && curl -s -m 5 -H 'Authorization: Bearer @API@' http://127.0.0.1:9090/version | grep -q version; then
    echo '@CH@ @VER@ @SHA@' > @MARK@; opkg flag hold sing-box >/dev/null 2>&1
    rm -f $O /tmp/rh-config.old $N /tmp/sb-new.json; rmdir /tmp/heal.lock
    echo 'ROUTER BUILD: installed'; echo "pushed-md5 $(md5sum /etc/sing-box/config.json | cut -c1-32)"; exit 0
  fi
  echo 'ROUTER BUILD: the new build did not start:'; logread 2>/dev/null | grep -i sing-box | tail -5
else
  echo 'ROUTER BUILD: not enough flash space for the new build'
fi
/etc/init.d/sing-box stop; rm -f $B; cp $O $B; chmod +x $B; cp /tmp/rh-config.old /etc/sing-box/config.json
/etc/init.d/sing-box start; rm -f $O /tmp/rh-config.old $N /tmp/sb-new.json; rmdir /tmp/heal.lock
echo 'ROUTER BUILD: the previous sing-box and config are back'; exit 1
