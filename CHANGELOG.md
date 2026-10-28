# Changelog

Each release explains what changed and why it matters to you. Installed copies update themselves (Settings > Install
Reality Hop updates automatically). To update by hand, download `RealityHop.exe` from the release and run it.

## 1.1.3 (2026-10-28)

**Why update:** several PCs can share one router without overwriting each other, and a program blocking port 2080 is named at once.

- **Several PCs, one router.** The router now holds the node set and the device list that run. When another PC pushed
  (or the router repaired itself), Check now, Test on router, a fix and every background check take the router's nodes
  into both configs and the PC tunnel, instead of pushing this PC's older set over them. A push replaces the 6 nodes on
  the router; nothing is added twice. The device list is shared too: set it once on any PC.
- The router tile says "Older config" only when this PC has changes the router does not run yet.
- **PC tunnel blocked by another program on port 2080.** Restart no longer loops for a minute and ends with "did not come
  back"; it says at once which program holds the port and what to do. The tunnel of the test builds before 1.0
  (`sb-core.exe`, started at boot by its own scheduled task) is now removed by the setup, in the same UAC prompt.

## 1.1.2 (2026-10-28)

**Why update:** Reality Hop keeps finding nodes when GitHub is blocked where you are, and the overview reads cleaner.

- **Node list when GitHub is blocked.** Sources are tried in this order: GitHub direct, GitHub through your own tunnel
  (one working node is enough), the jsDelivr mirrors, the router (only with router features on), then the last good
  copy saved on disk. Also covers a download that stalls partway, as DPI filters do.
- **sing-box and app updates** also retry through the tunnel. The SHA-256 check is unchanged.
- **Fix: test online nodes** falls back to the nodes that worked before when the online list cannot be reached or has
  no working node, instead of stopping with an error.
- **PC only:** a failed node list download no longer tries to reach the router over SSH (up to 10 s wasted).
- Each scan keeps only the usable lines of the 18 MB node list (about 4.5 MB).
- **Overview:** "Press Test on router" in the router tile is now a link that runs the test; "press Push to router"
  runs the push. Legend and counters are capitalised (PIA Dropped, Node Replaced, Tunnel Restart; PIA Drops,
  Replacements, Restarts, Rescues), and so are the tile hints (Hourly checks wait while this window is open).

## 1.1.1 (2026-10-28)

**Why update:** clearer node fix actions, a new fix from nodes that worked before, a readable 24-hour legend, and the
new license.

- **License:** now CC BY-NC-ND 4.0 instead of PolyForm Noncommercial 1.0.0. You may share Reality Hop unchanged and
  with credit (Reality Hop by KillerKingSolo, sing-box by SagerNet, FastNodes by rtwo2); changed versions and
  commercial use are not allowed. See [LICENSE](LICENSE).
- **Fixing a node says what it does.** Right-click a node, or use the Nodes section:
  - **Fix: test online nodes**: tests the fastest nodes from the online list and swaps in the best one.
  - **Fix: nodes that worked before** (new): tests the vault again and swaps in the best one. Useful when the online list
    cannot be reached.
  - **Fix: quick swap, untested**: puts the fastest-answering node into the configs only, without a test or a restart.

  These replace "Replace selected Â· configs only / tested".
- **Window size:** the window opens tall enough for the whole sidebar, so the Settings button is no longer cut off
  (never taller than the screen).
- **Last 24 hours:** Rescue is purple now, so it no longer looks like PIA dropped (red). The legend is capitalised.

## 1.1.0 (2026-10-28)

**Why update:** everything Reality Hop keeps now sits in one folder that only you can open, a broken line in the
public node list can no longer stop every scan, and several crashes and wrong instructions are fixed.

- **One folder for everything.** The settings, configs, node vault, logs and the tunnel engine (`rh-core.exe`) now live
  in the install folder with `RealityHop.exe` (`C:\Program Files\RealityHop` by default), not in
  `%LOCALAPPDATA%\RealityHop`. The setup can install to **another folder** (Install step > Change...). After updating,
  the window offers to move an existing install there once (one admin prompt; the tunnel stops for a few seconds and the
  boot start follows the move).
- **The install folder is yours alone.** Only your Windows user, SYSTEM and administrators can open it, because it holds
  the router SSH key, the node keys and the API secret. Before, every account on the PC could read Program Files.
- **Name:** Reality Hop, with a space, everywhere you see it: the window, the tray, the Start menu, Windows search and
  Apps & features. The Start menu, desktop and logon shortcuts are renamed to match. Task Manager shows the name too.
  File and folder names stay `RealityHop` (`RealityHop.exe`, the install folder, the GitHub repository).
- **Router only (new):** the setup's router question has a third answer, **Router only**, also in Settings. Nothing then
  runs on this PC's connection: the router tunnels the devices you tick, or, with the new **Every device on the main
  network** switch (setup and Router > Devices), every device on the router's main Wi-Fi and LAN, this PC included. Guest
  and IoT networks stay as they are. In this mode the node tests run on the router, because this PC's own traffic may go
  through the tunnel, and the main network's IPv6 is refused so nothing goes around the tunnel.
- **sing-box version in the window:** under the name, for example `sing-box 1.13.2 Â· stable` or
  `sing-box 1.15.0-alpha.13 Â· test build`.
- **Router security fix:** sing-box on the router listened for the diverted traffic on every address, so any device on
  your network could connect to the router's port 7895 and make sing-box dial itself. The router's firewall rule now
  drops every packet sent to that port directly (diverted traffic keeps its real port). Press **Push to router** once
  after updating to install it.
 Nodes (globe), PC tunnel (computer), Router (router) and Help (question mark) use Windows' own
  icon font instead of a diamond, square, triangle and dot.
- **Scroll bars:** the slider turns fully red under the pointer, without the grey grip lines in the middle.
- **Router > Devices and the setup:** the address reservation steps were wrong. It is GL.iNet web panel > **CLIENTS**,
  then the **Reserved IP** slider next to the device.
- **Fixed:** one malformed line in the node list (a port like 99999) stopped every scan, replace and rescue.
- **Fixed:** a node location with letters outside Western European ones (such as ÅÃ³dÅº) stopped a replace halfway, with
  the configs written but not deployed.
- **Fixed:** putting a candidate into vless-5 or vless-6 failed when fewer than 6 nodes were in use.
- **Fixed:** Install again (repair) in Run setup again could break the install by copying the program onto itself.
- **Fixed:** opening Reality Hop while it was already open started a second window; it now brings the open one forward.
- **Fixed:** an account or folder name with an apostrophe (such as O'Brien) broke the install, the boot start and the
  shortcuts. In the install step that runs as administrator, such a name could even run as a command.
- **Fixed:** a non-ASCII or `%` character in the user name broke Add key to router.
- **Safer node list handling:** nodes that point at this PC or the local network are ignored, and unknown TLS
  fingerprints fall back to chrome.
- **Safer settings:** numbers out of range (such as a check every 0 minutes) are clamped, and settings, configs and the
  vault are written atomically, so the window and the watcher never read a half-written file.
- **Control API:** no web page may call the PC tunnel's local API any more (it always needed the secret). Applies from
  the next tunnel restart.
- **Router devices:** only addresses on a local network are accepted (no 999.1.1.1). Save and push waits for a running
  action instead of failing silently.
- **Report a problem** now also replaces the websites in sing-box's log (host-N), its time zone offset, and MAC
  addresses written as aabb.ccdd.eeff.
- README: a **Star History** chart and screenshots of the Devices window and Settings.

## 1.0.4 (2026-10-27)

**Why update:** the tray icon now shows on the taskbar instead of hiding, and you can change which devices use the router
tunnel without running the setup again.

- **Tray icon visible:** Windows 11 hides every new tray icon behind the `^` arrow, and moving Reality Hop to Program
  Files in 1.0.2 counted as new, so the icon seemed to be missing. Reality Hop now shows its icon on the taskbar the first
  time it runs. If you hide it yourself later (Windows Settings > Personalization > Taskbar > Other system tray icons),
  your choice is kept.

- **Router > Devices** (new window): every device the router knows, with the ones in the tunnel ticked. Tick to add,
  untick to remove, or type an address. **Save and push** sends the list to the router.
- The same window explains how to keep a device on one address, which is what the router goes by. Turn off the
  changing ("private" or "random") Wi-Fi MAC address on the phone for your network, then reserve its address in the
  GL.iNet panel (CLIENTS, the Reserved IP slider next to the device). A link opens the panel.
- README: a clearer picture of how the traffic flows, and screenshots of the app.

## 1.0.3 (2026-10-27)

**Why update:** Reality Hop is easier to find, and it now says plainly where its nodes come from.

- **Start menu and Windows search:** a choice in the setup (Install step) and in Settings, on by default. Press the
  Windows key and type Reality Hop. An optional **desktop icon** is there too. Both follow the program when it moves or
  updates, and Uninstall removes them.
- **Where the nodes come from** is now linked everywhere you meet Reality Hop: the setup's welcome page, Settings, the
  README and every release page. Reality Hop takes its nodes from the public list
  [rtwo2/FastNodes](https://github.com/rtwo2/FastNodes) and tests each one itself before using it. The tunnel engine is
  [SagerNet/sing-box](https://github.com/SagerNet/sing-box).
- **Settings** links to this changelog ("What's new") and to the project page.

## 1.0.2 (2026-10-27)

**Why update:** pressing **Update** in 1.0.0 or 1.0.1 could leave Reality Hop half-broken until you restarted it: the
window icon turned into the default Python quill, the PC tunnel tile said "Needs restart", **Restart tunnel** reported
that the tunnel did not come back (even though it was running), and Windows showed "Failed to remove temporary
directory". This release fixes that, and moves Reality Hop to Program Files.

- **Update fix:** RealityHop.exe is a one-file program that unpacks itself to a temporary folder when it starts. When
  the old version started the new one, the new one took over the old one's temporary folder instead of unpacking its own,
  and Windows deleted that folder from under it when the old version closed. Every restart that Reality Hop does itself
  (after an update, from the tray, from Settings) now gives the new process a clean start.
- **Installs to `C:\Program Files\RealityHop`** instead of `%LOCALAPPDATA%\Programs\RealityHop`. Installing asks for one
  UAC prompt. It also gives your Windows user modify rights on that one folder, so later updates still install silently.
  The program never runs as administrator.
- **Moving from 1.0.0 or 1.0.1:** the first time you open the window after this update, Reality Hop offers to move itself to
  Program Files (one UAC prompt). It also updates the Start menu entry, the logon start and the Apps list, and removes the
  old folder. Your settings, nodes and the running tunnel are kept. If the update to 1.0.2 came through the background
  watcher, restart Reality Hop once (tray > Quit, then open it from the Start menu) so the fixed version takes over cleanly.
- **Release page:** the SHA-256 checksum of `RealityHop.exe` is now printed in the release notes. It replaces the
  separate `RealityHop.exe.sha256` download. GitHub also shows the checksum next to the download, and the built-in
  updater checks it before it installs anything, so updating works exactly as before.
- **Release notes** now come from this changelog: what changed and why it matters, instead of a list of commits.

## 1.0.1 (2026-10-27)

**Why update:** if your VPN would not connect through Reality Hop, this is the likely cause. The setup did not say which
VPN protocol to use.

- **VPN instructions:** the setup wizard, README and docs now say to set your VPN to **OpenVPN over TCP, port 443**.
  The VPN reaches Reality Hop through a SOCKS5 proxy, and a SOCKS5 proxy carries only TCP, so UDP and WireGuard
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
- The wizard downloads sing-box (official releases, checksum checked), finds and tests the first 6 nodes from the public
  list [rtwo2/FastNodes](https://github.com/rtwo2/FastNodes), starts the tunnel and shows how to set up your VPN app.
- Choose sing-box **stable** (default) or **test builds** (beta, alpha).

**Staying connected**
- The background watcher (tray icon) checks every node through the live tunnel every 60 minutes. A dead node is
  re-checked twice before it is replaced, so one dropped test does not cost you a working node.
- When every node is dead, **Rescue** refills from the vault, a list of nodes that worked before. It works even when
  the public node list cannot be downloaded.
- With PIA, Reality Hop follows PIA's own connection state and repairs at once when PIA cannot reconnect.
- At logon the tray icon shows the last known state immediately, then checks as soon as the tunnel answers, instead of
  sitting grey.
- **Stop / Start** turns everything off for when you are away, including the start at boot.

**Router (optional, GL.iNet)**
- Put a phone, console or TV through the tunnel with nothing installed on it. The wizard guides you step by step: SSH
  key, sing-box on the router, choose devices. Your router password is typed into Windows' own `ssh`; Reality Hop never
  sees it.
- The router repairs itself from the vault while your PC is off.
- Turn router features off and the router section disappears.

**Updates**
- Reality Hop updates itself from GitHub Releases and checks the checksum before it replaces the program.
- sing-box updates itself daily on your channel, separately from Reality Hop releases.

**Seeing and reporting**
- **Show backend files** adds buttons for the `PC`, `Router` and `logs` folders: the live configs, the router scripts
  and the three logs.
- **Report a problem** makes an anonymous copy of the logs, with addresses, names, keys and secrets replaced by
  tokens, ready to attach to a GitHub issue. Nothing is sent automatically.

**License:** PolyForm Noncommercial 1.0.0. Free to use, change and share; not for sale or commercial use.
