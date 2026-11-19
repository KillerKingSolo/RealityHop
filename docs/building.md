# Building from source and releasing

Reality Hop is written in Rust with [Tauri 2](https://github.com/tauri-apps/tauri). The window is HTML, CSS and
JavaScript in `rust/app/ui`, shown by the system's web view: WebView2 on Windows, WebKit on macOS. Install Rust with
[rustup](https://rustup.rs); the version is pinned in `rust/rust-toolchain.toml`, and rustup fetches it on the first
build.

## Run from source

```
cd rust
cargo run -p realityhop-app
```

A development build never installs, updates or moves anything. It keeps its data in `RealityHop-dev` in your local app
data folder (`%LOCALAPPDATA%` on Windows, `~/Library/Application Support` on a Mac), or in the folder in
`REALITYHOP_DATA` if that is set. It never touches an installed Reality Hop.

## Build

Windows (MSVC toolchain, which links the WebView2 loader into the exe):

```
cd rust
cargo build --locked --release -p realityhop-app
```

The result is `rust\target\release\RealityHop.exe`, the one file that is released. It needs nothing next to it.

Mac (on a Mac with the chip you build for):

```
cd rust
sh build-mac.sh
```

The result is `rust/dist/RealityHop-mac-apple-silicon.dmg` or `rust/dist/RealityHop-mac-intel.dmg`, with
`RealityHop.app` (ad hoc signed, not notarized) and a link to Applications.

## Test

```
cd rust
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

`rust/parity` holds the outputs of the 1.x code for the same inputs. The tests check that 2.0 still returns the same,
so configs, settings and data files from 1.x keep working.

## Release

Releases are built by GitHub Actions ([`.github/workflows/release.yml`](../.github/workflows/release.yml)):

1. Set the version in `rust/Cargo.toml` and add its section to [CHANGELOG.md](../CHANGELOG.md).
2. Check the build backend: `python tools/check-backend.py`. It lists the GitHub Actions, the Rust toolchain, the
   pinned crates and cargo-deny, and checks that every crate in `rust/Cargo.lock` is at the newest version allowed
   (`cargo update --dry-run`). It fails while one is out of date; a crate held back by the crate that uses it is
   listed but allowed. A release cannot go out until it passes: the
   workflow runs it first, and the pre-push hook runs it before a `v*` tag leaves your PC (turn the hook on once per
   clone with `git config core.hooksPath .githooks`).
3. Tag and push:
   ```
   git tag v2.0.0
   git push origin v2.0.0
   ```
4. The workflow runs every check and test on Windows and on macOS 15, 26 and 27, refuses a tag that does not match the
   version in `rust/Cargo.toml`, and builds `RealityHop.exe` and the two Mac disk images. It tests each disk image on
   every macOS version, on its own chip and through Rosetta, then publishes a release with the three files. The release
   notes are the CHANGELOG section for that version, plus the SHA-256 checksums. A tag with a suffix
   (`v2.1.0-beta.1`) is published as a prerelease, which installed copies do not update to.

Installed copies check for a new release when the window opens and every 6 hours. They download the new file and check
it against the release's SHA-256. On Windows, the running `RealityHop.exe` is renamed out of the way
(`RealityHop.old-<time>.exe`, removed at the next start) and the new one takes its place. On a Mac, the new
`RealityHop.app` is copied from the disk image next to the old one and then swapped in. Copies of 1.x update to 2.0 the
same way they always updated.

The update source is the `REPO` constant in `rust/engine/src/lib.rs`. If you fork the project, point it at your fork.

## Layout

```
rust/app/                   the app: window, setup wizard, tray icon, entry point (--watch, --setup, --uninstall)
rust/app/ui/                the window's HTML, CSS and JavaScript
rust/engine/                nodes, tests, tunnel control, router, watcher, install, update, problem reports
rust/cli/                   the engine on the command line, for tests
rust/resources/             config templates, router scripts, translations, icon
rust/parity/                1.x outputs that 2.0 must match
rust/build-mac.sh           the Mac app and disk image
router-core/                the router build of sing-box
tools/check-backend.py      refuses a release while the build backend is out of date
tools/mac-app-test.py       the release's test of the built Mac app
docs/                       documentation
```
