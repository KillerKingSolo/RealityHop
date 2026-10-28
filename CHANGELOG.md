# Changelog

Each release explains what changed and why it matters to you. Installed copies update themselves (Settings > Install
RealityHop updates automatically). To update by hand, download `RealityHop.exe` from the release and run it.

## 1.0.2 (2026-10-27)

**Why update:** pressing **Update** in 1.0.0 or 1.0.1 could leave RealityHop half-broken until you restarted it: the
window icon turned into the default Python quill, the PC tunnel tile said "Needs restart", **Restart tunnel** reported
that the tunnel did not come back (even though it was running), and Windows showed "Failed to remove temporary
directory". This release fixes that, and moves RealityHop to Program Files.

- **Update fix:** RealityHop.exe is a one-file program that unpacks itself to a temporary folder when it starts. When
  the old version started the new one, the new one took over the old one's temporary folder instead of unpacking its own,
  and Windows deleted that folder from under it when the old version closed. Every restart that RealityHop does itself
  (after an update, from the tray, from Settings) now gives the new process a clean start.
- **Installs to `C:\Program Files\RealityHop`** instead of `%LOCALAPPDATA%\Programs\RealityHop`. Installing asks for one
  UAC prompt. It also gives your Windows user modify rights on that one folder, so later updates still install silently.
  The program never runs as administrator.
- **Moving from 1.0.0 or 1.0.1:** the first time you open the window after this update, RealityHop offers to move itself to
  Program Files (one UAC prompt). It also updates the Start menu entry, the logon start and the Apps list, and removes the
  old folder. Your settings, nodes and the running tunnel are kept. If the update to 1.0.2 came through the background
  watcher, restart RealityHop once (tray > Quit, then open it from the Start menu) so the fixed version takes over cleanly.
- **Release page:** the SHA-256 checksum of `RealityHop.exe` is now printed in the release notes. It replaces the
  separate `RealityHop.exe.sha256` download. GitHub also shows the checksum next to the download, and the built-in
  updater checks it before it installs anything, so updating works exactly as before.
- **Release notes** now come from this changelog: what changed and why it matters, instead of a list of commits.

## 1.0.1 (2026-10-27)

**Why update:** if your VPN would not connect through RealityHop, this is the likely cause. The setup did not say which
VPN protocol to use.

- **VPN instructions:** the setup wizard, README and docs now say to set your VPN to **OpenVPN over TCP, port 443**.
  The VPN reaches RealityHop through a SOCKS5 proxy, and a SOCKS5 proxy carries only TCP, so UDP and WireGuard
  connections cannot get through. Port 443 is the normal HTTPS port, the least likely to be blocked.
- **Check PIA** (setup wizard, VPN step) now checks all four PIA settings: protocol OpenVPN, transport TCP, remote port
  443, and the SOCKS5 proxy at 127.0.0.1:2080. It tells you which one is wrong instead of only checking the proxy.
- **OpenVPN config files:** the example now shows all three lines you need: `proto tcp`, `remote <server> 443`,
  `socks-proxy 127.0.0.1 2080`.

## 1.0.0 (2026-10-27)

First public release.

**Setup**
- One file: download `RealityHop.exe`, run it, and the setup wizard installs it to
  `%LOCALAPPDATA%\Programs\RealityHop` (moved to Program Files in 1.0.2). No admin rights are needed, except one UAC
  prompt if you want the tunnel to start at boot.
- The wizard downloads sing-box (official releases, checksum checked), finds and tests the first 6 nodes, starts the
  tunnel and shows how to set up your VPN app.
- Choose sing-box **stable** (default) or **test builds** (beta, alpha).

**Staying connected**
- The background watcher (tray icon) checks every node through the live tunnel every 60 minutes. A dead node is
  re-checked twice before it is replaced, so one dropped test does not cost you a working node.
- When every node is dead, **Rescue** refills from the vault, a list of nodes that worked before. It works even when
  the public node list cannot be downloaded.
- With PIA, RealityHop follows PIA's own connection state and repairs at once when PIA cannot reconnect.
- At logon the tray icon shows the last known state immediately, then checks as soon as the tunnel answers, instead of
  sitting grey.
- **Stop / Start** turns everything off for when you are away, including the start at boot.

**Router (optional, GL.iNet)**
- Put a phone, console or TV through the tunnel with nothing installed on it. The wizard guides you step by step: SSH
  key, sing-box on the router, choose devices. Your router password is typed into Windows' own `ssh`; RealityHop never
  sees it.
- The router repairs itself from the vault while your PC is off.
- Turn router features off and the router section disappears.

**Updates**
- RealityHop updates itself from GitHub Releases and checks the checksum before it replaces the program.
- sing-box updates itself daily on your channel, separately from RealityHop releases.

**Seeing and reporting**
- **Show backend files** adds buttons for the `PC`, `Router` and `logs` folders: the live configs, the router scripts
  and the three logs.
- **Report a problem** makes an anonymous copy of the logs, with addresses, names, keys and secrets replaced by
  tokens, ready to attach to a GitHub issue. Nothing is sent automatically.

**License:** PolyForm Noncommercial 1.0.0. Free to use, change and share; not for sale or commercial use.
