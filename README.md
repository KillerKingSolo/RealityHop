# RealityHop

**Get your VPN through networks that block VPNs.** RealityHop wraps your VPN connection in a VLESS + REALITY tunnel
([sing-box](https://github.com/SagerNet/sing-box)). To the network it looks like ordinary HTTPS to a big website,
such as apple.com, so school, work, hotel and dorm Wi-Fi, or a router with "VPN blocking", lets it through. It finds free
public nodes from [rtwo2/FastNodes](https://github.com/rtwo2/FastNodes), tests them itself, keeps the fastest working ones in use and replaces dead
ones on its own.

**What's new:** [CHANGELOG.md](CHANGELOG.md) (newest version first) · **Download:** [latest release](../../releases/latest)

![Setup wizard](docs/images/setup-welcome.png)

```
 VPN app ──► 127.0.0.1:2080 ──► REALITY node ──────────────► VPN server ──► internet
 (PIA, ...)   RealityHop         (looks like HTTPS to          (your VPN's own
              on your PC          apple.com to the network)     encryption inside)
```

## Features

- **One exe, guided setup.** Download `RealityHop.exe`, run it, and the wizard does the rest: install, sing-box, the
  first nodes, your VPN app, start at boot.
- **Finds and tests nodes for you.** It downloads the public node list, runs a real REALITY handshake and an HTTPS
  fetch through each candidate, and keeps the 6 fastest that work.
- **Repairs itself.** A background watcher with a tray icon checks the nodes every hour. It replaces a dead node and
  rescues from a vault of known-good nodes when all are down. With PIA, it reacts the moment the VPN drops.
- **Tray icon shows the state.** The centre is the overall state. The ring has one segment per node: green working,
  amber slow, red dead.
- **Start menu and Windows search.** Press the Windows key and type RealityHop. A desktop icon is optional.
- **Optional GL.iNet router support.** Put a phone, console or TV through the tunnel too, with nothing installed on
  them. Turn it off and the router section disappears.
- **Stop / Start for when you travel.** One button turns everything off, including the start at boot, until you press
  Start.
- **Automatic updates.** RealityHop updates itself from this repo's Releases (SHA-256 checked).
- **sing-box keeps itself current on its own**, daily, on the channel you choose: **stable** (default) or **test
  builds** (beta, alpha). This does not wait for a RealityHop release.
- **See what runs behind the scenes.** Turn on *Show backend files* to get buttons for the `PC`, `Router` and `logs`
  folders: the live sing-box configs, the router scripts and the three logs.
- **Anonymous problem reports.** *Report a problem* bundles the three logs with addresses, names, keys and secrets
  replaced by tokens. You read it first, then attach it to a GitHub issue. Nothing is sent automatically.
- **Installs to `C:\Program Files\RealityHop`** with one UAC prompt, and updates itself there without asking again.
  The program never runs as administrator. No telemetry.

## Install

1. Download **`RealityHop.exe`** from [Releases](../../releases/latest).
2. Run it. Windows SmartScreen may warn because the exe is not code-signed: click **More info > Run anyway**. Some
   antivirus programs flag PyInstaller apps by mistake; the source is all here, and you can
   [build it yourself](docs/building.md).
3. Follow the setup wizard:
   - **Install** copies RealityHop to `C:\Program Files\RealityHop` (one UAC prompt), downloads sing-box, finds the first nodes
     and starts the tunnel. This takes about a minute.
   - **Router?** answers whether you also want a GL.iNet router set up. If you answer yes, the wizard guides you
     through it step by step ([details](docs/router-setup.md)). If you answer no, RealityHop shows only the PC part.
   - **VPN app** shows how to point your VPN at the tunnel ([details](docs/vpn-setup.md)).
   - **Finish** sets the background start at logon, the start at boot, and automatic updates.

Requirements: Windows 10 or 11 (64-bit), and a VPN app that can use a SOCKS5 proxy.

## Set up your VPN app

RealityHop listens on **SOCKS5 `127.0.0.1` port `2080`**, with no username or password.

Your VPN must use **OpenVPN over TCP, port 443**. A SOCKS5 proxy carries TCP only, and 443 is the normal HTTPS port.

**Private Internet Access (PIA):**
Settings > Protocols: Protocol **OpenVPN**, Transport **TCP**, Remote Port **443**. Then Settings > Multi-Hop: tick
**Multi-hop and Obfuscation**, choose **SOCKS5 Proxy**, click **EDIT**, enter host `127.0.0.1` and port `2080`, and
save.

![PIA Multi-Hop with the SOCKS5 proxy](docs/images/pia-multihop-socks5.png)

**Other VPN apps:** any app with a SOCKS5 proxy setting works the same way, using OpenVPN over TCP port 443. In an
OpenVPN config file, use `proto tcp`, the server's port `443`, and add `socks-proxy 127.0.0.1 2080`. See
[docs/vpn-setup.md](docs/vpn-setup.md).

## Day to day

| Where | What |
|---|---|
| Tray icon | Double-click opens the window. Right-click: Restart tunnel, Check now, Rescue, Quit. |
| **Check now** | Tests every node through the running tunnel. |
| **Restart tunnel** | Restarts the tunnel on the current nodes (about 2 seconds). |
| **Stop / Start** | Turns everything off, including the start at boot, for when you are away. Turn the VPN app's proxy off while stopped. |
| Nodes > **Show candidates** | Lists nodes from the public list; test them and put any of them in a slot. |
| Nodes > **Rescue** | Use when every node is dead. Tests the vault of known-good nodes and puts the best 6 in. |
| Help > **Report a problem** | Makes an anonymous report of the logs and opens a GitHub issue for you to attach it to. |
| Files (optional) | **Open backend folder** (`PC`, `Router`, `logs`, `anonymous-logs`) and **Open logs folder**. |
| **Settings** | Check interval, slow threshold, switching margin, router features on/off, sing-box stable or test builds, show backend files, updates, run setup again, uninstall. |

**Uninstall:** use Settings > Uninstall, or Windows Settings > Apps > RealityHop.

## Where the nodes come from

- **Node list:** [rtwo2/FastNodes](https://github.com/rtwo2/FastNodes), file
  [`sub/verified_tls.txt`](https://raw.githubusercontent.com/rtwo2/FastNodes/main/sub/verified_tls.txt). RealityHop
  keeps only VLESS + REALITY + XTLS-Vision nodes on port 443 that use a well-known front site. It then tests them
  itself; the list's own verification is not trusted.
- **Tunnel engine:** [SagerNet/sing-box](https://github.com/SagerNet/sing-box), downloaded from its official GitHub
  releases. Stable is the newest full release; test builds are the newest of all, alpha and beta included. The SHA-256
  checksum is checked, and the new build must accept the config before it replaces the old one. On your PC it runs as
  `rh-core.exe`.
- **Protocol:** VLESS with REALITY and XTLS-Vision, from [XTLS](https://github.com/XTLS).

## Privacy and security

- **Public nodes are run by strangers.** Your VPN still encrypts everything end to end, so a node sees only
  encrypted VPN traffic and its destination, the VPN server. Keep the VPN on top. Do not send private traffic through
  RealityHop without a VPN.
- **What RealityHop contacts:**
  - GitHub, for the node list, the sing-box releases and the RealityHop releases.
  - `api.ipify.org`, fetched through each node when it is tested.
  - `www.gstatic.com/generate_204`, the latency test through the tunnel.
  - `ip-api.com`, for node locations, asked through the tunnel.
  - Quad9 DNS (`9.9.9.9`), through the tunnel.

  Nothing about you is sent anywhere, and there is no telemetry.
- **What it keeps:** configs, the node vault, logs and settings in `%LOCALAPPDATA%\RealityHop` (folders `PC`,
  `Router`, `logs`, `anonymous-logs`). The router key is
  an SSH key made on your PC. Your router password is typed into Windows' own `ssh` and is never seen or stored by
  RealityHop.
- Make sure you are allowed to use a VPN on the network you are on.

See [SECURITY.md](SECURITY.md) for security problems. For anything else, use **Report a problem** in the window and
attach the anonymous report to an issue. Never attach `pc-config.json`, `router-config.json` or `vault.json`.

## Documentation

- [How it works](docs/how-it-works.md)
- [GL.iNet router setup](docs/router-setup.md)
- [VPN app setup](docs/vpn-setup.md)
- [Troubleshooting](docs/troubleshooting.md)
- [Building from source and releasing](docs/building.md)

## Run from source

```
pip install -r requirements.txt
python src/realityhop.pyw
```

## License

[PolyForm Noncommercial 1.0.0](LICENSE). You may use, change and share RealityHop for any **noncommercial** purpose:
personal use, study, research, hobby projects, charities, schools and public bodies. **Selling it or using it for
commercial purposes is not allowed.** This is a source-available license, not an OSI "open source" license.

sing-box and the node list are separate projects under their own licenses. RealityHop downloads them at run time and
does not ship them.
