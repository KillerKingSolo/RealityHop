# Security

## Reporting a problem

Please report security problems privately through GitHub's **Report a vulnerability** button
(Security → Advisories) on this repository, not in a public issue.

## What Reality Hop does and does not do

- No telemetry. The only network requests are listed in the README under "Privacy and security".
- The router password is typed into Windows' own `ssh` in a console window; Reality Hop never sees or stores it. The
  router is reached with an SSH key created on your PC (`router_ed25519` in the install folder).
- The install folder (`C:\Program Files\RealityHop` by default) holds the program, the configs, the keys and the logs.
  The setup makes it readable by your Windows user, SYSTEM and administrators only.
- sing-box and Reality Hop updates are checked against the SHA-256 checksum published with the release before they
  replace anything. A new sing-box build must also accept the current config first.
- The local control API of sing-box (127.0.0.1:9090) uses a random secret created at install, and no web page may
  call it (CORS closed).
- Public nodes are run by strangers. Always keep a VPN on top; a node then sees only encrypted VPN traffic.

## Do not share

`pc-config.json`, `router-config.json` and `vault.json` contain node keys and your local API secret. Leave them out of
issues and screenshots. For a normal problem report, use **Report a problem** in the window: it replaces addresses,
names, keys and secrets with neutral tokens, and nothing is sent until you attach the file yourself.
