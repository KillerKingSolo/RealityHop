"""install.py  -  install, update and uninstall of RealityHop.exe. No admin rights needed (only the optional
boot start asks for one UAC prompt).

  installed to   %LOCALAPPDATA%\\Programs\\RealityHop\\RealityHop.exe   (+ Start menu shortcut, Apps & features entry)
  data           %LOCALAPPDATA%\\RealityHop                             (configs, sing-box, node vault, logs)
  updates        the newest release of github.com/<REPO>, SHA-256 checked before it replaces the exe
"""
import glob, hashlib, json, os, re, shutil, subprocess, sys, time, urllib.request
import engine as N

UNINSTALL_KEY = r'Software\Microsoft\Windows\CurrentVersion\Uninstall\RealityHop'
MENU_LNK = os.path.join(os.environ.get('APPDATA', ''), r'Microsoft\Windows\Start Menu\Programs\RealityHop.lnk')


def installed_here():
    return N.FROZEN and os.path.normcase(os.path.abspath(sys.executable)) == os.path.normcase(N.INSTALLED_EXE)


def needs_install():
    """True when RealityHop.exe runs from somewhere else (the Downloads folder): this run is the installer."""
    return N.FROZEN and not installed_here()


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
    """Old exes left by an update (a running exe can be renamed, not deleted)."""
    for f in glob.glob(os.path.join(N.INSTALL_DIR, '*.old-*.exe')):
        try:
            os.remove(f)
        except OSError:
            pass


def place(src):
    """Put src at the install path; a running copy there is renamed out of the way first."""
    os.makedirs(N.INSTALL_DIR, exist_ok=True)
    if os.path.exists(N.INSTALLED_EXE):
        os.replace(N.INSTALLED_EXE, N.INSTALLED_EXE[:-4] + f'.old-{int(time.time())}.exe')
    shutil.copy2(src, N.INSTALLED_EXE)


def shortcut(lnk, target, args=''):
    ps = (f"$s = (New-Object -ComObject WScript.Shell).CreateShortcut('{lnk}'); $s.TargetPath = '{target}'; "
          f"$s.Arguments = '{args}'; $s.WorkingDirectory = '{os.path.dirname(target)}'; $s.IconLocation = '{target},0'; $s.Save()")
    N.run(['powershell', '-NoProfile', '-Command', ps], timeout=30)


def install():
    """Copy this exe to the install folder, add a Start menu shortcut and an Apps & features entry (current user)."""
    import winreg
    if not N.FROZEN:
        return 'running from source: nothing to install'
    if not installed_here():
        N.watcher_kill(); place(sys.executable)
    shortcut(MENU_LNK, N.INSTALLED_EXE)
    with winreg.CreateKey(winreg.HKEY_CURRENT_USER, UNINSTALL_KEY) as k:
        for name, val in (('DisplayName', 'RealityHop'), ('DisplayVersion', N.VERSION), ('Publisher', 'RealityHop'),
                          ('DisplayIcon', N.INSTALLED_EXE), ('InstallLocation', N.INSTALL_DIR),
                          ('UninstallString', f'"{N.INSTALLED_EXE}" --uninstall'), ('URLInfoAbout', f'https://github.com/{N.REPO}')):
            winreg.SetValueEx(k, name, 0, winreg.REG_SZ, val)
        for name in ('NoModify', 'NoRepair'):
            winreg.SetValueEx(k, name, 0, winreg.REG_DWORD, 1)
    N.say(f'installed RealityHop {N.VERSION} to {N.INSTALL_DIR}')
    return f'installed to {N.INSTALL_DIR}'


def start_watcher():
    if N.FROZEN:
        subprocess.Popen([N.INSTALLED_EXE, '--watch'], cwd=N.DATA, creationflags=subprocess.DETACHED_PROCESS)


def latest():
    """The newest release on GitHub: {'ver', 'asset', 'notes'}, or None when it has no RealityHop.exe."""
    req = urllib.request.Request(f'https://api.github.com/repos/{N.REPO}/releases/latest', headers={'Accept': 'application/vnd.github+json'})
    r = json.loads(urllib.request.urlopen(req, timeout=20).read())
    assets = {a['name']: a for a in r.get('assets', [])}
    if 'RealityHop.exe' not in assets:
        return None
    return {'ver': r['tag_name'].lstrip('v'), 'asset': assets['RealityHop.exe'], 'sum': assets.get('RealityHop.exe.sha256'),
            'notes': (r.get('body') or '')[:1500]}


def check_update():
    """The newer release, or None. Only for the installed exe; running from source updates through git."""
    if not N.FROZEN:
        return None
    rel = latest()
    return rel if rel and vtuple(rel['ver']) > vtuple(N.VERSION) else None


def apply_update(rel):
    """Download the release exe, check its SHA-256 against the release (GitHub's asset digest, or the .sha256 file
    published next to it), then swap it in. The caller restarts whatever it needs."""
    a, tmp = rel['asset'], os.path.join(N.DATA, 'update.exe')
    os.makedirs(N.DATA, exist_ok=True)
    N.say(f"downloading RealityHop {rel['ver']} ({a['size'] // 1_000_000} MB)...")
    with urllib.request.urlopen(a['browser_download_url'], timeout=180) as r, open(tmp, 'wb') as f:
        shutil.copyfileobj(r, f)
    want = (a.get('digest') or '').removeprefix('sha256:')
    if not want and rel.get('sum'):
        want = urllib.request.urlopen(rel['sum']['browser_download_url'], timeout=30).read().decode().split()[0]
    got = hashlib.sha256(open(tmp, 'rb').read()).hexdigest()
    if not want or got.lower() != want.lower():
        os.remove(tmp)
        raise RuntimeError('the download does not match the checksum published with the release; not installed')
    place(tmp); os.remove(tmp)
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
    dirs = [N.INSTALL_DIR] + ([] if keep_data else [N.DATA])
    # this process may run from the install folder: a separate shell removes the folders once it has exited
    rm = ' & '.join(f'rmdir /s /q "{d}"' for d in dirs)
    subprocess.Popen(['cmd', '/c', f'ping -n 4 127.0.0.1 >nul & {rm}'], creationflags=subprocess.CREATE_NO_WINDOW)
    out.append('program folder' + ('' if keep_data else ' and data folder') + ': removed')
    return out
