# Changelog

Each release explains what changed and why it matters to you. Installed copies update themselves (Settings → Install
Reality Hop updates automatically). To update by hand, download `RealityHop.exe` (Windows) or the `.dmg` for your Mac
from the release.

## 2.1.6 (2026-11-27)

**Why update:** the router update offer no longer gets lost behind Settings, Settings names upgrades right, and update checks no longer fail after a few restarts.

- Fixed: Settings called a newer alpha or beta build, such as 1.15.0-alpha.13 to 1.15.0-alpha.15, a downgrade.
- Fixed: closing Settings while the router update offer waited behind it counted as Later and hid the offer for 3 days. The offer now shows again after Settings closes.
- Fixed: a few restarts in an hour used up GitHub's request limit, and update checks failed with HTTP 403. GitHub's answers are kept for 15 minutes, and the last answer is used when GitHub refuses.
- Changed: the router build workflow commits as the repository owner, not as github-actions[bot].

## 2.1.5 (2026-11-27)

**Why update:** Check now and tunnel restarts no longer show working nodes as dead while PIA reconnects.

- Fixed: Check now and the check after a tunnel restart tested nodes while PIA was reconnecting and showed working nodes as dead. They now wait for PIA, up to 45 s, and test a failed node again 10 s later before they mark it dead.
- Changed: the router's sing-box builds are checked every 6 hours and published by themselves after the Linux tests, with no pull request to merge. Reality Hop still asks before it updates the router.

## 2.1.4 (2026-11-27)

**Why update:** the PC tunnel no longer keeps dead nodes while PIA is connected, router devices no longer get nodes that fail only from the router, and the router starts its tunnel again after a crash.

- Fixed: while PIA was connected, the PC tunnel kept its old node set until PIA showed "Disconnected", which PIA rarely does. It now takes the waiting set on a PIA drop, or when fewer than half of its running nodes work.
- Fixed: nodes that failed only from the router were never replaced. The hourly check now also tests the router's tunnel and replaces nodes that fail there twice, 30 s apart.
- Fixed: if sing-box on the router crashed or did not come back after a reboot, the router devices had no internet. The router's self-repair now starts it again within 2 minutes. A service turned off on purpose is left alone.
- Changed: Rust 1.100.1, cargo-deny 0.22.2, and updated crates.

## 2.1.3 (2026-11-25)

**Why update:** more nodes to choose from, speed tests that no longer fail good nodes, and Candidates shows each test as it runs.

- Fixed: the speed and lasting-connection tests failed good nodes when CacheFly sent a stand-in page. They switch to Cloudflare then.
- Fixed: a kept node that failed its test after a swap could be marked working. It is now replaced or marked failed.
- Fixed: node test failures during a PIA drop counted against the node. They are no longer counted.
- Fixed: the Fix buttons dropped the selected node when no spare tested better. They keep it.
- Changed: the node list also comes from [morpheusadam/v2ray-config](https://github.com/morpheusadam/v2ray-config), read first, next to FastNodes. The app, docs and LICENSE name it.
- Changed: slow nodes, 0 Mbps included, get a speed test of their own.
- Changed: Candidates shows each test as it runs and sorts rows as pings and results arrive. It has a Test all button. A row shows a blank 443 ping while pinging and no "?" while a failure's reason is found. The log counts failure reasons.
- Changed: the busy bar fills left to right over the time the action took last time, and also shows on top of each open dialog.

## 2.1.2 (2026-11-25)

**Why update:** the node list downloads again, the Mac tunnel stays up through PIA changes, and scans find real working nodes faster.

- Fixed: the node list returned 404 because its source moved it to a new branch. Reality Hop reads the new address.
- Fixed: on macOS the PC tunnel closed every connection each time PIA changed the routes.
- Fixed: on a Mac behind PIA the 443 ping answered in about 1 ms and meant nothing. It now times the server's reply to a TLS hello.
- Fixed: a scan during a network fault kept good nodes out of later tests. Failures that can pass later expire after 1 hour. Nodes with the wrong certificate stay out.
- Fixed: a burst of PIA drops started a blame and a node test for each drop. One of each now runs per burst, and no node swaps or speed checks run until PIA is steady.
- Fixed: a pinned node was dropped when PIA reconnected.
- Fixed: Reality Hop's leftover test addresses stayed on PIA's bypass list.
- Fixed: the window had a white scrollbar corner.
- Changed: nodes working is counted out of all 6 slots ("5 / 6", not "5 / 5"). An empty slot shows red in the tray ring.
- Changed: nodes are retested every minute, with a lasting-connection test and a minimum-speed test. Steering follows the PIA server actually in use. The log has a line per connection.
- Changed: faster node tests, fastest nodes tested first, and a fairer speed test. Scans go past the first batch and refill to 6 nodes.
- Changed: drag PC or Router onto a node to pin it. The window shows a pinned node as the one in use. Candidates has Test 50 more and a 200ms+ label.
- Changed: in France, Reality Hop looks up names with Cloudflare (1.1.1.2), since Quad9 stops serving France.
- Changed: Mac sidebar icons match Windows. The docs add Mac install screenshots and PIA DNS settings.

## 2.1.1 (2026-11-24)

**Why update:** PIA comes back faster after a drop, slow nodes are caught within the hour, and the window shows more and matches the tray.

- Fixed: after a PIA drop the tunnel kept the same node for up to 10 minutes even when it had stopped passing traffic. Every node is now tested at once on a drop, and a node that broke PIA but still passes the quick test is set aside for 10 minutes.
- Fixed: a node that slowed down after it was picked was caught only by the 6-hourly speed check. The speed check now runs with each periodic check (Settings: every 1 to 24 hours), still waiting while you game or download.
- Fixed: the tray showed a slow node in orange while the window showed it green. The window now marks it "Working, but slow", and the window icon and sidebar dot follow the tray.
- Fixed: hosting company names were cut at 22 characters. They are kept up to 60 and cut names are looked up again.
- Fixed: unfinished writes (.tmp) and other leftover files stayed in the install folder. They are removed at start, and rh-test.exe is removed after each node test.
- Fixed: the tray tooltip could lose its end past the 127 characters Windows shows.
- Changed: the node tables and Candidates show the hosting company (HOST) and size their columns to their content, keeping room for STATUS.
- Changed: the Last 24 hours chart has a Clear button, a legend entry for PIA Back, PIA downtime totals and tooltips on each event. Problem reports add PIA downtime.
- Changed: the tray tooltip shows the version.

## 2.1.0 (2026-11-23)

**Why update:** far fewer PIA drops, most of all with two PCs on one router, and no speed test while you game.

- Fixed: when another PC changed the router's nodes, this PC restarted its tunnel at once and PIA dropped. It now keeps its working nodes while PIA is connected and takes the new set when PIA is not connected or at its next restart.
- Fixed: a node that breaks long connections passed every check and dropped PIA every few minutes. A node in use at 3 PIA drops within an hour is now replaced at once and kept out for 12 hours.
- Fixed: Start after Stop, or turning the background watcher back on in Settings, did not restart the watcher until Reality Hop was reopened.
- Fixed: PIA's state feed kept running after the watcher stopped.
- Fixed: the window and the watcher could change the nodes and the tunnel at the same time, and the window's Cancel could stop the watcher's work. They now take turns, and Cancel stops only the window's action.
- Changed: the 6-hourly speed check waits while traffic goes through the tunnel (a game, a call, a download), for up to a day. Swapping a slow node that still works no longer restarts the tunnel while PIA is connected.
- Changed: nodes taken from the router are no longer download-tested on the spot.
- Changed: anonymous problem reports add the PC tunnel and PIA state, each node's pings over 48 hours, and a 48-hour activity history. Everything personal is still taken out.
- Changed: rh-test.exe is a hard link to rh-core.exe instead of a second 79 MB copy, and waiting for a program uses less CPU on Windows.

## 2.0.7 (2026-11-21)

**Why update:** Check for updates in Settings now shows the update window when there is an update.

- Fixed: Check for updates in Settings found the update but did not show it. Settings now closes and the update window opens.

## 2.0.6 (2026-11-21)

**Why update:** the Last 24 hours legend no longer shows each label twice.

- Fixed: the Last 24 hours legend could show PIA Dropped, Node Replaced, Tunnel Restart and Rescue twice when the chart drew twice in quick succession as the window opened.
- Changed: jiff 1.0.2.

## 2.0.5 (2026-11-20)

**Why update:** Stop and Restart now really close the PC tunnel, and the folder and GitHub buttons work again.

- Fixed: Stop left the PC tunnel running when Windows would not let Reality Hop close it, so the VPN app could still connect. Stop now closes it in the same UAC step.
- Fixed: Restart failed with "an old rh-core would not stop" and the tunnel kept the old nodes. Reality Hop now asks once for UAC to close it, then starts it on the new nodes.
- Fixed: Open backend folder, Open logs folder, Star on GitHub and All changes on GitHub did nothing on Windows.
- New: click the PC tunnel tile when it says Needs restart to restart the tunnel.
- New: click a problem in the status line to go to the activity log; logs/core.log in it opens that file.
- New: the update window asks for a star on GitHub every time.
- Changed: toml 1.2.2, toml_edit 0.25.20 and toml_parser 1.1.6.
- Changed: every line in the activity log, the setup log, the status line and notifications starts with a capital letter. Ctrl+A selects only the log, and the progress bar stays at the top when you scroll.

## 2.0.4 (2026-11-20)

**Why update:** labels and status lines in the window start with a capital letter, in every language.

- Changed: status lines, dialog states, setup steps and the tray tip now start with a capital letter (for example "Checking", "Not tunnelled", "Next auto-check"), in all 9 languages.
- Docs: the README lists the networks Reality Hop works on with capital letters, in English and the translations.
- Changed: url 2.5.9, form_urlencoded 1.2.3, walkdir 2.5.1, and on Linux builds zbus 6.5.0.

## 2.0.3 (2026-11-20)

**Why update:** the window opens at a size that shows everything, and an update you start yourself no longer shows its notes twice.

- Fixed: the window opened a little too short, with a scroll bar. It now grows to fit on its first draw, within the screen.
- Fixed: after you install an update with the Update button, Reality Hop goes back to the main page. What's new shows only after an automatic update.
- Fixed: on a Mac, each update left one more Reality Hop in the Dock's recent apps. Updates now keep the same app folder.
- Docs: step-by-step help for the first start on a Mac (System Settings → Privacy & Security → Open Anyway), in the README, the troubleshooting guide and the release notes.
- Changed: cc 1.11.3, and on Linux builds zcheapstr 1.4.0 and zruntime 0.2.4.

## 2.0.2 (2026-11-19)

**Why update:** after an update Reality Hop now tells you and shows what changed, and the tray icon matches the window again.

- New: the first time you open Reality Hop after an update, it shows what changed since the version you had.
- New: after an automatic update in the background, a notification says which version is now installed.
- New: the setup welcome page and the What's new window ask for a star on GitHub, with a link.
- Fixed: the tray icon went back to an older result after the window checked the nodes, so the two disagreed.
- Changed: the Windows download is about 220 KB smaller. Reality Hop no longer builds in Tauri features it never uses.
- Docs: new screenshots in all 9 languages.
- Changed: serde_with 3.25.0, and on Linux builds zbus 6.4.0 and zcheapstr 1.3.0.

## 2.0.1 (2026-11-19)

**Why update:** Reality Hop runs fewer background commands on Windows, so security scanners have less reason to flag it.

- Changed: on Windows, Reality Hop finds and stops its old copies, watchers and PIA monitors through Windows itself, not through PowerShell and WMI.
- Changed: on Windows, a PC without the boot task no longer asks Task Scheduler about it at every start. The answer is kept in `this-pc.json`.
- Fixed: the main window shows "PC + Router" with a capital R.
- Docs: the README badges have rounded corners in all 9 languages.
- Changed: pin-project-lite 0.2.18, and the build checks dependencies with cargo-deny 0.22.0.

## 2.0.0 (2026-11-19)

**Why update:** Reality Hop is rebuilt in Rust: a much smaller download, with the same nodes, settings and router setup.

- Changed: the whole app is rewritten in Rust with [Tauri](https://github.com/tauri-apps/tauri). The Windows download is 12 MB instead of 43 MB, and each Mac disk image is 4.2 to 4.6 MB instead of 15 to 16 MB.
- Changed: on Windows, the install is one `RealityHop.exe`, with no `app-<version>` folder to unpack. An update swaps the exe in place.
- Changed: the window and the background watcher run in one process. Opening Reality Hop while the watcher runs opens its window.
- Changed: a new look with rounded corners and floating panels in the window, the dialogs and the setup wizard.
- Kept: an update from 1.x keeps your settings, nodes, vault, history, logs, router setup, start at logon and start at boot. There is nothing to set up again.
- New: on a Mac, starting Reality Hop from the disk image now asks you to drag it into Applications first. A login item that points at the disk image stops working after a restart.
- New: on a Mac, if Reality Hop was moved to a different folder, its login item points at the new place at the next start.
- Note: on Windows, the window uses Microsoft Edge WebView2. It is part of Windows 11 and of current Windows 10.
- Docs: new screenshots in all 9 languages, and a build guide for the Rust source. The Python source, its build files and the Python build tools are removed from the repo; the release checks and the router build of sing-box now run in Rust (`rust/tools`). 1.7.2 and older stay in the releases and the git history.

## 1.7.2 (2026-11-17)

**Why update:** PIA's Split Tunnel sets itself up on Windows, France gets a working DNS choice, and updates arrive sooner.

- New: on Windows, Reality Hop adds rh-core.exe and rh-test.exe to PIA's Split Tunnel as Bypass VPN by itself. Your own rules stay, and uninstalling removes only its rules. The manual steps and screenshots stay in the docs for other VPN apps and in case it fails.
- New: the first setup asks, after the language, whether you live in France. Quad9 stops serving France on 24 November 2026 after a Paris court order; "Yes" switches the PC and router configs to Cloudflare (1.1.1.1).
- New: Show candidates keeps testing the next nodes that answer on 443 until 6 work or none are left.
- Changed: the app update check runs 2 minutes after logon, then every 6 hours (was 15 minutes, then daily). Each check is 1 GitHub API call; the limit is 60 an hour per IP.
- Changed: the Mac app leaves out image libraries it never uses, and the disk image uses stronger (LZMA) compression: a smaller download.
- Changed: the release build tests each Mac chip as soon as its own build is done, so a release finishes sooner.
- Fixed: on a Mac, after an update the new copy removed the old copy at once instead of 10 minutes later, while the old one could still be exiting.
- Docs: get PIA for Mac from its website, not the Mac App Store: the App Store version has no Multi-Hop or Split Tunnel.

## 1.7.1 (2026-11-17)

**Why update:** the install steps in every language now cover the Mac.

- Fixed: the translated READMEs named only the Windows download. Each now lists the Apple silicon and Intel `.dmg` files and macOS 15 or newer.
- Fixed: the build guide now says each release has the exe and both Mac disk images, with a SHA-256 for each.

## 1.7.0 (2026-11-17)

**Why update:** Reality Hop now runs on Macs, Apple silicon and Intel, with PIA's kill switch on.

- New: macOS app (macOS 15 or newer). Each release has `RealityHop-mac-apple-silicon.dmg` and `RealityHop-mac-intel.dmg`: drag Reality Hop to Applications. It keeps its data in your user's Library, starts at login without an admin password, shows the watcher in the menu bar and updates itself from the release.
- New: on a Mac, Reality Hop adds itself to PIA's Split Tunnel list (Bypass VPN) and keeps the 6 node addresses there, so PIA's kill switch and Advanced Kill Switch stay on. New nodes are added before the tunnel switches to them, old ones removed after; your own rules stay.
- New: Check PIA on a Mac warns when another DNS app (such as NextDNS) is on, which stops every name from resolving while PIA is connected.
- Fixed: while Reality Hop is stopped, a node swap or restart no longer starts the tunnel; the new nodes are used at the next Start.
- Fixed: after a name lookup gets no answer, downloads go through the tunnel first for 10 minutes instead of waiting 30 seconds each time.
- Fixed: the locations of the nodes picked during setup fill in at the first check instead of staying "?".
- Changed: on a Mac the window says "Mac" where Windows says "PC", in all 9 languages.

## 1.6.2 (2026-11-16)

**Why update:** in Persian and Arabic, sing-box versions like 1.15.0-alpha.13 now read correctly.

- Fixed: in Persian and Arabic, versions with a suffix (1.15.0-alpha.13) showed with their parts swapped in the sidebar, Settings, the router update window and the status line. They are now marked left to right.
- Docs: new screenshots in all 9 languages, in a new gallery layout.

## 1.6.1 (2026-11-16)

**Why update:** the window now says when the router cannot be reached, instead of showing old router results as if they were current.

- Fixed: away from the router's network, the router parts of the window showed nothing or stale data. Now the Router tunnel tile shows "Not connected", the Router devices card shows "Router not connected", and the sidebar shows it under the router's sing-box version. All three clear on the next check once the router answers again.

## 1.6.0 (2026-11-15)

**Why update:** the router can now run a current sing-box, built small enough for its flash, instead of GL.iNet's 1.9.7.

- New: Reality Hop's own router build of sing-box (arm64 routers). Same official source, with everything the router does not use left out: 18 MB instead of 27 MB. Stable (1.14.4) and test builds (1.15.0-alpha.13).
- New: Reality Hop checks for a new router build once a day and asks before it updates the router. The update checks the SHA-256, tests the new build against the router config, keeps the old build in the router's RAM and puts it back if the new one does not start.
- New: Settings sets the router's channel: Same as this PC, Stable or Test builds. The sidebar shows the sing-box version on the PC and on the router.
- Changed: Push to router sends the config in the format the router's sing-box reads (the 1.12+ format for new builds, the old one for GL.iNet's package). DNS stays Quad9 over HTTPS through the tunnel.
- Note: the router's Plug-ins page keeps showing GL.iNet's 1.9.7 package. The sidebar shows the version that really runs. See router-core/README.md.

## 1.5.8 (2026-11-14)

**Why update:** "in use by Router" now shows right after a node change, and Check for updates can be clicked again and again.

- Fixed: after Replace all, a fix, Push to router or Rescue, the tables did not show the router's node until the next Check now. The app now asks the router right after the push, and once more 20 s later in case the router switched node.
- Changed: Check for updates stays clickable and asks GitHub on every click. The answer shows under the buttons, with the time, instead of in a box you had to close.

## 1.5.7 (2026-11-14)

**Why update:** the node tables now show correctly when the PC and the router use the same node.

- Fixed: when the PC and the router used the same server, only "in use by PC" showed. Both tables now show "in use by PC & Router".
- Fixed: the tables matched the active node by slot name, so a slot with the same name but a different server could be marked. They now match by server address.
- Changed: Check now also asks the router which node it uses, with one quick request. It runs only when the router is reachable and tests no nodes.

## 1.5.6 (2026-11-14)

**Why update:** the activity log and problem reports stay fully in English, so support can read them in every language.

- Fixed: in Persian, Arabic and the other translations, the node check wrote "PC tunnel" and "Router devices" into the activity log in the chosen language. It now writes them in English, like the rest of the log.

## 1.5.5 (2026-11-14)

**Why update:** the node tables now show which site each node disguises your traffic as.

- New: a FRONT column in the node tables on the main tab names the site the node poses as, for example Cloudflare, Yahoo, Apple or Microsoft. The node details window still shows the full address.

## 1.5.4 (2026-11-13)

**Why update:** at some window sizes, opening Settings froze the window and used half a CPU core. That no longer happens.

- Fixed: the overview tiles could resize each other in an endless loop when one hint sat exactly on a line-break boundary (for example a 1300 px wide window, 1000 px tall or less). The window then froze when Settings opened.

## 1.5.3 (2026-11-13)

**Why update:** Reality Hop now speaks 9 languages, tests 50 candidates instead of 40, and a node test can no longer fail or hang because of a busy port, a slow PC or a stalled download.

- New: 9 languages: English, Spanish, Simplified Chinese, Russian, Ukrainian, Persian, Arabic, Turkish and Vietnamese. Pick one at the first start; change it in Settings → Language. Persian and Arabic read right to left. The activity log and problem reports stay in English.
- New: translated guides in docs/lang, linked from the top of the README.
- Changed: each scan tests the 50 fastest candidates instead of 40. A saved setting of 40 (the old default) moves to 50 once.
- Changed: the node test runs 10 checks at a time instead of 5 and starts as soon as the test instance is ready, not after a fixed 2 s.
- Fixed: on a slow PC the test instance could take longer than 2 s to start; every node then failed and working nodes could be replaced.
- Fixed: the node test used fixed local ports; when another program or a second test held one, the whole test failed. It now picks free ports.
- Fixed: a download that only trickled (as some filters do) had no time limit and could hold up a check for hours. Every GitHub download now stops after a set time and tries the next source.
- Fixed: the window's activity log grew without limit when left open for days; it keeps the last 2000 lines.
- Changed: the watcher log now shows the result of the 30 s re-check after a node fails.
- Security: the release build uses softprops/action-gh-release 3.0.4, and the release check now refuses any GitHub Action that is not on its newest release.

## 1.5.2 (2026-11-09)

**Why update:** faster speed tests, Replace all keeps your fast nodes, and the background watcher comes back after you quit it.

- Fixed: after Quit in the tray menu, opening Reality Hop did not start the background watcher (and its tray icon) again. It does now, when "run in the background at logon" is on.
- Fixed: Replace all 6 in Candidates replaced every slot even when fewer candidates were fast, and could put in nodes that sent nothing back in the speed test. It now uses only nodes that reach the minimum speed and replaces only that many slots; a node with no answer counts as 0 Mbps.
- Fixed: Candidates did not close after Replace all when the check ran while the restarted tunnel was still starting. It now waits for the tunnel, always closes, and shows a failed slot in the main window.
- Fixed: the window sometimes checked the PC tunnel two or three times after an action: the watcher sent the window's own earlier result back to it.
- Changed: the speed test runs up to 8 nodes at once first; only nodes it cannot judge get a test of their own. A 40-node scan takes about 35 s instead of 41 to 56 s.
- Changed: the PIA Split Tunnel rule for rh-core.exe is needed only when All Other Apps is set to Use VPN. The setup and the guides now say so.

## 1.5.1 (2026-11-09)

**Why update:** Reality Hop now picks nodes by download speed, not only by ping, so your VPN no longer rides a node that gives 1 to 5 Mbps.

- Fixed: the node with the lowest ping always got the traffic, even when it downloaded at 1 to 5 Mbps. Every node search (Replace, Replace all, Candidates, Rescue, the first setup) now also measures download speed through each working node and puts nodes that reach at least 100 Mbps first.
- Changed: when your line is slower than 100 Mbps, a node needs 70% of the fastest node seen in the last 24 hours (at least 50 Mbps). On a 100 Mbps line the best node gives about 93 Mbps, so a fixed 100 would reject every node.
- Changed: every 6 hours the background check also measures the nodes in use. A node under the minimum on two tests in a row is replaced with a spare that reaches it; with no such spare, nothing changes. Each test downloads up to about 75 MB per node.
- Changed: the minimum is `min_mbps` in settings.json (100 by default; 0 turns the speed test off).
- Changed: the setup and the VPN guide now show the PIA Split Tunnel rule: rh-core.exe set to Bypass VPN. Without it the VPN cannot connect.

## 1.5.0 (2026-11-09)

**Why update:** it closes a command injection in the setup, keeps website names out of problem reports, and makes tunnel restarts and checks faster.

- Security: a folder or user name with a typographic apostrophe (’) ended a quoted PowerShell command. The install then failed with "the UAC prompt was declined", and a crafted folder name could run its own command with admin rights. Every quote character is now escaped.
- Security: the problem report left website names that end in .sh, .md, .py, .zip, .pub or .new readable (crt.sh, for example). They are now replaced like every other website.
- Fixed: Add key to router did not save the router's host key when the install folder had a space in its path (the default C:\Program Files), and did not run at all from a folder with an & in its name.
- Fixed: on new Windows builds Reality Hop could not tell which program holds port 2080. It now reads Windows' own list of listening ports.
- Fixed: with router features on and rh-core missing, replacing a dead node stopped with the error 'ms'.
- Fixed: on a slow start the "Start tunnel at boot" switch showed off and the sing-box version stayed blank.
- Fixed: Check for updates froze the Settings window until GitHub answered.
- Changed: a tunnel restart is faster (5.5 s down to 0.5 s in a test without the boot start).
- Changed: Check now looks up the exit address once, while the nodes are tested (about 3 s down to 1 s in a test).
- Changed: the background check tests each node once, not twice, and no longer looks up the exit address.
- Changed: the Update button's animation uses about a third of the processor time it did.

## 1.4.4 (2026-11-06)

**Why update:** the log now shows when an action has finished.

- Changed: each window action ends with a log line, "done (N s)" or "cancelled", so a finished check no longer looks stuck.

## 1.4.3 (2026-11-05)

**Why update:** the window and the tray icon now always show the same node state.

- Fixed: with the window open, its taskbar icon kept an old result after the watcher re-tested the nodes or restarted the tunnel, so it could show red while the tray showed green. The window now re-checks as soon as the tray shows something different.
- Fixed: when the nodes had changed since the window was last closed, the window opened with the old nodes' results. It now starts on "checking" instead.
- Changed: the buttons under Settings (Check for updates, Run setup again, Change router, Uninstall) have an outline that turns red on hover.
- Changed: the exe's file properties now include a description.

## 1.4.2 (2026-11-04)

**Why update:** fixes the crash dialog some people saw while updating to 1.4.1, keeps the tray icon current after a repair, and does less that antivirus sandboxes flag.

- Fixed: updating from 1.4.0 could show "Error -5 while decompressing data". The copy that switches the install to the program folder now stops the other Reality Hop processes first and starts them again from the new exe. Every update now stops them before the exe is swapped.
- Fixed: after the watcher replaced a dead node or restarted the PC tunnel, the tray icon kept showing the old, dead nodes until the next hourly check. It now tests the nodes that run and updates the icon right away.
- Fixed: on some PCs the install failed with "Access is denied" while it moved the new program folder into place (a virus scanner was still reading the new files). Reality Hop now waits for the scan and, if needed, moves the files one by one.
- Fixed: the Control Panel entry repair no longer depends on the cleanup of old files succeeding.
- Security tools see less to flag:
  - rh-core checks certificates against sing-box's own Mozilla root list (sing-box 1.12 or newer), so Windows no longer adds root certificates on its behalf.
  - Downloads use Mozilla's root list bundled with Reality Hop, not a PowerShell web request, on a fresh Windows.
  - Steps that need administrator rights run as readable PowerShell text, not an encoded command.
  - The tunnel process is checked and stopped directly, not with `tasklist` and `taskkill`. The check is also faster.

## 1.4.1 (2026-11-04)

**Why update:** Reality Hop starts about 90 seconds sooner after logon, opens in a fraction of a second, and uninstalls cleanly.

- Faster logon start: the background watcher now starts from a Task Scheduler task a few seconds after logon, not from the Startup folder (about 90 s later on a busy PC). It runs as you, not elevated. The first node check still waits 90 s, so a network that is not up yet does not count as dead nodes.
- Faster launch: Reality Hop now runs from its program folder (`RealityHop.exe` plus `app-<version>`), so nothing is unpacked to `%TEMP%` at each start. A window opens in about 0.3 s instead of 1 s or more, and the first start after an update no longer waits about 13 s for the antivirus scan of the unpacked files.
- Fixed: a long-running watcher could lose its unpacked files to a temp cleaner, and router updates then failed with "No such file or directory".
- Updates still come as one `RealityHop.exe`. Reality Hop checks its SHA-256, then the new exe puts its program folder next to the running one. The first start after this update switches your install over by itself.
- The PC tunnel now runs at normal priority, not below normal. Existing installs are asked once (one UAC prompt).
- Fixed: uninstalling with Reality Hop's window open left the program folder behind. Uninstall now stops every Reality Hop process first, and also removes what Windows keeps about the app (tray icon entry, Start menu tile cache, name cache, usage count).
- Fixed: an install made by 1.0.x kept a Control Panel entry that pointed at a folder that is gone, so "Uninstall a program" failed. The entry is repaired the first time the window opens.
- Fixed: on a freshly installed Windows the setup could fail with "CERTIFICATE_VERIFY_FAILED". Reality Hop now lets Windows fetch the missing root certificate and tries again.
- The uninstall dialogs show the Reality Hop icon, not the Tk feather.
- Docs: Control Panel > Programs > Uninstall a program is listed as a way to uninstall.

## 1.4.0 (2026-11-02)

**Why update:** replacing a node now always restarts the PC tunnel, and a node that just died in your tunnel is not offered again.

- Fixed: when the router could not be reached, "Fix this node", "Replace all" and the Candidates buttons stopped before restarting the PC tunnel. The PC tunnel now restarts anyway, and the status says the router was not updated.
- Fixed: a node that failed in your PC tunnel could come back as a candidate right after you replaced it. Such nodes are now skipped for 12 hours, or until they work in your tunnel again.
- Candidates pings and tests the 40 fastest on its own when it opens. The "Test 40 fastest" button is gone; "Ping and test again" does both.
- "Put picked in slots" now restarts the tunnel, checks the slots and closes Candidates when they work, like the other replace buttons.
- The picked list in Candidates shows real arrows (vless-1 ← node).

## 1.3.9 (2026-11-02)

**Why update:** putting one node into a slot from Candidates now takes effect right away.

- Fixed: "Put this one into vless-N" (right-click in Candidates) and the vless-1 to vless-6 buttons only wrote the configs. They now restart the tunnel, check the slot, and close the window when the node works.

## 1.3.8 (2026-11-02)

**Why update:** trackpad scrolling works, and replacing nodes from Candidates only puts in nodes that work.

- Fixed: a trackpad could not scroll the window when it was too small for its content.
- Fixed: "Replace all 6 with the best 6" in Candidates could put in untested nodes that did not work. It now uses only tested working nodes and tests first when too few passed.
- Fixed: "Replace all 6 with the best 6" now restarts the tunnel, checks every slot, swaps a node that fails once more, and closes the window when all work.

## 1.3.7 (2026-10-31)

**Why update:** the window opens without flashing.

- Fixed: the window and dialogs flashed twice when they opened. They now appear once, fully drawn.

## 1.3.6 (2026-10-31)

**Why update:** a tidier install folder.

- Tray icons now go in their own `tray-icons` folder. The old ones in the main folder are removed automatically.

## 1.3.5 (2026-10-30)

**Why update:** security fixes for the router connection and the installer, actions wait their turn, and the background
watcher uses less memory.

- Security: the router's SSH host key is now really checked. In a folder with a space in its name, such as Program
  Files, it was never saved, so a different device at the router's address went unnoticed.
- Security: when another PC or the router changes the router's nodes, this PC follows only nodes that pass the same
  checks as the public node list, and takes nothing else from the router's config.
- Security: Windows programs start by their full path, so a file with the same name in the Downloads folder cannot run
  in their place.
- Security: problem reports also redact website names without a port, wildcard names, short IDs and other users' folders.
- Security: the release build uses pinned tools and actions.
- Actions now queue. Press Update, or anything else, during a check and it runs when the check finishes.
- Clicking away from a node clears its highlight.
- The background watcher loads no window code and gives its memory back to Windows while idle.
- Uninstalling with "keep my settings" now removes the program, and a later install starts normally.
- Uninstalling with your data removed and the router tunnel switched off also takes this PC's key off the router.
- Fixed: saving could fail while the other process read the same file, the router's self-repair could stay locked until
  a reboot, Start could leave the boot start off on non-English Windows, and the tray icon cache could grow without limit.

## 1.3.4 (2026-10-30)

**Why update:** the tray icon now matches the window.

- While the window is open, the tray icon shows the same node ring and status as the window, and keeps that result
  after you close the window. Before, it kept showing the last hourly check.

## 1.3.3 (2026-10-30)

**Why update:** resizing the window is smooth, and nothing gets cut off when it is narrow.

- Resizing no longer stutters. The content settles into place when you pause the drag.
- In a narrow window, tile hints wrap, node tables keep the status readable, and the 24-hour counters move to their own
  line. The status pill hides when the header buttons need the room. The smallest window width is now 1060.
- The Update button has an RGB snake around it and a shine passing over it.
- A darker red throughout the app. Links and error states keep their brighter colors, so they stay easy to read.
- The Update button shows its tooltip again.

## 1.3.2 (2026-10-29)

**Why update:** the main window can now be made smaller.

- The window shrinks down to 1000x560. When it is shorter than its content, the sidebar and the main area scroll, each on
  its own, with the mouse wheel or a scrollbar.

## 1.3.1 (2026-10-29)

**Why update:** the setup's welcome page explains better where Reality Hop helps.

- The welcome page names more places that block VPNs: public Wi-Fi, and countries that filter the internet, such as
  China, Russia or Iran. Its example front site is cloudflare.com.
- It also reminds you to check that a VPN is allowed in the country you are in.

## 1.3.0 (2026-10-29)

**Why update:** router steps only run on your own router's network, updates show what changed in a clear window, and
every button explains itself.

- **Your router only.** Before every router step (Push, Test on router, Check now, the background repairs), Reality Hop
  checks that this PC is on its own router's network: the router's address must answer, with the hardware address it
  had when it was set up. Away from home the router steps are skipped at once, instead of waiting for timeouts, and
  nothing is sent to another network's router. The PC tunnel keeps working.
- **Change router** in Settings: for a new or reset router. It forgets the old one and opens the router steps of the
  setup.
- **Update window.** A new release opens a window at launch with what changed in every version since yours, and
  **Install update** or **Later**. The Update button stays at the top.
- **Hover help.** Every button shows what it does when you hold the mouse over it.
- **Test PC tunnel** in Candidates: after you put nodes in the slots and restart, test the PC tunnel without going
  back to the main window.
- Menu paths in the app use arrows (Settings → Multi-Hop).

## 1.2.0 (2026-10-28)

**Why update:** Reality Hop is now built with the newest Python and build tools, and every release is checked for that
before it goes out.

- Built with Python 3.15 (was 3.14), the newest PyInstaller and Pillow, and the newest GitHub Actions (Node.js 24).
- A release can no longer go out on an outdated backend: a check of the build tools runs first in the release workflow
  and before a version tag is pushed, and stops the release until they are updated.

## 1.1.4 (2026-10-28)

**Why update:** problem reports are easier to read, for you and for whoever looks at the issue.

- **Report a problem** is laid out in clear sections: a setup table, then every problem found in the logs in time
  order, then the watcher, window and tunnel engine logs, and a redaction key at the end.
- The tunnel engine log shows each repeated error once, with how often it happened and since when, instead of many
  near-identical lines. Connection IDs and local port numbers are left out.
- The report window says "redacted" for the personal data it takes out.

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
  `%LOCALAPPDATA%\RealityHop`. The setup can install to **another folder** (Install step → Change...). After updating,
  the window offers to move an existing install there once (one admin prompt; the tunnel stops for a few seconds and the
  boot start follows the move).
- **The install folder is yours alone.** Only your Windows user, SYSTEM and administrators can open it, because it holds
  the router SSH key, the node keys and the API secret. Before, every account on the PC could read Program Files.
- **Name:** Reality Hop, with a space, everywhere you see it: the window, the tray, the Start menu, Windows search and
  Apps & features. The Start menu, desktop and logon shortcuts are renamed to match. Task Manager shows the name too.
  File and folder names stay `RealityHop` (`RealityHop.exe`, the install folder, the GitHub repository).
- **Router only (new):** the setup's router question has a third answer, **Router only**, also in Settings. Nothing then
  runs on this PC's connection: the router tunnels the devices you tick, or, with the new **Every device on the main
  network** switch (setup and Router → Devices), every device on the router's main Wi-Fi and LAN, this PC included. Guest
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
- **Router → Devices and the setup:** the address reservation steps were wrong. It is GL.iNet web panel → **CLIENTS**,
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
  time it runs. If you hide it yourself later (Windows Settings → Personalization → Taskbar → Other system tray icons),
  your choice is kept.

- **Router → Devices** (new window): every device the router knows, with the ones in the tunnel ticked. Tick to add,
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
  watcher, restart Reality Hop once (tray → Quit, then open it from the Start menu) so the fixed version takes over cleanly.
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
