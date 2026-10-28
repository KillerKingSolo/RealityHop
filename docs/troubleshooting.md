# Troubleshooting

Logs are in the `logs` folder of the install folder (`C:\Program Files\RealityHop\logs` by default): `window.log` (the window and the setup), `watch.log` (the background
watcher) and `core.log` (sing-box warnings). Turn on Settings > **Show backend files** for buttons that open them.

**To report a problem,** use **Report a problem** in the window. It makes an anonymous copy of the three logs, shows
it to you, and opens a GitHub issue to attach it to. **Do not paste `pc-config.json`, `router-config.json` or
`vault.json` into a public issue.** They contain node keys and your local API secret.

## The VPN app does not connect

- Is the tunnel running? The window's PC TUNNEL tile should say Running. If not, press **Restart tunnel**.
- Is the VPN app pointed at SOCKS5 `127.0.0.1:2080`? For PIA, see [vpn-setup.md](vpn-setup.md), or use the setup
  wizard's **Check PIA** button.
- Use **OpenVPN over TCP, port 443**. In PIA: Settings > Protocols, Protocol OpenVPN, Transport TCP, Remote Port 443.
  UDP and WireGuard cannot go through the SOCKS5 proxy.
- Press **Check now**. If every node is red, press **Rescue**.

## SmartScreen or antivirus blocks RealityHop.exe

The exe is not code-signed. SmartScreen: **More info > Run anyway**. Some antivirus products flag PyInstaller apps by
mistake. Check its SHA-256 against the one printed in the release notes, or build from source
([building.md](building.md)).

## "port 2080 is used by ..."

Another program already listens on port 2080. Close it, then run the setup again (Settings > Run setup again).

## sing-box: stable or test builds

Settings > **sing-box test builds (beta, alpha)** switches the channel. The newest build of that channel is installed
at once, even if it is older than the current one. Either way, sing-box updates itself daily. If a new build rejects the
config, the current build is kept, and the log says why.

## The tray icon is grey

Grey means the state is not known yet. At logon the icon first shows the last known state, then updates as soon as the
first check finishes, which takes a few seconds after the tunnel starts. If it stays grey, open the window and press
**Check now**.

## All nodes are dead

Press **Rescue**. It tests the vault of nodes that worked before and fills up from the public list. If nothing works,
check that the PC has internet at all. Then try **Show candidates > Test 40 fastest** later; public nodes come and go.

## Router: Push to router failed

- "CONFIG REJECTED": the router's sing-box did not accept the config. If its version is 1.13 or newer, please open an
  issue with the version number.
- "router unreachable over ssh": run the setup again and redo **Add key to router**. A firmware upgrade can reset the
  router's keys.
- Check the router's log: `logread | grep sing-box` over SSH.

## A device on the router is not tunnelled

Its address probably changed. Reserve a fixed address for it (GL.iNet web panel > CLIENTS, turn on the Reserved IP
slider next to the device), then update the device list (Router > Devices).

## Remove everything

Settings > **Uninstall**, or Windows Settings > Apps > Reality Hop. Then turn your VPN app's proxy off.
