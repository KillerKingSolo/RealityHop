# Changelog

## 1.0.1

- VPN instructions: OpenVPN over **TCP port 443** (setup wizard, README, docs).
- Check PIA also checks the protocol (OpenVPN), the transport (TCP) and the remote port (443).

## 1.0.0

First public release.

- Setup wizard: installs to `%LOCALAPPDATA%\Programs\RealityHop`, downloads sing-box, finds and tests the first nodes,
  starts the tunnel, guides the VPN app setup, and sets up the optional background start and start at boot.
- Optional GL.iNet router support, set up step by step in the wizard (SSH key, sing-box on the router, devices).
  Router features can be turned off, which hides the router section.
- Background watcher with a tray icon: hourly node checks, automatic replacement and rescue, instant reaction to PIA
  drops.
- The tray and window icon show the last known state at once at logon. The first check runs as soon as the tunnel
  answers.
- Stop / Start button for when you are away; Stop also turns off the start at boot.
- Automatic updates from GitHub Releases, checked against the published SHA-256 checksum.
- Uninstall from Settings or Windows' Apps list.
- sing-box channel: stable (default) or test builds (beta, alpha). sing-box updates itself daily, separately from
  RealityHop releases.
- Backend files: an option that shows buttons for the `PC`, `Router` and `logs` folders.
- Report a problem: an anonymous copy of the three logs, ready to attach to a GitHub issue.
- License: PolyForm Noncommercial 1.0.0 (no selling, no commercial use).
