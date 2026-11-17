"""macos.py  -  the macOS side of Reality Hop: start at login (launchd), the process list, notifications, the menu bar
icon, the router key Terminal window, and updates from the release .dmg.

  app        RealityHop.app, dragged from the .dmg to /Applications; it runs from wherever it was put
  data       ~/Library/Application Support/RealityHop (only this user may open it): configs, sing-box, vault, logs
  at login   ~/Library/LaunchAgents/com.realityhop.watcher.plist (the watcher) and com.realityhop.tunnel.plist (rh-core)
  updates    the release .dmg for this Mac's chip, SHA-256 checked, mounted read-only; its RealityHop.app replaces this one

No step needs an admin password.
"""
import glob, hashlib, os, platform, plistlib, re, shlex, shutil, signal, subprocess, sys, tempfile, threading, time

ARCH = 'arm64' if platform.machine() == 'arm64' else 'amd64'   # the chip this build runs as (sing-box asset names)
DMG = 'RealityHop-mac-apple-silicon.dmg' if ARCH == 'arm64' else 'RealityHop-mac-intel.dmg'
HOME = os.path.expanduser('~')
SUPPORT = os.path.join(HOME, 'Library', 'Application Support', 'RealityHop')
AGENTS = os.path.join(HOME, 'Library', 'LaunchAgents')
TUNNEL, WATCHER = 'com.realityhop.tunnel', 'com.realityhop.watcher'   # launchd labels
DEFAULT_APP = '/Applications/RealityHop.app'
PIACTL = '/Applications/Private Internet Access.app/Contents/MacOS/piactl'
PIA_SETTINGS = '/Library/Preferences/com.privateinternetaccess.vpn/settings.json'


def bundle(exe=None):
    """The RealityHop.app folder that exe (Contents/MacOS/RealityHop) is in, or None when it is not in an app."""
    b = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(exe or sys.executable))))
    return b if b.endswith('.app') else None


def app_exe(app):
    return os.path.join(app, 'Contents', 'MacOS', 'RealityHop')


def misplaced():
    """True when the app runs from the disk image, or from the read-only copy macOS makes of a downloaded app that was
    not moved (App Translocation): a login item would point at a path that is gone after a restart."""
    b = bundle() or ''
    return b.startswith('/Volumes/') or '/AppTranslocation/' in b


def lockdown(d):
    """Only this user may open the data folder: it holds the router SSH key, the node keys and the control API secret."""
    try:
        os.makedirs(d, exist_ok=True); os.chmod(d, 0o700)
    except OSError:
        pass


def ps(fmt):
    """[(pid, field)] from ps for every process; field is the rest of the line (it may hold spaces)."""
    try:
        out = subprocess.run(['/bin/ps', '-axww', '-o', f'pid=,{fmt}='], capture_output=True, text=True, timeout=15).stdout
    except (OSError, subprocess.TimeoutExpired):
        return []
    rows = []
    for line in out.splitlines():
        pid, _, rest = line.strip().partition(' ')
        if pid.isdigit():
            rows.append((int(pid), rest.strip()))
    return rows


def procs():
    """[(process id, program name)] of every process; comm is the program's full path on macOS."""
    return [(p, os.path.basename(c)) for p, c in ps('comm')]


def cmdlines():
    """[(process id, full command line)] of every process."""
    return ps('command')


def kill(pids_):
    for p in pids_:
        try:
            os.kill(p, signal.SIGKILL)
        except OSError:
            pass   # gone already, or another user's


def port_owner(port):
    """Program name of the process listening on a local TCP port (this user's processes), or None."""
    try:
        out = subprocess.run(['/usr/sbin/lsof', '-nP', f'-iTCP:{port}', '-sTCP:LISTEN', '-Fc'], capture_output=True,
                             text=True, timeout=15).stdout
    except (OSError, subprocess.TimeoutExpired):
        return None
    return next((l[1:] for l in out.splitlines() if l.startswith('c')), None)


def notify(msg):
    """A Notification Center message. The text goes in as an argument, never as script code."""
    try:
        subprocess.Popen(['/usr/bin/osascript', '-e', 'on run argv', '-e',
                          'display notification (item 1 of argv) with title "Reality Hop"', '-e', 'end run', msg[:250]],
                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    except OSError:
        pass


def open_path(p):
    subprocess.Popen(['/usr/bin/open', p], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def mac_from_arp(out):
    """macOS arp drops leading zeros (0:1a:2b:3:4c:5d): back to the 00-1a-2b-03-4c-5d form Windows prints."""
    m = re.search(r'\bat ((?:[0-9a-f]{1,2}:){5}[0-9a-f]{1,2})\b', out.lower())
    return '-'.join(x.zfill(2) for x in m.group(1).split(':')) if m else ''


# ---- launchd login items: plain plist files in ~/Library/LaunchAgents, read by launchd at every login
def agent_path(label):
    return os.path.join(AGENTS, label + '.plist')


def agent_exists(label):
    return os.path.isfile(agent_path(label))


def agent_write(label, args, cwd):
    """A login item that starts args once at every login, in this user's session. launchd reads it at the next login;
    whatever must run now is started directly by the caller."""
    os.makedirs(AGENTS, exist_ok=True)
    d = {'Label': label, 'ProgramArguments': [str(a) for a in args], 'RunAtLoad': True, 'WorkingDirectory': cwd,
         'ProcessType': 'Interactive', 'LimitLoadToSessionType': 'Aqua'}
    tmp = agent_path(label) + '.tmp'
    with open(tmp, 'wb') as f:
        plistlib.dump(d, f)
    os.replace(tmp, agent_path(label))
    return True


def agent_args(label):
    try:
        with open(agent_path(label), 'rb') as f:
            return plistlib.load(f).get('ProgramArguments') or []
    except (OSError, ValueError):
        return []


def agent_remove(label):
    """Remove the login item. Not unloaded from launchd: that would also stop what it started at this login."""
    try:
        os.remove(agent_path(label))
    except OSError:
        pass


def _launchctl(*a):
    try:
        return subprocess.run(['/bin/launchctl', *a], capture_output=True, text=True, timeout=15)
    except (OSError, subprocess.TimeoutExpired):
        return None


def agent_enabled(label):
    """False after agent_enable(label, False) (Stop): launchd then skips it at login, until Start."""
    r = _launchctl('print-disabled', f'gui/{os.getuid()}')
    return not (r and re.search(rf'"{re.escape(label)}"\s*=>\s*(disabled|true)', r.stdout))


def agent_enable(label, on):
    _launchctl('enable' if on else 'disable', f'gui/{os.getuid()}/{label}')
    return agent_enabled(label) == on


# ---- the router key: a Terminal window where the person types the router password into ssh
def key_console(data, ssh, host, remote):
    f = os.path.join(data, 'add-router-key.command')
    with open(f, 'w', encoding='utf-8') as out:
        out.write('#!/bin/sh\ncd "$(dirname "$0")" || exit 1\nprintf "\\033]0;Reality Hop: add key to router\\007"\n'
                  'echo "Type the router admin password (the one for the GL.iNet web panel) and press Enter."\n'
                  'echo "Nothing shows while you type. Reality Hop never sees it."\necho\n'
                  f'{shlex.quote(ssh)} -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=known_hosts '
                  f'root@{host} {shlex.quote(remote)}\n'
                  'echo\nprintf "Press Enter to close. "\nread _\n')
    os.chmod(f, 0o700)
    subprocess.Popen(['/usr/bin/open', '-a', 'Terminal', f], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                     stderr=subprocess.DEVNULL)


# ---- updates and removal
def old_name(app):
    """Where the replaced app waits until nothing runs from it: hidden next to the new one (same disk, so a rename)."""
    return os.path.join(os.path.dirname(app), f'.RealityHop.old-{int(time.time())}.app')


def cleanup(app):
    """Apps left by an update, once 10 minutes old (the old copy has exited by then)."""
    if not app:
        return
    d = os.path.dirname(app)
    for p in glob.glob(os.path.join(d, '.RealityHop.old-*.app')) + glob.glob(os.path.join(d, 'RealityHop.app.new')):
        try:
            if time.time() - os.path.getmtime(p) > 600:
                shutil.rmtree(p, ignore_errors=True)
        except OSError:
            pass


def apply_update(rel, data, get, say, before_swap):
    """Download the release .dmg, check its SHA-256, mount it read-only, copy its RealityHop.app next to this one, then
    swap the two. before_swap() stops the other Reality Hop processes. The caller starts again from the same path."""
    a, app = rel['asset'], bundle()
    if not app:
        raise RuntimeError('not running from RealityHop.app; not updated')
    if not os.access(os.path.dirname(app), os.W_OK):
        raise RuntimeError(f'no write right to {os.path.dirname(app)}: download {DMG} from GitHub and drag Reality Hop '
                           'to Applications again')
    os.makedirs(data, exist_ok=True); tmp = os.path.join(data, 'update.dmg')
    say(f"downloading Reality Hop {rel['ver']} ({a['size'] // 1_000_000} MB)...")
    with open(tmp, 'wb') as f:
        f.write(get(a['browser_download_url'], 180))
    want = (a.get('digest') or '').removeprefix('sha256:') or rel.get('sha', '')
    got = hashlib.sha256(open(tmp, 'rb').read()).hexdigest()
    if not want or got.lower() != want.lower():
        os.remove(tmp)
        raise RuntimeError('the download does not match the checksum published with the release; not installed')
    mnt, new = tempfile.mkdtemp(prefix='rh-update-'), app + '.new'
    try:
        r = subprocess.run(['/usr/bin/hdiutil', 'attach', '-nobrowse', '-readonly', '-noautoopen', '-mountpoint', mnt, tmp],
                           capture_output=True, text=True, timeout=120)
        if r.returncode:
            raise RuntimeError(f'the update could not be opened: {(r.stderr or r.stdout).strip()[:200]}')
        try:
            src = os.path.join(mnt, 'RealityHop.app')
            if not os.path.isfile(app_exe(src)):
                raise RuntimeError('the update holds no RealityHop.app; not installed')
            shutil.rmtree(new, ignore_errors=True)
            r = subprocess.run(['/usr/bin/ditto', src, new], capture_output=True, text=True, timeout=300)
            if r.returncode or not os.path.isfile(app_exe(new)):
                shutil.rmtree(new, ignore_errors=True)
                raise RuntimeError(f'the update could not be copied: {(r.stderr or r.stdout).strip()[:200]}')
        finally:
            subprocess.run(['/usr/bin/hdiutil', 'detach', mnt, '-force'], capture_output=True, timeout=60)
    finally:
        try:
            os.rmdir(mnt)
        except OSError:
            pass
        os.remove(tmp)
    before_swap()
    old = old_name(app)
    os.replace(app, old)
    try:
        os.replace(new, app)
    except OSError:
        os.replace(old, app); raise   # put the running version back
    say(f"Reality Hop {rel['ver']} installed")


def remove_later(paths):
    """Delete these folders once this process has exited (the app runs from one of them). Only RealityHop.app and the
    Reality Hop data folder are accepted."""
    ok = [p for p in paths if p and (os.path.basename(p) == 'RealityHop.app' or os.path.abspath(p) == SUPPORT)]
    if ok:
        subprocess.Popen(['/bin/sh', '-c', 'while kill -0 "$0" 2>/dev/null; do sleep 1; done; sleep 1; rm -rf -- "$@"',
                          str(os.getpid()), *ok], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                         stderr=subprocess.DEVNULL, start_new_session=True)


# ---- the menu bar icon of the watcher (AppKit through pyobjc). AppKit needs the main thread: run() blocks there.
_ACTIONS, _Target = {}, [None]


def _target():
    if _Target[0] is None:
        from Foundation import NSObject

        class RealityHopMenuTarget(NSObject):
            def pick_(self, item):
                f = _ACTIONS.get(int(item.tag()))
                if f:
                    threading.Thread(target=f, daemon=True).start()
        _Target[0] = RealityHopMenuTarget
    return _Target[0]


class Tray:
    """Menu bar icon: the node ring, the status as the first (greyed) menu line and tooltip; menu: open, restart, check
    now, rescue, quit. No Dock icon for the watcher."""
    MENU = [(1, 'Open Reality Hop'), (4, 'Restart PC tunnel'), (2, 'Check now'), (3, 'Rescue from verified vault'), (0, None),
            (9, 'Quit background watcher')]

    def __init__(self, actions, icon_file, T):
        from AppKit import NSApplication, NSMenu, NSMenuItem, NSStatusBar, NSVariableStatusItemLength
        from PyObjCTools import AppHelper
        self.AppHelper, self.icon_file, self.cache = AppHelper, icon_file, {}
        _ACTIONS.update(actions)
        self.app = NSApplication.sharedApplication(); self.app.setActivationPolicy_(1)   # accessory: menu bar only
        self.item = NSStatusBar.systemStatusBar().statusItemWithLength_(NSVariableStatusItemLength)
        self.target = _target().alloc().init()
        menu = NSMenu.alloc().init(); menu.setAutoenablesItems_(False)
        self.status = NSMenuItem.alloc().initWithTitle_action_keyEquivalent_('Reality Hop', None, '')
        self.status.setEnabled_(False); menu.addItem_(self.status); menu.addItem_(NSMenuItem.separatorItem())
        for tag, text in self.MENU:
            if text is None:
                menu.addItem_(NSMenuItem.separatorItem()); continue
            mi = NSMenuItem.alloc().initWithTitle_action_keyEquivalent_(T(text), 'pick:', '')
            mi.setTarget_(self.target); mi.setTag_(tag); menu.addItem_(mi)
        self.item.setMenu_(menu)
        self._set('unk', 'Reality Hop', ())

    def _image(self, state, segs):
        from AppKit import NSImage
        path = self.icon_file(state, segs)
        if path not in self.cache:
            img = NSImage.alloc().initWithContentsOfFile_(path)
            if img is not None:
                img.setSize_((18, 18))
            self.cache = {path: img}   # one icon at a time: thousands of ring patterns are possible
        return self.cache[path]

    def _set(self, state, tip, segs):
        b = self.item.button()
        img = self._image(state, segs)
        if img is not None:
            b.setImage_(img)
        else:
            b.setTitle_('RH')
        b.setToolTip_(tip); self.status.setTitle_(tip)

    def update(self, state, tip, segs=()):
        self.AppHelper.callAfter(self._set, state, tip, tuple(segs))

    def remove(self):
        try:
            from AppKit import NSStatusBar
            self.AppHelper.callAfter(NSStatusBar.systemStatusBar().removeStatusItem_, self.item)
        except Exception:
            pass

    def run(self):
        self.AppHelper.runEventLoop(installInterrupt=True)
