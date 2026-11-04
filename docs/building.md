# Building from source and releasing

## Run from source

```
pip install -r requirements.txt
python src/realityhop.pyw
```

From source, Reality Hop keeps its data in `%LOCALAPPDATA%\RealityHop` (the exe keeps it in its install folder), and it
does not install or update itself.

## Build the exe

```
pip install -r requirements.txt
powershell -ExecutionPolicy Bypass -File build.ps1
```

The result is `dist\RealityHop.exe`, the one file that is released. It carries the program as a folder:
`RealityHop.exe` plus `app-<version>` (Python, Tk and the templates and router scripts from `src/resources`). The setup
installs that folder, and Reality Hop runs from it. It starts in about 0.2 s because nothing is unpacked; a one-file
exe unpacks about 20 MB to `%TEMP%` at every start (1 s or more, 13 s on the first start while antivirus scans it).

## Release

Releases are built by GitHub Actions ([`.github/workflows/release.yml`](../.github/workflows/release.yml)):

1. Update [CHANGELOG.md](../CHANGELOG.md).
2. Check the build backend: `python tools/check-backend.py`. It lists the GitHub Actions, the Python version and any
   pinned packages, and fails while one is out of date. A release cannot go out until it passes: the workflow runs it
   first, and the pre-push hook runs it before a `v*` tag leaves your PC (turn the hook on once per clone with
   `git config core.hooksPath .githooks`).
3. Tag and push:
   ```
   git tag v1.2.1
   git push origin v1.2.1
   ```
4. The workflow writes the version from the tag into `src/version.py` and builds `RealityHop.exe` on Windows. It then
   publishes a release with `RealityHop.exe`. The release notes are the CHANGELOG section for that version, plus the
   SHA-256 checksum of the exe.

Installed copies find the new release within a day (the watcher checks daily, and the window checks when it opens).
They download `RealityHop.exe` and check it against the release's SHA-256, then run it with `--unpack-to`. It puts the
new `app-<version>` folder next to the running one, then the new `RealityHop.exe` replaces the old one. The old folder
is removed at a later start, once nothing runs from it. Copies up to 1.4.0 swap in the downloaded exe instead; at its
first start it installs the folder it carries and starts again from it.

The update source is the `REPO` constant in `src/engine.py`. If you fork the project, point it at your fork.

## Layout

```
src/realityhop.pyw          window, setup wizard, entry point (--watch, --setup, --uninstall)
src/engine.py               nodes, tests, tunnel control, router, watcher, tray icon
src/install.py              install, update, uninstall
src/report.py               redacted problem reports
tools/check-backend.py      refuses a release while the build backend is out of date
src/version.py              version (set by the release workflow)
src/resources/              config templates, router scripts, icon, PIA screenshot
docs/                       documentation
```
