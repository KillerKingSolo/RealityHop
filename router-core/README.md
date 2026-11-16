# Router sing-box (router-core)

Reality Hop's build of [sing-box](https://github.com/SagerNet/sing-box) for the router. Same official source, with everything the router does not use left out. That makes it small enough for routers with little flash and RAM.

| | GL.iNet package (1.9.7) | This build (1.14.4) |
|---|---|---|
| Binary (linux/arm64) | 26.7 MB | 18.4 MB |

## What is in it

- **Inbounds:** tproxy (the device diversion), direct (the DNS inbound), mixed (the node test).
- **Outbounds:** direct, block, urltest, selector, VLESS with Reality and uTLS.
- **DNS:** DNS over HTTPS (Quad9), and the system resolver, which sing-box falls back to when a config has none.
- **Also kept:** the Clash API, which Reality Hop reads the router through.

Left out: every other protocol (Shadowsocks, VMess, Trojan, TUIC, Hysteria, WireGuard, Tailscale, Tor, ...), TUN, the V2Ray transports, the debug server, the cache file and every CLI command except `run`, `check` and `version`. A config that asks for one of them stops with an error.

## How it is built

`build.py` builds the binary:

1. It gets the official source at the release tag.
2. It deletes the left-out CLI commands and protocol registrations.
3. It copies in the files under `overlay/`.
4. It makes two exact edits to `box.go`.
5. It builds with the official release flags and the Go version in `version.json`.

The same inputs give the same file, so anyone can rebuild a release and compare the SHA-256.

```
python build.py --verify
```

## How it gets to routers

1. **Daily check:** the [router-core workflow](../.github/workflows/router-core.yml) looks for a new sing-box release (stable and preview) and a new Go.
2. **Pull request:** when it finds one, it builds and tests the new version on Linux and opens a pull request with the new `version.json`.
3. **Merge:** merging publishes the build to the rolling releases `router-core-stable` and `router-core-preview`.
4. **Offer:** Reality Hop checks once a day. When the router does not run the newest build of its channel, it asks first. It shows the version the router runs now and the new one.
5. **Install:**
   1. Download, check the SHA-256, upload to the router's RAM.
   2. The new build must run and accept the router config.
   3. The old binary is kept in RAM, then the new one is swapped in.
   4. If it does not start, the old binary and config go back.

The router's channel follows the PC's (Settings) unless it is set on its own: Same as this PC, Stable or Test builds.

Only arm64 routers (`uname -m` = aarch64) get this build. Other routers keep the GL.iNet package.

## The router's Plug-ins page still shows 1.9.7

This is expected. GL.iNet's **APPLICATIONS > Plug-ins** page reads the package list, not the program. The update replaces only the program file `/usr/bin/sing-box`. The package record stays as GL.iNet installed it, on hold, so the router's own updater does not put 1.9.7 back.

To see what really runs, look at the Reality Hop window: the sidebar shows `Router: <version>`. Or run this on the router (SSH):

```
sing-box version            # the version that runs
cat /etc/sing-box/rh-core   # Reality Hop's record: channel, version, SHA-256
```

Do not press **uninstall** for sing-box on the Plug-ins page. It deletes the program, the router config and the service script, and the router tunnel stops. To go back to GL.iNet's version, follow the next section.

## Back to the GL.iNet package

On the router:

```
opkg flag user sing-box && opkg install --force-reinstall sing-box && rm /etc/sing-box/rh-core
```

Then press Push to router. The push sends the config in the format the router's sing-box reads.
