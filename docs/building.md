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
pip install -r requirements.txt pyinstaller
powershell -ExecutionPolicy Bypass -File build.ps1
```

The result is `dist\RealityHop.exe`, one file with the templates and router scripts from `src/resources` inside.

## Release

Releases are built by GitHub Actions ([`.github/workflows/release.yml`](../.github/workflows/release.yml)):

1. Update [CHANGELOG.md](../CHANGELOG.md).
2. Tag and push:
   ```
   git tag v1.0.1
   git push origin v1.0.1
   ```
3. The workflow writes the version from the tag into `src/version.py` and builds `RealityHop.exe` on Windows. It then
   publishes a release with `RealityHop.exe`. The release notes are the CHANGELOG section for that version, plus the
   SHA-256 checksum of the exe.

Installed copies find the new release within a day (the watcher checks daily, and the window checks when it opens).
They check the download against the release's SHA-256 before they swap it in.

The update source is the `REPO` constant in `src/engine.py`. If you fork the project, point it at your fork.

## Layout

```
src/realityhop.pyw          window, setup wizard, entry point (--watch, --setup, --uninstall)
src/engine.py               nodes, tests, tunnel control, router, watcher, tray icon
src/install.py              install, update, uninstall
src/report.py               anonymous problem reports
src/version.py              version (set by the release workflow)
src/resources/              config templates, router scripts, icon, PIA screenshot
docs/                       documentation
```
