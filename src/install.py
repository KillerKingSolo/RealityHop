"""install.py  -  install, update and uninstall of RealityHop.exe.

  installed to   C:\\Program Files\\RealityHop\\RealityHop.exe   (+ Start menu shortcut, Apps & features entry)
  data           %LOCALAPPDATA%\\RealityHop                    (configs, sing-box, node vault, logs)
  updates        the newest release of github.com/<REPO>, SHA-256 checked before it replaces the exe

Installing asks for one UAC prompt: it creates the Program Files folder, copies the exe and gives this Windows user
modify rights on that one folder, so RealityHop can update itself later without asking again. The exe itself never
runs as administrator. Copies installed by 1.0.0 and 1.0.1 (in %LOCALAPPDATA%\\Programs\\RealityHop) are moved there.
"""
import glob, hashlib, json, os, re, shutil, subprocess, sys, time, urllib.request
import engine as N

UNINSTALL_KEY = r'Software\Microsoft\Windows\CurrentVersion\Uninstall\RealityHop'
MENU_LNK = os.path.join(os.environ.get('APPDATA', ''), r'Microsoft\Windows\Start Menu\Programs\RealityHop.lnk')
LEGACY_EXE = os.path.join(N.LEGACY_DIR, 'RealityHop.exe')


def same(a, b):
    return os.path.normcase(os.path.abspath(a)) == os.path.normcase(os.path.abspath(b))


def installed_here():
    return N.FROZEN and same(sys.executable, N.INSTALLED_EXE)


def running_legacy():
    """True when this is a copy installed by 1.0.0 or 1.0.1 in %LOCALAPPDATA%\\Programs."""
    return N.FROZEN and same(sys.executable, LEGACY_EXE)


def needs_install():
    """True when RealityHop.exe runs from somewhere else (the Downloads folder): this run is the installer."""
    return N.FROZEN and not installed_here() and not running_legacy()


def installed_version():
    try:
        import winreg
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, UNINSTALL_KEY) as k:
            return winreg.QueryValueEx(k, 'DisplayVersion')[0]
    except OSError:
        return None


def vtuple(v):
    return tuple(int(x) for x in re.findall(r'\d+', v or '')[:3])


def cleanup():
    """Old exes left by an update (a running exe can be renamed, not deleted), and the old install folder once the
    Program Files copy runs."""
    for f in glob.glob(os.path.join(N.INSTALL_DIR, '*.old-*.exe')):
        try:
            os.remove(f)
        except OSError:
            pass
    if installed_here() and os.path.isdir(N.LEGACY_DIR):
        shutil.rmtree(N.LEGACY_DIR, ignore_errors=True)


def place(src, dst):
    """Put src at dst; a running copy there is renamed out of the way first."""
    if os.path.exists(dst):
        os.replace(dst, dst[:-4] + f'.old-{int(time.time())}.exe')
    shutil.copy2(src, dst)


def others_kill():
    """Stop every other RealityHop: watchers, and windows running from another copy of the exe."""
    N.watcher_kill()
    N.run(['powershell', '-NoProfile', '-Command',
           "Get-CimInstance Win32_Process -Filter \"Name='RealityHop.exe'\" | Where-Object { $_.ExecutablePath -ne "
           f"'{sys.executable}' }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force }}"], timeout=30)


def user_sid():
    r = N.run(['whoami', '/user', '/fo', 'csv', '/nh'], timeout=15)
    m = re.search(r'S-1-[\d-]+', r.stdout)
    return m.group(0) if m else None


def shortcut(lnk, target, args=''):
    ps = (f"$s = (New-Object -ComObject WScript.Shell).CreateShortcut('{lnk}'); $s.TargetPath = '{target}'; "
          f"$s.Arguments = '{args}'; $s.WorkingDirectory = '{os.path.dirname(target)}'; $s.IconLocation = '{target},0'; $s.Save()")
    N.run(['powershell', '-NoProfile', '-Command', ps], timeout=30)


def install():
    """Copy this exe to C:\\Program Files\\RealityHop (one UAC prompt), add a Start menu shortcut and an Apps & features
    entry, move a copy from the old per-user folder, and point the logon start at the new place."""
    import winreg
    if not N.FROZEN:
        return 'running from source: nothing to install'
    sid = user_sid()
    if not sid:
        raise RuntimeError('could not read this Windows user; not installed')
    others_kill()
    d, e = N.INSTALL_DIR, N.INSTALLED_EXE
    ps = (f"$d = '{d}'; $e = '{e}'; New-Item -ItemType Directory -Force -Path $d | Out-Null; "
          "if (Test-Path $e) { Rename-Item -LiteralPath $e -NewName ('RealityHop.old-' + [DateTimeOffset]::Now.ToUnixTimeSeconds() + '.exe') }; "
          f"Copy-Item -LiteralPath '{sys.executable}' -Destination $e -Force; "
          f"icacls $d /grant '*{sid}:(OI)(CI)M' /T /Q | Out-Null")   # updates later without a UAC prompt
    N.say('installing to Program Files; answer the UAC prompt...')
    N.elevated(ps)
    if not os.path.exists(e) or os.path.getsize(e) != os.path.getsize(sys.executable):
        raise RuntimeError('not installed (the UAC prompt was declined or the copy failed)')
    shortcut(MENU_LNK, e)
    with winreg.CreateKey(winreg.HKEY_CURRENT_USER, UNINSTALL_KEY) as k:
        for name, val in (('DisplayName', 'RealityHop'), ('DisplayVersion', N.VERSION), ('Publisher', 'RealityHop'),
                          ('DisplayIcon', e), ('InstallLocation', d),
                          ('UninstallString', f'"{e}" --uninstall'), ('URLInfoAbout', f'https://github.com/{N.REPO}')):
            winreg.SetValueEx(k, name, 0, winreg.REG_SZ, val)
        for name in ('NoModify', 'NoRepair'):
            winreg.SetValueEx(k, name, 0, winreg.REG_DWORD, 1)
    if not running_legacy():
        shutil.rmtree(N.LEGACY_DIR, ignore_errors=True)   # else the new copy removes it on its first start
    if os.path.exists(N.STARTUP_LNK):
        N.startup(True)   # the logon start now points at Program Files; starts the watcher from there
    N.say(f'installed RealityHop {N.VERSION} to {d}')
    return f'installed to {d}'


def start_watcher():
    if N.FROZEN:
        subprocess.Popen([N.exe(), '--watch'], cwd=N.DATA, env=N.own_env(), creationflags=subprocess.DETACHED_PROCESS)


def latest():
    """The newest release on GitHub: {'ver', 'asset', 'sha', 'notes'}, or None when it has no RealityHop.exe."""
    req = urllib.request.Request(f'https://api.github.com/repos/{N.REPO}/releases/latest', headers={'Accept': 'application/vnd.github+json'})
    r = json.loads(urllib.request.urlopen(req, timeout=20).read())
    assets = {a['name']: a for a in r.get('assets', [])}
    if 'RealityHop.exe' not in assets:
        return None
    body = r.get('body') or ''
    m = re.search(r'SHA-256[^0-9a-fA-F]*([0-9a-fA-F]{64})', body)   # also printed in the release notes
    return {'ver': r['tag_name'].lstrip('v'), 'asset': assets['RealityHop.exe'], 'sha': m.group(1) if m else '',
            'notes': body.split('\n---')[0][:1500]}


def check_update():
    """The newer release, or None. Only for the installed exe; running from source updates through git."""
    if not N.FROZEN:
        return None
    rel = latest()
    return rel if rel and vtuple(rel['ver']) > vtuple(N.VERSION) else None


def apply_update(rel):
    """Download the release exe, check its SHA-256 against the release (GitHub's asset digest, or the checksum printed
    in the release notes), then swap it in where this copy runs. The caller restarts whatever it needs, with
    N.own_env() so the new exe unpacks its own files instead of reusing this one's."""
    a, tmp = rel['asset'], os.path.join(N.DATA, 'update.exe')
    os.makedirs(N.DATA, exist_ok=True)
    N.say(f"downloading RealityHop {rel['ver']} ({a['size'] // 1_000_000} MB)...")
    with urllib.request.urlopen(a['browser_download_url'], timeout=180) as r, open(tmp, 'wb') as f:
        shutil.copyfileobj(r, f)
    want = (a.get('digest') or '').removeprefix('sha256:') or rel.get('sha', '')
    got = hashlib.sha256(open(tmp, 'rb').read()).hexdigest()
    if not want or got.lower() != want.lower():
        os.remove(tmp)
        raise RuntimeError('the download does not match the checksum published with the release; not installed')
    dst = sys.executable
    try:
        place(tmp, dst)
    except PermissionError:   # a Program Files folder without the modify right: one UAC prompt instead
        N.elevated(f"$e = '{dst}'; Rename-Item -LiteralPath $e -NewName ('RealityHop.old-' + [DateTimeOffset]::Now.ToUnixTimeSeconds() + '.exe'); "
                   f"Copy-Item -LiteralPath '{tmp}' -Destination $e -Force")
        if not os.path.exists(dst) or os.path.getsize(dst) != os.path.getsize(tmp):
            raise RuntimeError('the update could not be copied (UAC declined); the current version keeps running')
    os.remove(tmp)
    try:
        import winreg
        with winreg.CreateKey(winreg.HKEY_CURRENT_USER, UNINSTALL_KEY) as k:
            winreg.SetValueEx(k, 'DisplayVersion', 0, winreg.REG_SZ, rel['ver'])
    except OSError:
        pass
    N.say(f"RealityHop {rel['ver']} installed")


def uninstall(keep_data=False, router_too=False):
    """Remove RealityHop from this PC: tunnel, watcher, boot start, shortcuts, Apps & features entry, the program
    folder and (unless keep_data) the data folder. With router_too the router tunnel is switched off first.
    Returns lines for the person."""
    out = []
    N.set_stopped(True)   # nothing may restart the tunnel while it is being removed
    if router_too and not N.pc_only():
        try:
            out.append(N.router_off())
        except Exception as e:
            out.append(f'router: could not switch it off ({e}); do it later in the GL.iNet panel')
    N.startup(False); N.core_stop(); out.append('PC tunnel and background watcher: stopped')
    if N.boot_task_exists():
        out.append('boot start: removed' if N.boot_task(False) else 'boot start: NOT removed (UAC declined); remove the task '
                   f'"{N.BOOT_TASK}" in Task Scheduler')
    for f in (MENU_LNK, N.STARTUP_LNK):
        try:
            os.remove(f)
        except OSError:
            pass
    try:
        import winreg
        winreg.DeleteKey(winreg.HKEY_CURRENT_USER, UNINSTALL_KEY)
    except OSError:
        pass
    out.append('VPN app: turn its SOCKS5 proxy off (PIA: Settings > Multi-Hop, untick "Multi-hop and Obfuscation")')
    dirs = [N.INSTALL_DIR, N.LEGACY_DIR] + ([] if keep_data else [N.DATA])
    # this process may run from the install folder: a separate shell removes the folders once it has exited
    rm = ' & '.join(f'rmdir /s /q "{d}"' for d in dirs)
    subprocess.Popen(['cmd', '/c', f'ping -n 4 127.0.0.1 >nul & {rm}'], creationflags=subprocess.CREATE_NO_WINDOW)
    out.append('program folder' + ('' if keep_data else ' and data folder') + ': removed')
    return out
