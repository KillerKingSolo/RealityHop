"""install.py  -  install, update and uninstall of RealityHop.exe.

  installed to   C:\\Program Files\\RealityHop (or the folder picked in the setup): RealityHop.exe and everything it
                 keeps (configs, sing-box, node vault, logs), plus a Start menu shortcut and an Apps & features entry
  updates        the newest release of github.com/<REPO>, SHA-256 checked before it replaces the exe

Installing asks for one UAC prompt: it creates the folder, copies the exe and gives the folder to this Windows user
alone (with SYSTEM and administrators), so Reality Hop can update itself later without asking again and no other
account can read the keys in it. The exe itself never runs as administrator. Installs made by 1.0.x (data in
%LOCALAPPDATA%\\RealityHop, the program of 1.0.0 and 1.0.1 in %LOCALAPPDATA%\\Programs\\RealityHop) are moved there.
"""
import glob, hashlib, json, os, re, shutil, subprocess, sys, time, urllib.request
import engine as N

UNINSTALL_KEY = N.UNINSTALL_KEY
MENU_LNK = os.path.join(os.environ.get('APPDATA', ''), r'Microsoft\Windows\Start Menu\Programs', N.NAME + '.lnk')
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


def desktop_lnk():
    """The desktop icon's path (the Desktop folder may have moved, for example into OneDrive)."""
    import ctypes
    buf = ctypes.create_unicode_buffer(260); ctypes.windll.shell32.SHGetFolderPathW(None, 0x10, None, 0, buf)   # CSIDL_DESKTOPDIRECTORY
    return os.path.join(buf.value or os.path.expanduser(r'~\Desktop'), N.NAME + '.lnk')


def cleanup():
    """Old exes left by an update (a running exe can be renamed, not deleted), the old install folder once the
    Program Files copy runs, and shortcuts under the name used up to 1.0.4."""
    for p in (N.STARTUP_LNK, MENU_LNK, desktop_lnk()):
        N.lnk_migrate(p)
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
    """Stop every other Reality Hop: watchers, and windows running from another copy of the exe."""
    N.watcher_kill()
    N.run(['powershell', '-NoProfile', '-Command',
           "Get-CimInstance Win32_Process -Filter \"Name='RealityHop.exe'\" | Where-Object { $_.ExecutablePath -ne "
           f"{N.psq(sys.executable)} }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force }}"], timeout=30)


def lock_ps(sid):
    """PowerShell (needs admin, $d = the folder): only this Windows user, SYSTEM and administrators may open it. It holds
    the router SSH key, the node keys and the control API secret; Program Files rights would let every account read them."""
    return (f"icacls $d /grant:r '*{sid}:(OI)(CI)F' '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' /Q | Out-Null; "
            "icacls $d /inheritance:r /Q | Out-Null; icacls (Join-Path $d '*') /reset /T /C /Q | Out-Null")


def user_sid():
    r = N.run(['whoami', '/user', '/fo', 'csv', '/nh'], timeout=15)
    m = re.search(r'S-1-[\d-]+', r.stdout)
    return m.group(0) if m else None


def shortcuts():
    """The Start menu entry (it is also what Windows search finds when you press the Windows key and type Reality Hop)
    and the desktop icon, each as chosen in the setup or in Settings."""
    s, target = N.settings_load(), N.exe()
    for on, path in ((s['start_menu'], MENU_LNK), (s['desktop_icon'], desktop_lnk())):
        N.lnk_migrate(path)
        if on:
            N.run(['powershell', '-NoProfile', '-Command',
                   f"$s = (New-Object -ComObject WScript.Shell).CreateShortcut({N.psq(path)}); $s.TargetPath = {N.psq(target)}; "
                   f"$s.WorkingDirectory = {N.psq(os.path.dirname(target))}; $s.IconLocation = {N.psq(target + ',0')}; "
                   "$s.Description = 'Reality Hop: get your VPN through networks that block VPNs'; $s.Save()"], timeout=30)
        elif os.path.exists(path):
            os.remove(path)


def install(target=None):
    """Copy this exe into the install folder (C:\\Program Files\\RealityHop unless the setup picked another; one UAC
    prompt), give the folder to this Windows user alone, add the Start menu and Apps & features entries, move in the data
    that 1.0.x or the setup so far kept in %LOCALAPPDATA%, and point the logon and boot starts at the new place."""
    import winreg
    if not N.FROZEN:
        return 'running from source: nothing to install'
    sid = user_sid()
    if not sid:
        raise RuntimeError('could not read this Windows user; not installed')
    d = os.path.abspath(target or N.INSTALL_DIR)
    if os.path.basename(d).lower() != N.APP.lower():
        d = os.path.join(d, N.APP)   # a parent folder was picked: Reality Hop gets its own folder in it
    e = os.path.join(d, N.APP + '.exe')
    copy = not same(sys.executable, e)   # setup run again from the installed copy: nothing to copy
    others_kill()
    ps = (f"$d = {N.psq(d)}; $e = {N.psq(e)}; New-Item -ItemType Directory -Force -Path $d | Out-Null; "
          + ("if (Test-Path -LiteralPath $e) { Rename-Item -LiteralPath $e -NewName ('RealityHop.old-' + "
             "[DateTimeOffset]::Now.ToUnixTimeSeconds() + '.exe') }; "
             f"Copy-Item -LiteralPath {N.psq(sys.executable)} -Destination $e -Force; " if copy else '')
          + lock_ps(sid)   # also lets Reality Hop update itself there later without a UAC prompt
          + ('; ' + N.boot_task_ps(os.path.join(d, 'PC', 'rh-core.exe'), os.path.join(d, 'PC', 'pc-config.json'))
             if N.boot_task_exists() else ''))
    N.say(f'installing to {d}; answer the UAC prompt...')
    if N.elevated(ps) == 2 or not os.path.exists(e) or (copy and os.path.getsize(e) != os.path.getsize(sys.executable)):
        raise RuntimeError('not installed (the UAC prompt was declined or the copy failed)')
    N.INSTALL_DIR, N.INSTALLED_EXE = d, e
    shortcuts()
    with winreg.CreateKey(winreg.HKEY_CURRENT_USER, UNINSTALL_KEY) as k:
        for name, val in (('DisplayName', 'Reality Hop'), ('DisplayVersion', N.VERSION), ('Publisher', 'Reality Hop'),
                          ('DisplayIcon', e), ('InstallLocation', d),
                          ('UninstallString', f'"{e}" --uninstall'), ('URLInfoAbout', f'https://github.com/{N.REPO}')):
            winreg.SetValueEx(k, name, 0, winreg.REG_SZ, val)
        for name in ('NoModify', 'NoRepair'):
            winreg.SetValueEx(k, name, 0, winreg.REG_DWORD, 1)
    if not running_legacy():
        shutil.rmtree(N.LEGACY_DIR, ignore_errors=True)   # else the new copy removes it on its first start
    N.move_data(d)
    if os.path.exists(N.STARTUP_LNK):
        N.startup(True)   # the logon start now points at Program Files; starts the watcher from there
    N.say(f'installed Reality Hop {N.VERSION} to {d}')
    return f'installed to {d}'


def needs_move():
    """True for an install made by 1.0.x: the program is in its folder, the data still in %LOCALAPPDATA%."""
    return installed_here() and same(N.DATA, N.OLD_DATA) and not N.port_taken(N.GUI_PORT)


def move_here():
    """Move such an install's data into the program folder. One UAC prompt: the folder goes to this user alone and the
    boot task is pointed at the new rh-core. Then the tunnel and the watcher start again from there."""
    sid = user_sid()
    if not sid:
        raise RuntimeError('could not read this Windows user')
    d, watcher = N.INSTALL_DIR, os.path.exists(N.STARTUP_LNK)
    ps = f"$d = {N.psq(d)}; " + lock_ps(sid) + ('; ' + N.boot_task_ps(os.path.join(d, 'PC', 'rh-core.exe'), os.path.join(
        d, 'PC', 'pc-config.json')) if N.boot_task_exists() else '')
    if N.elevated(ps) == 2:
        raise RuntimeError('the admin prompt was declined')
    others_kill(); N.move_data(d)
    if not N.stopped():
        N.core_restart()
    if watcher:
        N.startup(True)   # the logon start's working folder moved too; starts the watcher


def start_watcher():
    if N.FROZEN:
        subprocess.Popen([N.exe(), '--watch'], cwd=N.DATA, env=N.own_env(), creationflags=subprocess.DETACHED_PROCESS)


def latest():
    """The newest release on GitHub: {'ver', 'asset', 'sha', 'notes'}, or None when it has no RealityHop.exe."""
    req = urllib.request.Request(f'https://api.github.com/repos/{N.REPO}/releases/latest', headers={'Accept': 'application/vnd.github+json'})
    r = json.loads(N.gh_get(req, 20))
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
    N.say(f"downloading Reality Hop {rel['ver']} ({a['size'] // 1_000_000} MB)...")
    with open(tmp, 'wb') as f:
        f.write(N.gh_get(a['browser_download_url'], 180))
    want = (a.get('digest') or '').removeprefix('sha256:') or rel.get('sha', '')
    got = hashlib.sha256(open(tmp, 'rb').read()).hexdigest()
    if not want or got.lower() != want.lower():
        os.remove(tmp)
        raise RuntimeError('the download does not match the checksum published with the release; not installed')
    dst = sys.executable
    try:
        place(tmp, dst)
    except PermissionError:   # a Program Files folder without the modify right: one UAC prompt instead
        N.elevated(f"$e = {N.psq(dst)}; Rename-Item -LiteralPath $e -NewName ('RealityHop.old-' + [DateTimeOffset]::Now.ToUnixTimeSeconds() + '.exe'); "
                   f"Copy-Item -LiteralPath {N.psq(tmp)} -Destination $e -Force")
        if not os.path.exists(dst) or os.path.getsize(dst) != os.path.getsize(tmp):
            raise RuntimeError('the update could not be copied (UAC declined); the current version keeps running')
    os.remove(tmp)
    try:
        import winreg
        with winreg.CreateKey(winreg.HKEY_CURRENT_USER, UNINSTALL_KEY) as k:
            winreg.SetValueEx(k, 'DisplayVersion', 0, winreg.REG_SZ, rel['ver'])
    except OSError:
        pass
    N.say(f"Reality Hop {rel['ver']} installed")


def uninstall(keep_data=False, router_too=False):
    """Remove Reality Hop from this PC: tunnel, watcher, boot start, shortcuts, Apps & features entry, the program
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
    for f in (MENU_LNK, N.STARTUP_LNK, desktop_lnk()):
        for p in (f, os.path.join(os.path.dirname(f), N.APP + '.lnk')):   # also the name used up to 1.0.4
            try:
                os.remove(p)
            except OSError:
                pass
    try:
        import winreg
        winreg.DeleteKey(winreg.HKEY_CURRENT_USER, UNINSTALL_KEY)
    except OSError:
        pass
    out.append('VPN app: turn its SOCKS5 proxy off (PIA: Settings > Multi-Hop, untick "Multi-hop and Obfuscation")')
    dirs = [N.LEGACY_DIR] + ([] if keep_data else [N.DATA])
    rm = [f'Remove-Item -LiteralPath {N.psq(d)} -Recurse -Force -ErrorAction SilentlyContinue' for d in dirs]
    if keep_data and same(N.DATA, N.INSTALL_DIR):   # the data stays in the program folder: only the program goes
        rm.append(f"Get-ChildItem -LiteralPath {N.psq(N.INSTALL_DIR)} -File -Filter 'Reality Hop*.exe' | Remove-Item -Force")
    else:
        rm.append(f'Remove-Item -LiteralPath {N.psq(N.INSTALL_DIR)} -Recurse -Force -ErrorAction SilentlyContinue')
    # this process may run from the install folder: a separate shell removes it once this one has exited
    subprocess.Popen(['powershell', '-NoProfile', '-Command', 'Start-Sleep 3; ' + '; '.join(rm)], creationflags=subprocess.CREATE_NO_WINDOW)
    out.append('program folder' + ('' if keep_data else ' and data folder') + ': removed')
    return out
