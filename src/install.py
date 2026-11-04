"""install.py  -  install, update and uninstall of RealityHop.exe.

  installed to   C:\\Program Files\\RealityHop (or the folder picked in the setup): RealityHop.exe with its program
                 folder app-X.Y.Z and everything it keeps (configs, sing-box, node vault, logs), plus a Start menu
                 shortcut and an Apps & features entry
  download       RealityHop.exe is one file that carries the program folder (a zip inside it) and installs it.
                 Installed, Reality Hop runs from the folder: nothing to unpack at each start (about 0.2 s, not 1 s+)
  updates        RealityHop.exe of the newest release of github.com/<REPO>, SHA-256 checked, then run with --unpack-to:
                 it puts the program folder it carries next to the running one and exits

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
EMBED = os.path.join(getattr(sys, '_MEIPASS', N.HERE), 'RealityHop-app.zip')   # the program folder (RealityHop.exe + app-X.Y.Z), inside the download


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
    """Old exes left by an update (a running exe can be renamed, not deleted), program folders of older versions once
    nothing runs from them, the old install folder once the Program Files copy runs, and shortcuts under the name used
    up to 1.0.4."""
    for p in (N.STARTUP_LNK, MENU_LNK, desktop_lnk()):
        N.lnk_migrate(p)
    for f in glob.glob(os.path.join(N.INSTALL_DIR, '*.old-*.exe')):
        try:
            os.remove(f)
        except OSError:
            pass
    dirs = [p for p in glob.glob(os.path.join(N.INSTALL_DIR, 'app-*')) if os.path.isdir(p)]
    plain = [p for p in dirs if re.fullmatch(r'app-\d+\.\d+\.\d+', os.path.basename(p))]
    keep = {os.path.normcase(getattr(sys, '_MEIPASS', ''))}   # the folder this copy runs from
    if plain:   # none yet on a one-file install that has not switched to the folder build
        keep.add(os.path.normcase(max(plain, key=lambda p: vtuple(os.path.basename(p)))))
    for p in dirs:
        if os.path.normcase(p) in keep or (p.endswith('.part') and time.time() - os.path.getmtime(p) < 600):   # an update unpacking now
            continue
        try:   # a copy that runs from it holds a file open (engine._PIN), so the rename fails: it stays until the next start
            gone = p if p.endswith('.del') else f'{p}.{int(time.time())}.del'
            if gone != p:
                os.replace(p, gone)
            shutil.rmtree(gone, ignore_errors=True)
        except OSError:
            pass
    if installed_here() and os.path.isdir(N.LEGACY_DIR):
        shutil.rmtree(N.LEGACY_DIR, ignore_errors=True)


def onedir():
    """True when this copy runs as the folder build (RealityHop.exe + app-X.Y.Z), not unpacked from one file."""
    return N.FROZEN and same(os.path.dirname(getattr(sys, '_MEIPASS', '')), os.path.dirname(sys.executable))


def unpack_app(src, d):
    """Put the program folder from zip src into folder d: app-X.Y.Z next to the one that runs now, then its
    RealityHop.exe (the running exe is renamed out of the way). cleanup() removes the old app-* folder later."""
    import zipfile
    exe_name = N.APP + '.exe'
    with zipfile.ZipFile(src) as z:
        names = [n for n in z.namelist() if not n.endswith('/')]
        tops = {n.split('/')[0] for n in names if '/' in n}
        app = tops.pop() if len(tops) == 1 else ''
        if not re.fullmatch(r'app-\d+\.\d+\.\d+', app) or exe_name not in names or \
                any(n != exe_name and not n.startswith(app + '/') for n in names):
            raise RuntimeError('this is not a Reality Hop program folder; not installed')
        part = os.path.join(d, app + '.part')
        shutil.rmtree(part, ignore_errors=True)
        for n in names:   # no file may land outside the folder
            if not os.path.normpath(os.path.join(part, n)).startswith(part + os.sep):
                raise RuntimeError(f'unsafe path in the program folder: {n}; not installed')
        z.extractall(part)
    dst = os.path.join(d, app)
    if os.path.isdir(dst):   # the same version again
        try:
            os.replace(dst, f'{dst}.{int(time.time())}.del')
        except OSError:
            dst = None       # it runs: it is this very version, keep it
    if dst:
        os.replace(os.path.join(part, app), dst)
    place(os.path.join(part, exe_name), os.path.join(d, exe_name))
    shutil.rmtree(part, ignore_errors=True)


def needs_unfold():
    """True for the one-file exe that the updater of 1.4.0 and older put in the install folder: it switches the
    install to the folder build it carries."""
    return installed_here() and not onedir() and os.path.isfile(EMBED) and \
        not os.path.isdir(os.path.join(N.INSTALL_DIR, f'app-{N.VERSION}'))


def unfold():
    """Swap the installed one-file exe for the folder build. The window and the watcher may both start at once after
    an update: only one does it. True when this process did it and should start again from the new exe."""
    lock = os.path.join(N.INSTALL_DIR, 'unfold.lock')
    try:
        if time.time() - os.path.getmtime(lock) > 300:
            os.remove(lock)   # left by a run that died
    except OSError:
        pass
    try:
        fd = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    except OSError:
        return False
    try:
        unpack_app(EMBED, N.INSTALL_DIR); N.say(f'Reality Hop {N.VERSION} now runs from its program folder: nothing to unpack at each start')
    finally:
        os.close(fd); os.remove(lock)
    return True


def place(src, dst):
    """Put src at dst; a running copy there is renamed out of the way first."""
    new = dst + '.new'
    shutil.copy2(src, new)   # first, so a full disk leaves the current copy in place
    if os.path.exists(dst):
        os.replace(dst, dst[:-4] + f'.old-{int(time.time())}.exe')
    os.replace(new, dst)


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
    copy, fresh = not same(sys.executable, e), not os.path.exists(e)   # setup run again from the installed copy: nothing to copy
    folder = copy and os.path.isfile(EMBED)   # the download carries the folder build: unpacked after the prompt, as this user
    others_kill()
    ps = (f"$d = {N.psq(d)}; $e = {N.psq(e)}; New-Item -ItemType Directory -Force -Path $d | Out-Null; "
          + ("if (Test-Path -LiteralPath $e) { Rename-Item -LiteralPath $e -NewName ('RealityHop.old-' + "
             "[DateTimeOffset]::Now.ToUnixTimeSeconds() + '.exe') }; "
             f"Copy-Item -LiteralPath {N.psq(sys.executable)} -Destination $e -Force; " if copy and not folder else '')
          + lock_ps(sid)   # also lets Reality Hop update itself there later without a UAC prompt
          + '; ' + N.legacy_ps()   # a test build's sb-core would hold port 2080
          + ('; ' + N.boot_task_ps(os.path.join(d, 'PC', 'rh-core.exe'), os.path.join(d, 'PC', 'pc-config.json'))
             if N.boot_task_exists() else ''))
    N.say(f'installing to {d}; answer the UAC prompt...')
    if N.elevated(ps) == 2 or not os.path.isdir(d):
        raise RuntimeError('not installed (the UAC prompt was declined)')
    if folder:
        unpack_app(EMBED, d)
    if not os.path.exists(e) or (copy and not folder and os.path.getsize(e) != os.path.getsize(sys.executable)):
        raise RuntimeError('not installed (the copy failed)')
    N.INSTALL_DIR, N.INSTALLED_EXE = d, e
    shortcuts(); register(d, e)
    if not running_legacy():
        shutil.rmtree(N.LEGACY_DIR, ignore_errors=True)   # else the new copy removes it on its first start
    N.move_data(d)
    if fresh:
        N.set_stopped(False)   # an uninstall that kept the data left this on, and the watcher would never start
    if N.startup_on():
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
    d, watcher = N.INSTALL_DIR, N.startup_on()
    ps = f"$d = {N.psq(d)}; " + lock_ps(sid) + ('; ' + N.boot_task_ps(os.path.join(d, 'PC', 'rh-core.exe'), os.path.join(
        d, 'PC', 'pc-config.json')) if N.boot_task_exists() else '')
    if N.elevated(ps) == 2:
        raise RuntimeError('the admin prompt was declined')
    others_kill(); N.move_data(d)
    if not N.stopped():
        N.core_restart()
    if watcher:
        N.startup(True)   # the logon start's working folder moved too; starts the watcher


def start_watcher(*extra):
    if N.FROZEN:
        subprocess.Popen([N.exe(), '--watch', *extra], cwd=N.DATA, env=N.own_env(), creationflags=subprocess.DETACHED_PROCESS)


def latest():
    """The newest release on GitHub: {'ver', 'asset', 'sha', 'notes'}, or None when it has no RealityHop.exe."""
    req = urllib.request.Request(f'https://api.github.com/repos/{N.REPO}/releases/latest', headers={'Accept': 'application/vnd.github+json'})
    r = json.loads(N.gh_get(req, 20))
    assets = {a['name']: a for a in r.get('assets', [])}
    if 'RealityHop.exe' not in assets:
        return None
    body = r.get('body') or ''
    m = re.search(r'SHA-256 of RealityHop\.exe[^0-9a-fA-F]*([0-9a-fA-F]{64})', body)   # also printed in the release notes
    return {'ver': r['tag_name'].lstrip('v'), 'asset': assets['RealityHop.exe'], 'sha': m.group(1) if m else '',
            'notes': body.split('\n---')[0][:1500]}


def check_update():
    """The newer release, or None. Only for the installed exe; running from source updates through git."""
    if not N.FROZEN:
        return None
    rel = latest()
    if not rel or vtuple(rel['ver']) <= vtuple(N.VERSION):
        return None
    try:
        rel['changes'] = changes(N.VERSION)
    except Exception:
        rel['changes'] = [(rel['ver'], '', rel['notes'])]
    return rel


def changes(since):
    """[(version, date, notes)] of every release newer than `since`, newest first, so a skipped version is not missed."""
    req = urllib.request.Request(f'https://api.github.com/repos/{N.REPO}/releases?per_page=30', headers={'Accept': 'application/vnd.github+json'})
    out = [(r['tag_name'].lstrip('v'), (r.get('published_at') or '')[:10], (r.get('body') or '').split('\n---')[0].strip())
           for r in json.loads(N.gh_get(req, 20)) if not r.get('draft') and not r.get('prerelease')]
    return sorted((c for c in out if vtuple(c[0]) > vtuple(since)), key=lambda c: vtuple(c[0]), reverse=True)


def apply_update(rel):
    """Download the release exe, check its SHA-256 against the release (GitHub's asset digest, or the checksum printed
    in the release notes), then put it in where this copy runs: a folder install runs it with --unpack-to, and it puts
    the program folder it carries there; a one-file copy swaps the exe. The caller restarts whatever it needs, with
    N.own_env() so a one-file exe unpacks its own files."""
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
    if onedir():
        d = os.path.dirname(dst)
        code = unpack_from(tmp, d)
        if code == 5:   # a folder without the modify right: one UAC prompt gives it to this user, as the setup does
            sid = user_sid()
            if sid and N.elevated(f"$d = {N.psq(d)}; " + lock_ps(sid)) != 2:
                code = unpack_from(tmp, d)
        os.remove(tmp)
        if code:
            raise RuntimeError(f'the update could not be unpacked (code {code}); the current version keeps running')
        _update_entry(rel['ver']); N.say(f"Reality Hop {rel['ver']} installed")
        return
    try:
        place(tmp, dst)
    except PermissionError:   # a Program Files folder without the modify right: one UAC prompt instead
        N.elevated(f"$e = {N.psq(dst)}; Rename-Item -LiteralPath $e -NewName ('RealityHop.old-' + [DateTimeOffset]::Now.ToUnixTimeSeconds() + '.exe'); "
                   f"Copy-Item -LiteralPath {N.psq(tmp)} -Destination $e -Force")
        if not os.path.exists(dst) or os.path.getsize(dst) != os.path.getsize(tmp):
            raise RuntimeError('the update could not be copied (UAC declined); the current version keeps running')
    os.remove(tmp); _update_entry(rel['ver']); N.say(f"Reality Hop {rel['ver']} installed")


def unpack_from(exe, d):
    """Run a downloaded (and checked) RealityHop.exe with --unpack-to d. Exit code: 0 done, 5 no write right, else failed."""
    try:
        return subprocess.run([exe, '--unpack-to', d], cwd=N.DATA, env=N.own_env(), timeout=300).returncode
    except (OSError, subprocess.TimeoutExpired):
        return 1


def register(d, e, ver=None):
    """The entry in Windows Settings > Apps and Control Panel > Uninstall a program, for the program in folder d."""
    import winreg
    with winreg.CreateKey(winreg.HKEY_CURRENT_USER, UNINSTALL_KEY) as k:
        for name, val in entry(d, e, ver).items():
            winreg.SetValueEx(k, name, 0, winreg.REG_SZ, val)
        for name in ('NoModify', 'NoRepair'):
            winreg.SetValueEx(k, name, 0, winreg.REG_DWORD, 1)


def entry(d, e, ver=None):
    return {'DisplayName': 'Reality Hop', 'DisplayVersion': ver or N.VERSION, 'Publisher': 'Reality Hop', 'DisplayIcon': e,
            'InstallLocation': d, 'UninstallString': f'"{e}" --uninstall', 'URLInfoAbout': f'https://github.com/{N.REPO}'}


def entry_fix():
    """Up to 1.4.0 an update changed only the version in the entry, so an entry made by 1.0.x kept pointing at a folder
    that is gone, and Uninstall a program failed. Rewrite it when it does not match this installed copy."""
    if not installed_here():
        return
    try:
        import winreg
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, UNINSTALL_KEY) as k:
            if all(winreg.QueryValueEx(k, n)[0] == v for n, v in entry(N.INSTALL_DIR, N.INSTALLED_EXE).items()):
                return
    except OSError:
        pass
    try:
        register(N.INSTALL_DIR, N.INSTALLED_EXE); N.say('Apps & features entry: repaired, it points at this copy again')
    except OSError:
        pass


def _update_entry(ver):
    try:
        register(os.path.dirname(sys.executable), sys.executable, ver)
    except OSError:
        pass


def uninstall(keep_data=False, router_too=False):
    """Remove Reality Hop from this PC: tunnel, watcher, boot start, shortcuts, Apps & features entry, the program
    folder and (unless keep_data) the data folder. With router_too the router tunnel is switched off first.
    Returns lines for the person."""
    out = []
    N.set_stopped(True)   # nothing may restart the tunnel while it is being removed
    if router_too and not N.pc_only():
        try:
            out.append(N.router_off())
            if not keep_data:
                out.append(N.router_key_remove())   # its private half is about to be deleted
        except Exception as e:
            out.append(f'router: could not switch it off ({e}); do it later in the GL.iNet panel')
    N.startup(False); others_stop(); N.core_stop(); out.append('PC tunnel, background watcher and open windows: stopped')
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
    traces_remove()
    out.append('VPN app: turn its SOCKS5 proxy off (PIA: Settings > Multi-Hop, untick "Multi-hop and Obfuscation")')
    dirs = [N.LEGACY_DIR] + ([] if keep_data else [N.DATA])
    rm = [f'Remove-Item -LiteralPath {N.psq(d)} -Recurse -Force -ErrorAction SilentlyContinue' for d in dirs]
    if keep_data and same(N.DATA, N.INSTALL_DIR):   # the data stays in the program folder: only the program goes
        rm.append(f"Get-ChildItem -LiteralPath {N.psq(N.INSTALL_DIR)} -File -Filter 'RealityHop*.exe' | Remove-Item -Force; "
                  f"Get-ChildItem -LiteralPath {N.psq(N.INSTALL_DIR)} -Directory -Filter 'app-*' | Remove-Item -Recurse -Force")
    else:
        rm.append(f'Remove-Item -LiteralPath {N.psq(N.INSTALL_DIR)} -Recurse -Force -ErrorAction SilentlyContinue')
    # this process runs from the install folder: a separate shell removes it once this one has exited (the last dialog
    # closed), and tries again for a few seconds while antivirus still holds a file
    script = (f'Wait-Process -Id {os.getpid()} -ErrorAction SilentlyContinue; Start-Sleep 1; foreach ($i in 1..10) {{ '
              + '; '.join(rm) + f'; if (-not (Test-Path -LiteralPath {N.psq(N.INSTALLED_EXE)})) {{ break }}; Start-Sleep 2 }}')
    subprocess.Popen([N.winexe('powershell'), '-NoProfile', '-Command', script], creationflags=subprocess.CREATE_NO_WINDOW)
    out.append('program folder' + ('' if keep_data else ' and data folder') + ': removed when you close this message')
    return out


def others_stop():
    """Stop every other Reality Hop process (a window left open, the watcher, a node test): a running copy keeps its
    program folder from being removed."""
    keep = ','.join(str(p) for p in {os.getpid(), os.getppid()})   # this copy (and its one-file starter)
    N.run(['powershell', '-NoProfile', '-Command', "Get-CimInstance Win32_Process | Where-Object { ($_.Name -in @('RealityHop.exe', "
           f"'rh-test.exe')) -and ($_.ProcessId -notin @({keep})) }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force "
           "-ErrorAction SilentlyContinue }"], timeout=30)


def traces_remove():
    """What Windows keeps about Reality Hop in this user's registry: its tray icon entry, the Start menu tile cache, the
    program name cache and the usage count. Windows' background activity record (HKLM) needs SYSTEM rights; it stays."""
    import codecs, winreg
    hk, tail = winreg.HKEY_CURRENT_USER, f'\\{N.APP}\\{N.APP}.exe'.lower()

    def keys(path):
        try:
            with winreg.OpenKey(hk, path) as k:
                return [winreg.EnumKey(k, i) for i in range(winreg.QueryInfoKey(k)[0])]
        except OSError:
            return []

    def values(path):
        try:
            with winreg.OpenKey(hk, path) as k:
                return [winreg.EnumValue(k, i) for i in range(winreg.QueryInfoKey(k)[1])]
        except OSError:
            return []

    def rmkey(path):
        for s in keys(path):
            rmkey(path + '\\' + s)
        try:
            winreg.DeleteKey(hk, path)
        except OSError:
            pass

    def rmvalues(path, match):
        for name, data, _ in values(path):
            if match(name):
                try:
                    with winreg.OpenKey(hk, path, 0, winreg.KEY_SET_VALUE) as k:
                        winreg.DeleteValue(k, name)
                except OSError:
                    pass
    base = r'Control Panel\NotifyIconSettings'
    for s in keys(base):
        if any(n == 'ExecutablePath' and str(d).lower().endswith(tail) for n, d, _ in values(f'{base}\\{s}')):
            rmkey(f'{base}\\{s}')
    base = r'Software\Microsoft\Windows\CurrentVersion\Start\TileProperties'
    for s in keys(base):
        for t in keys(f'{base}\\{s}'):
            if t.lower() == N.APP.lower():
                rmkey(f'{base}\\{s}\\{t}')
    rmvalues(r'Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache', lambda n: tail in n.lower())
    base = r'Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist'
    for g in keys(base):
        rmvalues(f'{base}\\{g}\\Count', lambda n: N.APP.lower() in codecs.decode(n, 'rot13').lower())
