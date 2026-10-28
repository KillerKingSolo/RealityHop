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

1. PIA > **Settings > Protocols**: Protocol **OpenVPN**, Transport **TCP**, Remote Port **443**.
2. **Settings > Multi-Hop**: tick **Multi-hop and Obfuscation** and choose **SOCKS5 Proxy**.
3. Click **EDIT**, enter host `127.0.0.1` and port `2080`, leave the username and password empty, and save.
4. Connect.

![PIA Multi-Hop with the SOCKS5 proxy](images/pia-multihop-socks5.png)

The setup wizard's **Check PIA** button reads PIA's settings file. It tells you whether the protocol (OpenVPN), the
transport (TCP), the remote port (443) and the proxy are all set correctly.

With PIA, Reality Hop also follows PIA's own connection state (`piactl monitor`). When PIA drops and has not
reconnected a minute later, Reality Hop checks the nodes and fixes them at once.

**Games and apps that should skip the VPN:** PIA > Settings > **Split Tunnel**.

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
