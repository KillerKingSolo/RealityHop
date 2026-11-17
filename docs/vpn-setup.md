# VPN app setup

Reality Hop gives your VPN app a local SOCKS5 proxy:

| Setting | Value |
|---|---|
| Type | SOCKS5 |
| Host | `127.0.0.1` |
| Port | `2080` |
| Username / password | none |

The VPN app sends its own encrypted connection through the proxy. Reality Hop carries it inside the REALITY tunnel.

**Use OpenVPN over TCP, port 443.** A SOCKS5 proxy carries TCP only, so UDP and WireGuard cannot go through it. 443
is the normal HTTPS port, the least likely to be blocked.

## Private Internet Access (PIA)

1. PIA → **Settings → Protocols**: Protocol **OpenVPN**, Transport **TCP**, Remote Port **443**.
2. **Settings → Multi-Hop**: tick **Multi-hop and Obfuscation** and choose **SOCKS5 Proxy**.
3. Click **EDIT**, enter host `127.0.0.1` and port `2080`, leave the username and password empty, and save.
4. Only if Split Tunnel has **All Other Apps** set to **Use VPN**: **Settings → Split Tunnel**, click
   **Add Application**, pick `C:\Program Files\RealityHop\PC\rh-core.exe` and set it to **Bypass VPN**.
5. Connect.

![PIA Multi-Hop with the SOCKS5 proxy](images/pia-multihop-socks5.png)

**Mac:** skip step 4. Reality Hop adds itself to PIA's Split Tunnel list as **Bypass VPN** (and switches Split Tunnel
on when it is off). That one app rule covers the tunnel engine too: Reality Hop signs its copy of sing-box with an id
that starts with the app's own (`com.realityhop.app.core`), and PIA counts it as part of the app. If PIA's command
line ever refuses, the setup wizard shows the manual step: **Settings → Split Tunnel → Add Application →
Applications → Reality Hop → Bypass VPN**.

**Mac: PIA's kill switch works, through IP rules Reality Hop keeps for you.** PIA's Mac kill switch (both the VPN Kill
Switch and the Advanced Kill Switch) blocks every program whatever its app rules say, the tunnel too, so the VPN would
lose the connection it runs through. Its one exception is Split Tunnel **IP** rules. So Reality Hop puts the addresses
of the 6 nodes in use there (Bypass VPN), and keeps the list current: every node swap and tunnel restart updates it, and
nodes under test are added for the test only. It changes only the addresses it added; your own rules stay, and
uninstalling removes its addresses. You see them in PIA → Settings → Split Tunnel. It uses PIA's command line
(`piactl -u applysettings`), which PIA marks unstable; if a PIA update breaks it, **Check PIA** warns that the kill
switch is on, and turning both kill switches off is the fallback.

**Mac: one DNS app at a time.** A DNS app or profile such as NextDNS sends every lookup through its own proxy, and PIA
blocks that while connected: the VPN connects but no website loads (names do not resolve; addresses do). Turn the DNS
app off while PIA is on, or set PIA → Settings → Network → Name Servers to **Use Existing DNS** so PIA leaves DNS to it.

**Why rh-core.exe must bypass the VPN:** rh-core.exe carries the VPN's own connection to the node. When all other apps
use the VPN, PIA routes that connection back into the VPN, which is not up yet, and the VPN cannot connect. Only
rh-core.exe needs the rule; RealityHop.exe does not. With All Other Apps set to Bypass VPN, rh-core.exe already
bypasses it and needs no rule.

![PIA Split Tunnel with rh-core.exe set to Bypass VPN](images/pia-split-tunnel.png)

The setup wizard's **Check PIA** button reads PIA's settings file. It tells you whether the protocol (OpenVPN), the
transport (TCP), the remote port (443) and the proxy are all set correctly.

With PIA, Reality Hop also follows PIA's own connection state (`piactl monitor`). When PIA drops and has not
reconnected a minute later, Reality Hop checks the nodes and fixes them at once.

**Games and apps that should skip the VPN:** PIA → Settings → **Split Tunnel**.

## Other VPN apps

Any VPN app with a SOCKS5 proxy setting works the same way: point it at `127.0.0.1:2080`.

**OpenVPN config files** (OpenVPN GUI, OpenVPN Connect and others): use TCP and the server's port 443, and add the
proxy line. Your provider's address goes where `vpn.example.com` is:

```
proto tcp
remote vpn.example.com 443
socks-proxy 127.0.0.1 2080
```

Without PIA, Reality Hop checks the nodes on its normal schedule (every 60 minutes by default, set in Settings). It does
not get an instant signal when the VPN drops.

## While Reality Hop is stopped

**Stop** in the window turns the tunnel off. While it is stopped, turn the VPN app's proxy off; in PIA, untick
**Multi-hop and Obfuscation**. Otherwise the VPN has nothing to connect through. After **Start**, turn the proxy back on.
