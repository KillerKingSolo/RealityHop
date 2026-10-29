#!/usr/bin/env python3
"""engine.py  -  Reality Hop engine and command line.

Reality Hop puts your VPN inside a VLESS + REALITY tunnel (sing-box), so a network that blocks VPNs sees ordinary
HTTPS to a big website. It finds free public nodes, tests them, keeps the fastest working ones in use and replaces
dead ones on its own.

  realityhop.pyw / RealityHop.exe   the window (the first run is the setup wizard)
  python engine.py --check          live health check of the nodes in use
  python engine.py                  scan the node list, test the fastest candidates, print a ranked table, offer to apply
  python engine.py --apply 6        put the 6 best into the configs and restart the tunnel(s)
  python engine.py --watch          background watcher with the tray icon (started at logon)

PC tunnel: rh-core.exe (official sing-box) runs pc-config.json and listens on 127.0.0.1:2080 (SOCKS5); the VPN app
connects through it. Router tunnel (optional, GL.iNet): sing-box on the router runs router-config.json and diverts the
chosen LAN devices into it; resources/router/heal.sh repairs it while this PC is off. Everything this PC keeps is in
its install folder (C:\\Program Files\\RealityHop by default), %LOCALAPPDATA%\\RealityHop when run from source.

Nodes in use are tested through the live tunnel over sing-box's Clash API (127.0.0.1:9090, bearer secret in each
config). Candidates get a real REALITY handshake plus an HTTPS fetch through a throwaway sing-box on this PC.
"""
import argparse, datetime, glob, hashlib, ipaddress, json, os, re, secrets, shutil, socket, subprocess, sys, threading, time, urllib.error, urllib.request
from concurrent.futures import ThreadPoolExecutor
from urllib.parse import urlparse, parse_qs
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import version
VERSION = version.VERSION

APP = 'RealityHop'      # file, folder and repository name
NAME = 'Reality Hop'    # the name people see
REPO = 'KillerKingSolo/RealityHop'   # GitHub owner/repo: app updates come from its Releases
FROZEN = getattr(sys, 'frozen', False)   # running as RealityHop.exe (PyInstaller)
HERE = os.path.dirname(os.path.abspath(__file__))
RES = os.path.join(getattr(sys, '_MEIPASS', HERE), 'resources')   # config templates and router scripts (inside the exe)
_LOCAL = os.environ.get('LOCALAPPDATA') or os.path.expanduser('~')
OLD_DATA = os.path.join(_LOCAL, APP)        # the data folder of 1.0.x (and when run from source); moved on request
LEGACY_DIR = os.path.join(_LOCAL, 'Programs', APP)               # where 1.0.0 and 1.0.1 installed; moved on update
DEFAULT_DIR = os.path.join(os.environ.get('ProgramW6432') or os.environ.get('ProgramFiles') or r'C:\Program Files', APP)
UNINSTALL_KEY = r'Software\Microsoft\Windows\CurrentVersion\Uninstall\RealityHop'   # the Apps & features entry


def install_dir():
    """The folder Reality Hop is installed in: chosen in the setup, C:\\Program Files\\RealityHop by default."""
    try:
        import winreg
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, UNINSTALL_KEY) as k:
            loc = winreg.QueryValueEx(k, 'InstallLocation')[0]
        return loc if os.path.isfile(os.path.join(loc, APP + '.exe')) else DEFAULT_DIR
    except OSError:
        return DEFAULT_DIR


def data_dir():
    """Everything Reality Hop keeps lives in its install folder, next to RealityHop.exe. Run from source, before the
    setup has installed it, or installed by 1.0.x and not moved yet: %LOCALAPPDATA%\\RealityHop."""
    moved = os.path.exists(os.path.join(INSTALL_DIR, 'settings.json')) or not os.path.exists(os.path.join(OLD_DATA, 'settings.json'))
    return INSTALL_DIR if FROZEN and os.path.isfile(INSTALLED_EXE) and moved else OLD_DATA


def set_paths(data):
    """Point every data path at one folder (again after the setup has installed Reality Hop, or after a move)."""
    global DATA, PC_DIR, ROUTER_DIR, PC_CFG, ROUTER_CFG, REPORTS, NODES_TXT, VAULT, LOGS, CORE, ROUTER_KEY, CORE_LOG
    global THIS_PC, SETTINGS, HISTORY, LAST, TRAY_LAST
    DATA = data                                                    # everything this PC keeps
    PC_DIR, ROUTER_DIR = os.path.join(DATA, 'PC'), os.path.join(DATA, 'Router')   # the backend files, readable by the person
    PC_CFG, ROUTER_CFG = os.path.join(PC_DIR, 'pc-config.json'), os.path.join(ROUTER_DIR, 'router-config.json')
    REPORTS = os.path.join(DATA, 'anonymous-logs')   # problem reports with everything personal taken out
    NODES_TXT = os.path.join(DATA, 'nodes.txt')
    VAULT = os.path.join(DATA, 'vault.json')   # every node that ever passed a test; the rescue pool
    LOGS = os.path.join(DATA, 'logs'); CORE_LOG = os.path.join(LOGS, 'core.log')
    CORE = os.path.join(PC_DIR, 'rh-core.exe')              # the official sing-box, under its own name: runs the PC tunnel
    ROUTER_KEY = os.path.join(DATA, 'router_ed25519')       # SSH key made by the setup wizard
    THIS_PC, SETTINGS, HISTORY, LAST, TRAY_LAST = (os.path.join(DATA, f) for f in
                                                   ('this-pc.json', 'settings.json', 'history.json', 'last.json', 'tray.json'))


INSTALL_DIR = install_dir(); INSTALLED_EXE = os.path.join(INSTALL_DIR, APP + '.exe')
set_paths(data_dir())
HEAL_SH = os.path.join(RES, 'router', 'heal.sh')       # router self-repair, installed with every push
NFT_SH = os.path.join(RES, 'router', 'nft-devices.sh')  # router firewall rule that diverts the chosen devices
WATCH_PORT, GUI_PORT = 47291, 47292                    # loopback ports used only as "is it running" locks
STARTUP_LNK = os.path.join(os.environ.get('APPDATA', ''), r'Microsoft\Windows\Start Menu\Programs\Startup', NAME + '.lnk')
API = 'http://127.0.0.1:9090'
TEST_URL = 'https://www.gstatic.com/generate_204'
FEEDS = ('https://raw.githubusercontent.com/rtwo2/FastNodes/main/sub/verified_tls.txt',)   # github.com/rtwo2/FastNodes
# the same file on CDN mirrors, for when GitHub is blocked; {0} = owner/repo, {1} = path
MIRRORS = ('https://cdn.jsdelivr.net/gh/{0}@main/{1}', 'https://fastly.jsdelivr.net/gh/{0}@main/{1}')
TUNNEL_PROXY = 'http://127.0.0.1:2080'   # the PC tunnel's mixed inbound also speaks HTTP proxy
DEFAULTS = {'check_min': 60, 'slow_ms': 200, 'tolerance_ms': 25, 'top': 40, 'router': 0, 'router_host': '192.168.8.1',
            'router_key': '', 'devices': '', 'auto_update': 1, 'singbox': 'stable', 'backend_files': 0,
            'start_menu': 1, 'desktop_icon': 0, 'pc_tunnel': 1}
LIMITS = {'check_min': (15, 1440), 'slow_ms': (50, 5000), 'tolerance_ms': (0, 1000), 'top': (10, 120)}   # as in Settings
FPS = {'chrome', 'firefox', 'safari', 'ios', 'android', 'edge', '360', 'qq', 'random', 'randomized'}   # uTLS fingerprints
TOP = 40   # candidates tested per scan (settings)
FRONTS = {'www.apple.com', 'swdist.apple.com', 'www.icloud.com', 'www.cloudflare.com', 'cloudflare.com', 'www.amazon.com',
          'amazon.com', 'yahoo.com', 'www.yahoo.com', 'cdnjs.com', 'www.microsoft.com', 'www.bing.com', 'www.python.org',
          'www.nvidia.com', 'www.amd.com'}
NOWIN = {'creationflags': subprocess.CREATE_NO_WINDOW} if os.name == 'nt' else {}
say = print   # the GUI points this at its log pane


def settings_load():
    try:
        raw = json.load(open(SETTINGS))
    except (OSError, ValueError):
        raw = {}
    d = {}
    for k, v in DEFAULTS.items():
        try:
            d[k] = type(v)(raw.get(k, v))
        except (TypeError, ValueError):
            d[k] = v
    for k, (lo, hi) in LIMITS.items():   # a hand-edited 0 would mean a node check every 30 s
        d[k] = min(max(d[k], lo), hi)
    global TOP, FULL_EVERY, SLOW
    TOP, FULL_EVERY, SLOW = int(d['top']), int(d['check_min']) * 60, int(d['slow_ms'])
    return d


def settings_save(d):
    """Save some or all settings; the switching margin also goes into both configs (next restart/push)."""
    cur = settings_load()
    cur.update({k: type(DEFAULTS[k])(v) for k, v in d.items() if k in DEFAULTS})
    jsave(SETTINGS, cur, indent=1); d = settings_load()   # read back: numbers out of range come back clamped
    for f in (PC_CFG, ROUTER_CFG):
        if not os.path.exists(f):
            continue
        c = json.load(open(f)); changed = False
        for o in c['outbounds']:
            if o.get('type') == 'urltest' and o.get('tolerance') != d['tolerance_ms']:
                o['tolerance'] = d['tolerance_ms']; changed = True
        if changed:
            jsave(f, c, indent=2)
    return d


def jsave(path, data, indent=None):
    """Write JSON atomically (the window and the watcher both write these files)."""
    os.makedirs(os.path.dirname(path), exist_ok=True)
    tmp = f'{path}.{os.getpid()}-{threading.get_ident()}.tmp'
    with open(tmp, 'w') as f:
        json.dump(data, f, indent=indent)
    os.replace(tmp, path)


def hist_load():
    try:
        h = json.load(open(HISTORY))
    except (OSError, ValueError):
        h = {}
    return {'pings': h.get('pings', {}), 'events': h.get('events', [])}


def hist_add(pings=None, event=None):
    """History for the window's graphs: per-node tunnel ping, plus events (drops, replacements, restarts), 48 h kept."""
    try:
        h, now = hist_load(), time.time()
        for server, ms in (pings or {}).items():
            h['pings'].setdefault(server, []).append([int(now), ms])
        if event:
            h['events'].append([int(now)] + list(event))
        cut_ = now - 48 * 3600
        h['pings'] = {k: [x for x in v if x[0] > cut_] for k, v in h['pings'].items() if any(x[0] > cut_ for x in v)}
        h['events'] = [e for e in h['events'] if e[0] > cut_][-2000:]
        jsave(HISTORY, h)
    except Exception:
        pass


def pythonw():
    """The newest installed pythonw.exe (the watcher runs on it), or this exe when frozen."""
    if FROZEN:
        return sys.executable
    cands = glob.glob(r'C:\Program Files\Python3*\pythonw.exe') + glob.glob(r'C:\Python3*\pythonw.exe') + \
        glob.glob(os.path.join(os.environ.get('LOCALAPPDATA', ''), r'Programs\Python\Python3*\pythonw.exe'))
    ver = lambda p: int(re.search(r'Python3(\d+)', p).group(1)) if re.search(r'Python3(\d+)', p) else 0
    return max(cands, key=ver) if cands else os.path.join(os.path.dirname(sys.executable), 'pythonw.exe')


def exe():
    """The installed RealityHop.exe (also when this run is the installer in the Downloads folder)."""
    return INSTALLED_EXE if os.path.exists(INSTALLED_EXE) else sys.executable


def own_env():
    """Environment for starting RealityHop.exe from RealityHop.exe. Without this the new process inherits the
    one-file unpack folder (_MEI...) of the old one, keeps using it, and breaks when the old one exits and deletes it
    (lost icon, failed checks, "Failed to remove temporary directory")."""
    e = {k: v for k, v in os.environ.items() if not k.startswith(('_PYI', '_MEIPASS')) and k not in ('TCL_LIBRARY', 'TK_LIBRARY')}
    e['PYINSTALLER_RESET_ENVIRONMENT'] = '1'
    return e


def self_cmd(*args):
    """Command line that runs this program again (python script or frozen exe)."""
    return [exe(), *args] if FROZEN else [pythonw(), os.path.abspath(__file__), *args]


CANCEL, _procs = threading.Event(), set()


def run(cmd, stdin=None, timeout=240, **kw):
    """stdin goes as raw bytes: text mode would turn every newline into CR LF, which the router's shell chokes on.
    Every call has a time limit, and cancel() kills whatever is running, so no action can hang the window."""
    if CANCEL.is_set():
        raise RuntimeError('cancelled')
    p = subprocess.Popen(cmd, stdin=subprocess.PIPE if stdin is not None else subprocess.DEVNULL,
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE, **NOWIN, **kw)
    _procs.add(p)
    try:
        out, err = p.communicate(stdin.encode() if stdin is not None else None, timeout=timeout)
    except subprocess.TimeoutExpired:
        p.kill(); p.communicate()
        raise RuntimeError(f'{os.path.basename(cmd[0])} took longer than {timeout} s and was stopped')
    finally:
        _procs.discard(p)
    if CANCEL.is_set():
        raise RuntimeError('cancelled')
    return subprocess.CompletedProcess(cmd, p.returncode, out.decode('utf-8', 'replace'), err.decode('utf-8', 'replace'))


def psq(s):
    """s as a PowerShell single-quoted string. Without this a ' in a path (a user called O'Brien, a renamed download)
    ends the string early: the command fails, or worse, the rest of the name runs as a command."""
    return "'" + str(s).replace("'", "''") + "'"


def cancel():
    CANCEL.set()
    for p in list(_procs):
        p.kill()


def file_log(name):
    """Activity log in logs/<name>.log; at 1 MB it rolls over to <name>.log.1, so at most 2 files per log."""
    import logging.handlers
    os.makedirs(LOGS, exist_ok=True)
    lg = logging.getLogger(name); lg.setLevel(logging.INFO)
    if not lg.handlers:
        h = logging.handlers.RotatingFileHandler(os.path.join(LOGS, f'{name}.log'), maxBytes=1_000_000, backupCount=1, encoding='utf-8')
        h.setFormatter(logging.Formatter('%(asctime)s  %(message)s', '%Y-%m-%d %H:%M:%S')); lg.addHandler(h)
    return lambda *a: lg.info(' '.join(str(x) for x in a))


def logs_close():
    """Let go of the open log files (the data folder is about to move); file_log opens them again."""
    import logging
    for name in ('window', 'watch'):
        lg = logging.getLogger(name)
        for h in lg.handlers[:]:
            lg.removeHandler(h); h.close()


def move_data(dst):
    """Move what 1.0.x (or the setup, before it had installed Reality Hop) kept in %LOCALAPPDATA%\\RealityHop into dst,
    the install folder, and use dst from now on. rh-core runs from there, so it is stopped; the caller starts it again."""
    import logging
    if os.path.isdir(OLD_DATA) and os.path.normcase(os.path.abspath(OLD_DATA)) != os.path.normcase(os.path.abspath(dst)):
        if core_running():
            core_stop()
        open_logs = [n for n in ('window', 'watch') if logging.getLogger(n).handlers]; logs_close()
        os.makedirs(dst, exist_ok=True)
        for name in os.listdir(OLD_DATA):
            s, d = os.path.join(OLD_DATA, name), os.path.join(dst, name)
            if os.path.isdir(s) and os.path.isdir(d):
                shutil.copytree(s, d, dirs_exist_ok=True); shutil.rmtree(s, ignore_errors=True)
            else:
                if os.path.isfile(d):
                    os.remove(d)
                shutil.move(s, d)
        shutil.rmtree(OLD_DATA, ignore_errors=True)
        set_paths(dst)
        for n in open_logs:
            file_log(n)
        try:   # rh-core writes its log by full path
            c = json.load(open(PC_CFG)); c['log']['output'] = CORE_LOG; jsave(PC_CFG, c, indent=2)
        except (OSError, ValueError, KeyError):
            pass
        say(f'data moved to {dst}')
    else:
        set_paths(dst)


def notify(msg):
    """Windows toast, so the background watcher can tell you what it did."""
    ps = ("[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null;"
          "$t = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02);"
          "$x = $t.GetElementsByTagName('text'); $x.Item(0).AppendChild($t.CreateTextNode('Reality Hop')) > $null;"
          "$x.Item(1).AppendChild($t.CreateTextNode($env:NM_MSG)) > $null;"
          "[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier("
          "'{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe').Show([Windows.UI.Notifications.ToastNotification]::new($t))")
    try:
        subprocess.Popen(['powershell', '-NoProfile', '-Command', ps], env=dict(os.environ, NM_MSG=msg[:250]),
                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, **NOWIN)
    except OSError:
        pass


def ssh_exe(name='ssh'):
    """Windows' own OpenSSH client (built into Windows 10 and 11)."""
    return shutil.which(name) or os.path.join(os.environ.get('SystemRoot', r'C:\Windows'), 'System32', 'OpenSSH', name + '.exe')


def ssh_base():
    s = settings_load()
    return [ssh_exe(), '-i', s['router_key'] or ROUTER_KEY, '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=10',
            '-o', 'StrictHostKeyChecking=accept-new', '-o', 'UserKnownHostsFile=' + os.path.join(DATA, 'known_hosts'),
            'root@' + s['router_host']]


def ssh(cmd, stdin=None, timeout=240):
    r = run(ssh_base() + [cmd], stdin, timeout=timeout)
    if r.returncode == 255:
        raise RuntimeError('router unreachable over ssh')
    return r.stdout


_feed = [0, None]


def public_ip(h):
    """A plain IPv4 internet address. A node list must never point the tunnel or the pings at this PC or the LAN."""
    try:
        return re.fullmatch(r'\d+\.\d+\.\d+\.\d+', h) is not None and ipaddress.IPv4Address(h).is_global
    except ValueError:
        return False


LAN_NETS = tuple(ipaddress.ip_network(n) for n in ('10.0.0.0/8', '172.16.0.0/12', '192.168.0.0/16'))


def lan_ip(x):
    """A valid address of a device on the local network (what the router tunnel goes by)."""
    try:
        return any(ipaddress.IPv4Address(x) in n for n in LAN_NETS)
    except ValueError:
        return False


def gh_get(url, timeout=30):
    """A whole download from GitHub (url or Request): direct first, then through the PC tunnel, which gets past a block.
    The retry also covers a download that stalls partway, as DPI filters do. TLS stays end to end with GitHub."""
    try:
        with urllib.request.urlopen(url, timeout=timeout) as r:
            return r.read()
    except Exception as e:
        if router_only() or not port_open(2080):
            raise
        say(f'direct download failed ({e}); retrying through the tunnel')
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({'http': TUNNEL_PROXY, 'https': TUNNEL_PROXY}))
    with opener.open(url, timeout=timeout * 3) as r:
        return r.read()


def mirrors(u):
    p = urlparse(u).path.split('/')   # /owner/repo/branch/path...
    return [m.format('/'.join(p[1:3]), '/'.join(p[4:])) for m in MIRRORS] if urlparse(u).hostname == 'raw.githubusercontent.com' else []


def feed_download():
    """The node lists from GitHub (direct, then through the tunnel), the CDN mirrors, the router, then the last good copy.
    Only the usable lines are kept (about a quarter of the 18 MB)."""
    cache = os.path.join(DATA, 'feed-cache.txt')
    def get(u):
        for src in [u] + mirrors(u):
            try:
                raw = '\n'.join(l for l in gh_get(src, 20).decode('utf-8', 'ignore').splitlines()
                                if l.startswith('vless://') and 'reality' in l and 'vision' in l)
                if raw:
                    return raw
            except Exception as e:
                say(f'node list source failed ({urlparse(src).hostname}): {e}')
        return ''
    with ThreadPoolExecutor(len(FEEDS)) as ex:
        raw = '\n'.join(ex.map(get, FEEDS))
    if 'vless://' not in raw and not pc_only():
        say('this PC could not download the node lists; asking the router to download them...')
        try:
            raw = router_feed()
        except Exception as e:
            say(f'router download failed: {e}')
    if 'vless://' in raw:
        try:
            with open(cache, 'w', encoding='utf-8') as f:
                f.write(raw)
        except OSError:
            pass
        return raw
    try:
        age = (time.time() - os.path.getmtime(cache)) / 3600
        with open(cache, encoding='utf-8') as f:
            raw = f.read()
    except OSError:
        raise RuntimeError('no node list source works and there is no saved copy')
    say(f'every node list source failed; using the saved copy from {age:.0f} h ago')
    return raw


def feed():
    """Candidates from the public node lists (FEEDS), merged, one per server; the download is reused for 15 minutes."""
    if _feed[1] is None or time.time() - _feed[0] > 900:
        _feed[:] = [time.time(), feed_download()]
    seen = {}
    for l in _feed[1].splitlines():
        if not l.startswith('vless://') or 'reality' not in l or 'vision' not in l:
            continue
        try:   # one broken line (a port like 99999) must not stop the whole scan
            u = urlparse(l.strip()); q = {k: v[0] for k, v in parse_qs(u.query).items()}; h, port = u.hostname or '', u.port
        except ValueError:
            continue
        if (q.get('security') != 'reality' or port != 443 or q.get('flow') != 'xtls-rprx-vision' or not q.get('pbk')
                or q.get('sni') not in FRONTS or not public_ip(h) or h in seen
                or not re.fullmatch(r'[A-Za-z0-9_-]{43}', q['pbk']) or not re.fullmatch(r'([0-9a-fA-F]{2}){0,8}', q.get('sid', ''))
                or not re.fullmatch(r'[0-9a-fA-F-]{36}', u.username or '')):
            continue
        seen[h] = dict(server=h, uuid=u.username, pbk=q['pbk'], sid=q.get('sid', ''), sni=q['sni'],
                       fp=q.get('fp') if q.get('fp') in FPS else 'chrome')
    return list(seen.values())


def ping443(host, timeout):
    s = socket.socket(); s.settimeout(timeout); t = time.perf_counter()
    try:
        s.connect((host, 443)); return int((time.perf_counter() - t) * 1000)
    except OSError:
        return None
    finally:
        s.close()


def tcp_probe(nodes):
    """Sets tcp_ms on every node (None if 443 never answered); returns the responders, fastest first.
    One 2 s ping for all, then the best of two more for the ones that answered."""
    def one(n):
        n['tcp_ms'] = ping443(n['server'], 2)
    def more(n):
        n['tcp_ms'] = min([n['tcp_ms']] + [m for m in (ping443(n['server'], 1.5) for _ in range(2)) if m is not None])
    with ThreadPoolExecutor(128) as ex:
        list(ex.map(one, nodes))
        list(ex.map(more, [n for n in nodes if n['tcp_ms'] is not None]))
    return sorted([n for n in nodes if n['tcp_ms'] is not None], key=lambda n: n['tcp_ms'])


def outbound19(n, tag):  # works on the router's sing-box 1.9 and on the PC's current sing-box
    return {'type': 'vless', 'tag': tag, 'server': n['server'], 'server_port': 443, 'uuid': n['uuid'],
            'flow': 'xtls-rprx-vision', 'packet_encoding': 'xudp',
            'tls': {'enabled': True, 'server_name': n['sni'], 'utls': {'enabled': True, 'fingerprint': n['fp']},
                    'reality': {'enabled': True, 'public_key': n['pbk'], 'short_id': n['sid']}}}


def from_outbound(o):
    t = o['tls']
    return dict(tag=o['tag'], server=o['server'], uuid=o['uuid'], sni=t['server_name'], fp=t['utls']['fingerprint'],
                pbk=t['reality']['public_key'], sid=t['reality']['short_id'], tcp_ms=None)


def nodes_of(cfg):
    """The vless nodes in one config file, either schema."""
    return [from_outbound(o) for o in json.load(open(cfg))['outbounds'] if o.get('type') == 'vless']


def inuse():
    """The nodes in use. With router features off this PC goes by its own config."""
    return nodes_of(PC_CFG if pc_only() else ROUTER_CFG)


def known_where(full=False):
    """server -> location (or {'where','isp'} with full=True) from the last nodes.txt; no network needed."""
    out = {}
    try:
        for l in open(NODES_TXT, encoding='utf-8', errors='replace'):
            m = re.match(r'\s+(?:vless-\d+\s+)?(\d+\.\d+\.\d+\.\d+)\s+\S+\s+(?:\d+ms|\?)\s+(.*)', l)
            if m:
                parts = re.split(r'\s{2,}', m.group(2).strip()) + ['?']
                if parts[0] and parts[0] != '?':
                    out.setdefault(m.group(1), {'where': parts[0], 'isp': parts[1]} if full else parts[0])
    except OSError:
        pass
    return out


def known_failed():
    """Servers in the FAILED section of the last nodes.txt: they failed the test, do not pick them blind."""
    out, failed = set(), False
    try:
        for l in open(NODES_TXT, encoding='utf-8', errors='replace'):
            failed = failed or l.startswith('FAILED')
            m = failed and re.match(r'\s+(\d+\.\d+\.\d+\.\d+)\s', l)
            if m:
                out.add(m.group(1))
    except OSError:
        pass
    return out


def secret(cfg):
    return json.load(open(cfg)).get('experimental', {}).get('clash_api', {}).get('secret', '')


def geolocate(nodes):
    """City and ISP for each node from ip-api, asked from this PC through the PC tunnel (the router is not used;
    the tunnel also keeps the web filter from swallowing the reply). Known locations come from the vault first."""
    known = vault_load()
    for n in nodes:
        e = known.get(n['server'], {})
        if not n.get('where') and e.get('where') and e['where'] != '?':
            n['where'], n['isp'] = e['where'], e.get('isp', '?')
    todo = [n for n in nodes if not n.get('where') or n['where'] == '?']
    if not todo:
        return nodes
    body = '[' + ','.join('{"query":"%s","fields":"query,city,regionName,countryCode,isp"}' % n['server'] for n in todo[:100]) + ']'
    try:
        via = [] if router_only() else ['--socks5-hostname', '127.0.0.1:2080']   # router-only: the router tunnels this PC
        r = run([CURL, '-s', *via, '-m', '20', '-H', 'Content-Type: application/json',
                 '--data-binary', '@-', 'http://ip-api.com/batch'], body, timeout=30)
        geo = {g['query']: g for g in json.loads(r.stdout)}
    except Exception:
        geo = {}
    clean = lambda s, k: re.sub(r'[\s\x00-\x1f\x7f]+', ' ', str(s or '')).strip()[:k] or '?'   # the answer crossed a public node
    for n in todo:
        g = geo.get(n['server'], {})
        n['where'] = f"{clean(g.get('city'), 30)}, {clean(g.get('regionName'), 30)}" if g.get('city') else '?'
        n['isp'] = clean(g.get('isp'), 22)
    return nodes


ERRS = re.compile(r'reality verification failed|certificate is valid for [a-z0-9.*-]+|connection refused|i/o timeout|'
                  r'connection reset|EOF', re.I)


def handshake(nodes):
    """Real REALITY handshake + HTTPS fetch through each node, all at once on the router, each timed end to end.
    One sing-box instance, one local port per node; a node that stops the instance from starting is dropped and retried."""
    for n in nodes:
        n.pop('ok', None)
    todo = list(range(len(nodes)))
    for _ in range(4):
        if not todo:
            break
        cfg = {'log': {'level': 'error'},
               'inbounds': [{'type': 'mixed', 'tag': f'in-{i}', 'listen': '127.0.0.1', 'listen_port': 21000 + i} for i in todo],
               'outbounds': [outbound19(nodes[i], f'n-{i}') for i in todo] + [{'type': 'direct', 'tag': 'direct'}],
               'route': {'rules': [{'ip_is_private': True, 'outbound': 'direct'}] +
                                  [{'inbound': [f'in-{i}'], 'outbound': f'n-{i}'} for i in todo],
                         'final': 'direct', 'auto_detect_interface': True}}
        fetch = ''.join(f"( T=$(curl -s --socks5-hostname 127.0.0.1:{21000 + i} --max-time 10 -o out-{i} -w '%{{time_total}}' "
                        f"https://api.ipify.org 2>/dev/null); IP=$(cat out-{i} 2>/dev/null); "
                        f'if [ -n "$IP" ]; then echo "nt-{i} PASS $IP $T"; else echo "nt-{i} FAIL"; fi ) &\nW="$W $!"\n'
                        + ('wait $W\nW=\n' if k % 5 == 4 else '') for k, i in enumerate(todo))   # 5 at a time keeps timings honest
        # plain `wait` would also wait on the sing-box instance and never return
        script = ('rm -rf /tmp/nt; mkdir -p /tmp/nt; cd /tmp/nt\n'
                  "cat > all.json <<'SBEOF'\n" + json.dumps(cfg) + '\nSBEOF\n'
                  'sing-box run -c all.json > log 2>&1 &\nP=$!\nsleep 2\n'
                  'if ! kill -0 $P 2>/dev/null; then echo DEAD; sed "s/\\x1b\\[[0-9;]*m//g" log | head -5; exit; fi\n'
                  + fetch + '[ -n "$W" ] && wait $W\nkill $P 2>/dev/null\necho ---LOG---\nsed "s/\\x1b\\[[0-9;]*m//g" log | tail -300\nrm -rf /tmp/nt\n')
        out = ssh('sh -s', script)
        if out.startswith('DEAD'):
            bad = re.search(r'outbound\[(\d+)\]|n-(\d+)', out)
            if not bad:
                raise RuntimeError('router test instance did not start: ' + ' '.join(out.split()[1:])[:200])
            k = int(bad.group(1)) if bad.group(1) else todo.index(int(bad.group(2)))
            n = nodes[todo.pop(k)]; n.update(ok=False, exit='-', ms=None, why='rejected by sing-box: bad node parameters')
            continue
        res, _, log = out.partition('---LOG---')
        for line in res.strip().splitlines():
            p = line.split()
            if len(p) < 2 or not p[0].startswith('nt-'):
                continue
            i = p[0][3:]; n = nodes[int(i)]; n['ok'] = p[1] == 'PASS'
            n['exit'], n['ms'] = (p[2], int(float(p[3]) * 1000)) if n['ok'] and len(p) > 3 else ('-', None)
            if not n['ok']:
                m = [ERRS.search(l) for l in log.splitlines() if f'[in-{i}]' in l or f'[n-{i}]' in l]
                n['why'] = next((x.group(0).lower() for x in m if x), 'no-response')
        break
    for n in nodes:
        n.setdefault('ok', False); n.setdefault('exit', '-'); n.setdefault('ms', None); n.setdefault('why', 'no result')
    return nodes


def node_test(nodes):
    """The handshake test, where it measures the real path: on THIS PC with rh-core, or on the router in router-only mode
    (and when rh-core is missing). The router has little RAM, so it tests 20 nodes per run."""
    if router_only() or not (os.path.exists(CORE) or pc_only()):
        for i in range(0, len(nodes), 20):
            handshake(nodes[i:i + 20])
    else:
        local_test(nodes)
    return nodes


def router_test(nodes):
    """The node test (node_test) with geolocation alongside; the results go into the vault."""
    with ThreadPoolExecutor(2) as ex:
        g = ex.submit(geolocate, nodes)
        node_test(nodes)
        g.result()
    vault_record(nodes)
    return nodes


def this_pc():
    try:
        return json.load(open(THIS_PC))
    except (OSError, ValueError):
        return {}


def router_only():
    """True when only the router tunnels (Settings or the setup): no tunnel on this PC. Node tests then run on the router,
    since this PC's own traffic may go through the router tunnel and would skew them."""
    s = settings_load()
    return bool(s['router']) and not s['pc_tunnel']


def all_devices():
    """True when the router tunnels every device on its main network (this PC too) instead of chosen ones."""
    return settings_load()['devices'].strip().lower() == 'all'


def pc_only():
    """True when router features are off (Settings or the setup wizard): only the PC tunnel, no SSH, no router section."""
    return not settings_load()['router']


CURL = shutil.which('curl') or r'C:\Windows\System32\curl.exe'


def test_exe():
    """A copy of rh-core.exe named rh-test.exe for the node tests: the tunnel checks look for rh-core.exe by name,
    and stopping the tunnel must not kill a running test. Refreshed whenever rh-core.exe changes."""
    t = os.path.join(DATA, 'rh-test.exe')
    try:
        if not os.path.exists(t) or os.path.getsize(t) != os.path.getsize(CORE) or os.path.getmtime(t) < os.path.getmtime(CORE):
            os.makedirs(DATA, exist_ok=True); shutil.copy2(CORE, t)
        return t
    except OSError:
        return CORE


def local_test(nodes):
    """The handshake test, run on this PC: one sing-box instance, one port per node, 5 fetches at a time."""
    import tempfile
    if not os.path.exists(CORE):
        raise RuntimeError('rh-core.exe is missing; run the setup again (Settings > Run setup again)')
    for n in nodes:
        n.pop('ok', None)
    with tempfile.TemporaryDirectory() as d:
        cfg = {'log': {'level': 'error', 'output': os.path.join(d, 'log')},
               'inbounds': [{'type': 'mixed', 'tag': f'in-{i}', 'listen': '127.0.0.1', 'listen_port': 23000 + i} for i in range(len(nodes))],
               'outbounds': [outbound19(n, f'n-{i}') for i, n in enumerate(nodes)] + [{'type': 'direct', 'tag': 'direct'}],
               'route': {'rules': [{'inbound': [f'in-{i}'], 'outbound': f'n-{i}'} for i in range(len(nodes))],
                         'final': 'direct', 'auto_detect_interface': True}}
        json.dump(cfg, open(os.path.join(d, 'c.json'), 'w'))
        p = subprocess.Popen([test_exe(), 'run', '-c', os.path.join(d, 'c.json')], stdin=subprocess.DEVNULL,
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, **NOWIN)
        try:
            time.sleep(2)
            if p.poll() is not None:
                raise RuntimeError('the local test instance did not start')
            def one(i):
                r = run([CURL, '-s', '--ssl-no-revoke', '--socks5-hostname', f'127.0.0.1:{23000 + i}', '-m', '10',
                         '-w', ' %{time_total}', 'https://api.ipify.org'], timeout=20)
                parts = r.stdout.split()
                ok = len(parts) == 2 and re.match(r'^\d+\.\d+\.\d+\.\d+$', parts[0])
                nodes[i].update(ok=bool(ok), exit=parts[0] if ok else '-', ms=int(float(parts[1]) * 1000) if ok else None)
            with ThreadPoolExecutor(5) as ex:
                list(ex.map(one, range(len(nodes))))
        finally:
            p.kill(); p.wait()
        try:
            log = open(os.path.join(d, 'log'), encoding='utf-8', errors='replace').read()
        except OSError:
            log = ''
    for i, n in enumerate(nodes):
        if not n.get('ok'):
            m = [ERRS.search(l) for l in log.splitlines() if f'[in-{i}]' in l or f'[n-{i}]' in l]
            n.update(ok=False, exit='-', ms=None, why=next((x.group(0).lower() for x in m if x), 'no-response'))
        n.setdefault('where', '?')
    return nodes


# ---- vault: every node that ever passed a router test, kept across scans for when the feed cannot be reached
KEYS = ('server', 'uuid', 'pbk', 'sid', 'sni', 'fp', 'where', 'isp')


def vault_load():
    try:
        return json.load(open(VAULT))
    except (OSError, ValueError):
        return vault_seed()


def vault_seed():
    """First run: build the vault from the nodes listed in nodes.txt (in use + spares)."""
    v, meta, cur = {}, known_where(full=True), None
    try:
        for l in open(NODES_TXT, encoding='utf-8', errors='replace'):
            if l.startswith('FAILED'):
                break
            m = re.match(r'\s+(?:vless-\d+\s+)?(\d+\.\d+\.\d+\.\d+)\s+(\S+)\s+(?:(\d+)ms|\?)', l)
            if m:
                cur = dict(server=m.group(1), sni=m.group(2), fp='chrome', ms=int(m.group(3) or 0) or None, **meta.get(m.group(1), {}))
                continue
            k = cur and re.match(r'\s+uuid=(\S+)\s+pbk=(\S+)\s+sid=(\S+)', l)
            if k:
                cur.update(uuid=k.group(1), pbk=k.group(2), sid='' if k.group(3) == '(empty)' else k.group(3),
                           last_ok=str(datetime.date.today()), passes=1, fail_streak=0)
                v[cur['server']] = cur; cur = None
    except OSError:
        pass
    for n in inuse():
        v.setdefault(n['server'], dict({k: n.get(k) for k in KEYS}, last_ok=str(datetime.date.today()), passes=1, fail_streak=0))
    if v:
        jsave(VAULT, v, indent=1)
    return v


def vault_record(nodes):
    """Router results into the vault: a pass refreshes the entry, three failures in a row remove it."""
    v, today = vault_load(), str(datetime.datetime.now().replace(microsecond=0))
    for n in nodes:
        if 'ok' not in n:
            continue
        e = v.get(n['server'])
        if n['ok']:
            e = v.setdefault(n['server'], {'passes': 0}); e.update({k: n.get(k) for k in KEYS if n.get(k) is not None})
            e.update(last_ok=today, ms=n.get('ms'), passes=e.get('passes', 0) + 1, fail_streak=0)
        elif e:
            e['fail_streak'] = e.get('fail_streak', 0) + 1
            if e['fail_streak'] >= 3:
                del v[n['server']]
    v = dict(sorted(v.items(), key=lambda kv: kv[1].get('last_ok', ''), reverse=True)[:200])
    jsave(VAULT, v, indent=1)


def vault_nodes(exclude=()):
    """Vault entries as nodes, most recently verified first, then fastest."""
    v = vault_load()
    out = [dict(e, fp=e.get('fp') or 'chrome', tcp_ms=None) for s, e in v.items() if s not in exclude]
    return sorted(out, key=lambda n: (n.get('last_ok', '')[:10], -(n.get('ms') or 9e9)), reverse=True)


# ---- when this PC has no internet: the router still does (it reaches the nodes directly)
def router_up():
    if pc_only():
        return False
    try:
        return ssh('echo up').strip() == 'up'
    except Exception:
        return False


def router_feed():
    """The feeds downloaded and pre-filtered by the router, for when this PC cannot reach GitHub."""
    out = ssh(' ; '.join(f"curl -s -m 40 '{u}' | grep '^vless://' | grep reality | grep vision" for u in FEEDS))
    if 'vless://' not in out:
        raise RuntimeError('the router could not download the node lists either')
    return out


def router_ping(nodes):
    """443 connect time measured on the router, 32 at a time. Same contract as tcp_probe."""
    body = ''
    for k, n in enumerate(nodes):
        body += (f"( echo \"{k} $(curl -sk -o /dev/null --connect-timeout 2 -m 3 -w '%{{time_connect}}' https://{n['server']}/)\" ) &\nW=\"$W $!\"\n"
                 + ('wait $W\nW=\n' if k % 32 == 31 else ''))
    res = {}
    for l in ssh('sh -s', body + '[ -n "$W" ] && wait $W\n').splitlines():
        p = l.split()
        if len(p) == 2 and p[0].isdigit():
            try:
                t = float(p[1].replace(',', '.'))
            except ValueError:
                continue
            res[int(p[0])] = int(t * 1000) if t > 0 else None
    for k, n in enumerate(nodes):
        n['tcp_ms'] = res.get(k)
    return sorted([n for n in nodes if n['tcp_ms'] is not None], key=lambda n: n['tcp_ms'])


def probe(nodes):
    """443 ping from this PC; if nothing at all answers (this PC is offline), ping from the router instead."""
    if router_only():   # this PC's traffic may go through the router tunnel: ping from the router itself
        return router_ping(nodes)
    live = tcp_probe(nodes)
    if live or len(nodes) < 5 or pc_only():
        return live
    say('nothing answers from this PC (no internet here?); pinging from the router instead...')
    return router_ping(nodes)


def rescue(k=6):
    """All nodes dead: test the vault on the router, put the best k in both files, deploy both sides.
    Needs no internet on this PC. If the vault has too few working nodes, the router scans the feed itself.
    Router unreachable: the most recently verified vault nodes go in untested. Returns a sentence for the window."""
    cur = inuse(); pool = {n['server']: n for n in vault_nodes()}
    for n in cur:
        pool.setdefault(n['server'], n)
    if not os.path.exists(CORE) and not router_up():
        best = [n for n in pool.values() if n['server'] not in {c['server'] for c in cur}][:k] or list(pool.values())[:k]
        if not best:
            raise RuntimeError('the vault is empty and nothing can test nodes; nothing to fall back on')
        write_configs(best)
        return (f'Nothing could test nodes (no rh-core, router unreachable): the {len(best)} most recently verified vault nodes '
                'went into the configs untested. Run the setup again.')
    test = list(pool.values())[:TOP]
    say(f"rescue: {'this PC' if os.path.exists(CORE) else 'the router'} is testing {len(test)} vault nodes...")
    router_test(test); ok = sorted([n for n in test if n['ok']], key=lambda n: n['ms'])
    say(f'rescue: {len(ok)} of {len(test)} vault nodes work')
    if len(ok) < k:
        say('rescue: too few, scanning the feed for more...')
        try:
            more, _ = find_spares(TOP, {n['server'] for n in test})
            ok += more
        except Exception as e:
            say(f'rescue: feed scan failed ({e}); using what works')
    if not ok:
        raise RuntimeError('no vault node and no feed node works right now; nothing changed')
    best = ok[:k]; router_ok = deploy(best, test)
    where = 'the PC tunnel' + ('' if pc_only() else ' and on the router' if router_ok else ' (router push FAILED, press Push to router later)')
    return f"Rescue done: {len(best)} working nodes (best {best[0]['ms']} ms) are in {where}."


def parse_delay(text):
    try:
        d = json.loads(text)
    except ValueError:
        return None, 'no response'
    return (d['delay'], '') if 'delay' in d else (None, d.get('message', 'failed'))


def check_phone(nodes):
    """Fresh test of every in-use node through the router's live tunnel. Sets phone_ms / phone_why on each node
    ("phone" = the router side, where the chosen LAN devices go through)."""
    script = (f'H="Authorization: Bearer {secret(ROUTER_CFG)}"\n'
              'echo "svc $(pgrep -f "/etc/sing-box/config[.]json" | head -1)"\n'
              f'for t in {" ".join(n["tag"] for n in nodes)}; do '
              f'( r=$(curl -s -m 12 -H "$H" "{API}/proxies/$t/delay?url={TEST_URL}&timeout=8000"); '
              f'case "$r" in *delay*) ;; *) r=$(curl -s -m 12 -H "$H" "{API}/proxies/$t/delay?url={TEST_URL}&timeout=8000");; esac; '
              f'echo "$t $r" ) & done; wait\n'
              f'echo "now $(curl -s -m 5 -H "$H" {API}/proxies/auto)"\n'
              'echo "cfg $(md5sum /etc/sing-box/config.json 2>/dev/null | cut -c1-32)"\n')
    res = dict(l.partition(' ')[::2] for l in ssh('sh -s', script).splitlines() if l.strip())
    for n in nodes:
        n['phone_ms'], n['phone_why'] = parse_delay(res.get(n['tag'], ''))
    try:
        now = json.loads(res.get('now', '')).get('now')
    except ValueError:
        now = None
    return {'service': bool(res.get('svc', '').strip()), 'now': now, 'cfg': res.get('cfg', '').strip()}


def pc_get(path, timeout=15):
    req = urllib.request.Request(API + path, headers={'Authorization': 'Bearer ' + secret(PC_CFG)})
    return urllib.request.build_opener(urllib.request.ProxyHandler({})).open(req, timeout=timeout).read().decode()


def set_mark(nodes):
    """Tag of an unused direct outbound that names the node set; the Clash API lists it, so the running set is known."""
    return 'set-' + hashlib.md5(' '.join(n['server'] for n in nodes).encode()).hexdigest()[:10]


def pc_delay(tag):
    try:
        return parse_delay(pc_get(f"/proxies/{tag}/delay?url={TEST_URL}&timeout=8000"))
    except urllib.error.HTTPError as e:
        return parse_delay(e.read().decode())
    except OSError:
        return None, 'no response'


def check_pc(nodes):
    """Fresh test of every node through the PC's running daemon, one retry each. None if the API is down.
    If the daemon still runs an older node set (files changed, no restart yet), nothing is tested: vless-N
    would still point at the old server. Returns {'stale': True} then."""
    try:
        px = json.loads(pc_get('/proxies', 5))['proxies']
    except (OSError, ValueError, KeyError):
        for n in nodes:
            n['pc_ms'], n['pc_why'] = None, 'api down'
        return None
    if set_mark(nodes) not in px:
        for n in nodes:
            n['pc_ms'] = n['pc_why'] = None
        return {'now': px.get('auto', {}).get('now'), 'exit': None, 'stale': True}
    def one(n):
        n['pc_ms'], n['pc_why'] = pc_delay(n['tag'])
        if n['pc_ms'] is None:
            n['pc_ms'], n['pc_why'] = pc_delay(n['tag'])
    with ThreadPoolExecutor(len(nodes) or 1) as ex:
        list(ex.map(one, nodes))
    return {'now': px.get('auto', {}).get('now'), 'exit': pc_exit(), 'stale': False}


def pc_exit():
    curl = shutil.which('curl') or r'C:\Windows\System32\curl.exe'
    return run([curl, '-s', '--ssl-no-revoke', '-m', '10', '--socks5-hostname', '127.0.0.1:2080', 'https://api.ipify.org']).stdout.strip() or None


def pc_status():
    """What the PC tunnel is doing right now, without SSH."""
    st = {'service': core_running(), 'listening': False, 'api': False, 'now': None, 'exit': None}
    s = socket.socket(); s.settimeout(1)
    try:
        s.connect(('127.0.0.1', 2080)); st['listening'] = True
    except OSError:
        pass
    finally:
        s.close()
    try:
        st['now'], st['api'] = json.loads(pc_get('/proxies/auto', 4)).get('now'), True
    except (OSError, ValueError):
        pass
    if st['listening']:
        st['exit'] = pc_exit()
    return st


def table(nodes):
    say(f"\n{'#':>2} {'server':16} {'front':20} {'where':24} {'tcp':>5} {'tunnel':>7}  result")
    for i, n in enumerate(sorted(nodes, key=lambda n: (not n['ok'], n['ms'] or 9e9)), 1):
        res = (f"PASS exit={n['exit']}" if n['ok'] else f"FAIL {n['why']}") + ('  (in use)' if n.get('inuse') else '')
        tun = f"{n['ms']}ms" if n['ok'] else '-'
        say(f"{i:>2} {n['server']:16} {n['sni']:20} {n.get('where', '?'):24} {str(n.get('tcp_ms') or '?'):>5} {tun:>7}  {res}")


def write_txt(nodes, best):
    def row(n, tag=''):
        ms = f"{n['ms']:5}ms" if n.get('ms') else '      ?'
        return [f"  {tag:8} {n['server']:16} {n['sni']:20} {ms}  {n.get('where', '?'):26}  {n.get('isp', '?')}",
                f"           uuid={n['uuid']}  pbk={n['pbk']}  sid={n['sid'] or '(empty)'}"]
    L = [f'VERIFIED REALITY NODES  -  tested {datetime.date.today()} by Reality Hop (REALITY handshake + HTTPS fetch)', '',
         'tunnel ms = full HTTPS fetch through the node. TCP-open alone proves nothing:',
         'a rejected key is silently forwarded to the front site and looks like a healthy host.', '',
         'IN USE  (both configs, urltest group "auto")']
    for i, n in enumerate(best, 1):
        L += row(n, f'vless-{i}')
    spares = [n for n in sorted(nodes, key=lambda n: n['ms'] or 9e9) if n.get('ok') and n not in best]
    L += ['', 'SPARES  (passed, not in use; the window or  python engine.py --apply N  rotates them in)']
    for n in spares:
        L += row(n)
    L += ['', 'FAILED  (do not retest until the feed changes their key)']
    L += [f"  {n['server']:16} {n['sni']:20} {n.get('why', '')}" for n in nodes if not n.get('ok')]
    L += ['', 'WORKFLOW: Reality Hop window     python engine.py --check (health)     python engine.py (find + rank + apply)']
    open(NODES_TXT, 'w', encoding='utf-8').write('\n'.join(L) + '\n')   # city names such as Łódź
    say(f'updated {os.path.basename(NODES_TXT)}')


def write_configs(best):
    """Put the node set into both configs, each in its own schema; everything else in them stays as is."""
    r = json.load(open(ROUTER_CFG))
    vl = [outbound19(n, f'vless-{i}') for i, n in enumerate(best, 1)]
    rest = [o for o in r['outbounds'] if o.get('type') != 'vless']
    for o in rest:
        if o.get('type') == 'urltest':
            o['outbounds'] = [v['tag'] for v in vl]
    r['outbounds'] = vl + rest
    jsave(ROUTER_CFG, r, indent=2)
    pc = json.load(open(PC_CFG))
    pv = [outbound19(n, f'vless-{i}') for i, n in enumerate(best, 1)]
    prest = [o for o in pc['outbounds'] if o.get('type') != 'vless' and not o.get('tag', '').startswith('set-')]
    for o in prest:
        if o.get('type') == 'urltest':
            o['outbounds'] = [v['tag'] for v in pv]
    pc['outbounds'] = pv + prest + [{'type': 'direct', 'tag': set_mark(best)}]
    jsave(PC_CFG, pc, indent=2)


PUSH_SH = r"""for pid in $(pgrep -f 'sb-test[.]json'); do kill $pid; done; sleep 1
mkdir -p /etc/sing-box; mv /tmp/sb-new.json /etc/sing-box/config.json
tr -d '\r' < /tmp/nft-devices.sh > /etc/sing-box/nft-devices.sh; chmod +x /etc/sing-box/nft-devices.sh
uci set sing-box.main.enabled='1'; uci set sing-box.main.user='root'; uci set sing-box.main.conffile='/etc/sing-box/config.json'; uci commit sing-box
uci -q delete firewall.sbphone; uci -q get firewall.realityhop >/dev/null || uci set firewall.realityhop=include
uci set firewall.realityhop.type='script'; uci set firewall.realityhop.path='/etc/sing-box/nft-devices.sh'; uci set firewall.realityhop.reload='1'
uci set firewall.realityhop.enabled='1'; uci set firewall.realityhop.fw4_compatible='1'; uci commit firewall
/etc/init.d/sing-box enable; /etc/init.d/sing-box restart; sleep 7; fw4 reload >/dev/null 2>&1; sleep 2
pgrep -af 'sing-box run' | grep -q '/etc/sing-box/config.json' && echo 'sing-box service: running' || { echo 'sing-box service: NOT RUNNING'; logread | grep -iE 'sing-box' | tail -5; }
netstat -ln 2>/dev/null | grep -q ':7895' && echo 'tproxy: listening' || echo 'tproxy: NOT listening'
nft list tables 2>/dev/null | grep -q singbox && echo 'nft: loaded' || echo 'nft: absent'
ip rule list | grep -q 'fwmark 0x1' && echo 'policy route: ok' || echo 'policy route: MISSING'
"""


def devices():
    """The LAN addresses that go through the router tunnel (Settings / setup wizard)."""
    return [x for x in re.split(r'[\s,;]+', settings_load()['devices']) if lan_ip(x)]


def nft_script():
    body = open(NFT_SH, encoding='utf-8').read().replace('\r', '')
    if all_devices():   # every device on the main network (br-lan); its IPv6 is refused so nothing goes around the tunnel
        return (body.replace('@ELEMENTS@', '').replace('@WHO@', 'iifname "br-lan" meta nfproto ipv4')
                .replace('@V6@', '    iifname "br-lan" oifname != "br-lan" meta nfproto ipv6 counter reject'))
    ips = devices()
    return (body.replace('@ELEMENTS@', f"elements = {{ {', '.join(ips)} }}" if ips else '').replace('@WHO@', 'ip saddr @tunneled')
            .replace('@V6@', ''))


def push():
    """Upload router-config.json and the device firewall rule, check the config on the router, restart its sing-box,
    install the self-repair. Runs over Windows' own ssh; nothing else is needed on this PC."""
    if pc_only():
        return 'router features are off: the router is not touched'
    os.makedirs(ROUTER_DIR, exist_ok=True)   # copies of what goes to the router, for anyone who wants to read them
    open(os.path.join(ROUTER_DIR, 'nft-devices.sh'), 'w', encoding='utf-8', newline='\n').write(nft_script())
    shutil.copy(HEAL_SH, os.path.join(ROUTER_DIR, 'heal.sh'))
    script = ("cat > /tmp/sb-new.json <<'SBEOF'\n" + open(ROUTER_CFG, encoding='utf-8').read() + "\nSBEOF\n"
              "sing-box check -c /tmp/sb-new.json || { echo 'CONFIG REJECTED, nothing deployed'; exit 1; }\n"
              "cat > /tmp/nft-devices.sh <<'NFTEOF'\n" + nft_script() + "NFTEOF\n" + PUSH_SH
              + f"echo '{'all' if all_devices() else ' '.join(devices())}' > /etc/sing-box/rh-devices\n"   # for the other PCs
              "echo \"pushed-md5 $(md5sum /etc/sing-box/config.json | cut -c1-32)\"\n")
    out = ssh('sh -s', script).strip() or 'no answer from the router'
    m = re.search(r'^pushed-md5 ([0-9a-f]{32})$', out, re.M)
    if m and 'CONFIG REJECTED' not in out:
        router_synced(m.group(1))
    out = re.sub(r'\n?pushed-md5 \S*', '', out)
    if 'sing-box service: running' in out:
        try:
            out += '\n' + push_heal()
        except Exception as e:
            out += f'\nself-repair: not installed ({e})'
    return out


def push_heal():
    """Router self-repair: heal.sh + the vault (router schema, nodes not in use) + a 30-minute cron line."""
    used = {n['server'] for n in inuse()}
    vault = [{k: v for k, v in outbound19(n, '').items() if k != 'tag'} for n in vault_nodes(used)][:60]
    sh = open(HEAL_SH, encoding='utf-8').read().replace('\r', '')
    return ssh('sh -s', "cat > /etc/sing-box/vault.json <<'VEOF'\n" + json.dumps(vault) + "\nVEOF\n"
               "cat > /etc/sing-box/heal.sh <<'HEOF'\n" + sh + "HEOF\n"
               'chmod +x /etc/sing-box/heal.sh\n'
               "sed -i '/heal.sh/d' /etc/crontabs/root; { echo '*/30 * * * * /etc/sing-box/heal.sh' >> /etc/crontabs/root; /etc/init.d/cron restart; }\n"
               f'echo "self-repair: installed, {len(vault)} vault nodes on the router"\n').strip()


def file_md5(path):
    try:
        return hashlib.md5(open(path, 'rb').read()).hexdigest()
    except OSError:
        return ''


def router_synced(router_md5):
    """Remember what the router runs and the router-config.json that matches it."""
    set_this_pc(router_md5=router_md5, router_local=file_md5(ROUTER_CFG))


def router_stale(router_md5):
    """True when router-config.json has changes the router does not run yet."""
    t = this_pc()
    return bool(router_md5) and (router_md5 != t.get('router_md5') or file_md5(ROUTER_CFG) != t.get('router_local'))


def router_sync():
    """Several PCs can share one router, and the router runs what was pushed last. When its config changed since this PC
    last pushed or looked (another PC pushed, or the router's self-repair swapped nodes), this PC takes the router's
    nodes and device list instead of pushing its own over them. Returns a sentence when the nodes changed, else None."""
    out = ssh("md5sum /etc/sing-box/config.json 2>/dev/null | cut -c1-32; echo ---; cat /etc/sing-box/rh-devices 2>/dev/null; "
              "echo; echo ---; cat /etc/sing-box/heal.state 2>/dev/null; echo; echo ---; cat /etc/sing-box/config.json 2>/dev/null")
    md5, devs, heal, cfg = [p.strip() for p in (out.split('---\n', 3) + ['', '', '', ''])[:4]]
    if not md5 or md5 == this_pc().get('router_md5'):
        return None
    try:
        r = json.loads(cfg); nodes = [from_outbound(o) for o in r['outbounds'] if o.get('type') == 'vless']
    except (ValueError, KeyError, TypeError):
        return None
    if not nodes:
        return None
    before = [n['server'] for n in inuse()]
    meta = vault_load()
    for n in nodes:   # a node another PC found is not in this PC's vault: no latency or place known yet
        n.update({'ms': None, **{k: v for k, v in meta.get(n['server'], {}).items() if k in ('where', 'isp', 'fp', 'ms') and v}})
    jsave(ROUTER_CFG, r, indent=2)   # the router's own file: its API secret too, so this PC can still test it
    write_configs(nodes); write_txt(nodes, nodes)
    if devs == 'all' or (devs and all(lan_ip(x) for x in devs.split())):
        settings_save({'devices': devs})
    router_synced(md5)
    if heal:
        ssh('rm -f /etc/sing-box/heal.state')
    if [n['server'] for n in nodes] == before:
        return None
    pc_push()
    who = 'The router repaired itself with vault nodes' if heal else "Another PC changed the router's nodes"
    return f"{who} ({', '.join(n['server'] for n in nodes)}); this PC follows them now."


def core_mode():
    """True when rh-core.exe exists (the setup downloads it)."""
    return os.path.exists(CORE)


def core_stop():
    """Stop every rh-core: the boot task's copy (only the task can stop it) and our own; wait until 2080 is free."""
    if boot_task_exists():
        run(['schtasks', '/end', '/tn', BOOT_TASK], timeout=20)
    run(['taskkill', '/F', '/IM', os.path.basename(CORE)], timeout=20)
    for _ in range(20):
        if not core_running() and not port_taken(2080):
            return True
        time.sleep(0.5)
    return False


def core_direct():
    os.makedirs(LOGS, exist_ok=True)
    subprocess.Popen([CORE, 'run', '-c', PC_CFG], cwd=os.path.dirname(CORE), stdin=subprocess.DEVNULL,
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                     creationflags=subprocess.CREATE_NO_WINDOW | subprocess.CREATE_NEW_PROCESS_GROUP, startupinfo=hidden())


def core_start():
    """Start rh-core hidden: through the boot task when it exists (so it lives in the same place it does after a
    reboot), otherwise directly."""
    if boot_task_exists() and run(['schtasks', '/run', '/tn', BOOT_TASK], timeout=20).returncode == 0:
        return 'task'
    core_direct(); return 'direct'


def core_up(wait=15):
    for _ in range(wait):
        time.sleep(1)
        try:
            if set_mark(nodes_of(PC_CFG)) in json.loads(pc_get('/proxies', 3))['proxies']:
                return True
        except (OSError, ValueError, KeyError):
            pass
    return False


LEGACY_CORE = 'sb-core.exe'   # the PC tunnel's name in the test builds before 1.0; their boot task may still start it


def legacy_ps():
    """PowerShell (needs admin): remove every scheduled task that starts the old sb-core.exe, then stop it."""
    return ("Get-ScheduledTask | Where-Object { $_.Actions.Execute -like '*sb-core.exe*' } | "
            "Unregister-ScheduledTask -Confirm:$false -ErrorAction SilentlyContinue; "
            "Stop-Process -Name sb-core -Force -ErrorAction SilentlyContinue")


def port_blocker():
    """The other program on port 2080 that keeps rh-core from starting, or None. The old sb-core is stopped if it can be."""
    owner = port_owner(2080) if port_taken(2080) else None
    if owner and owner.lower() == LEGACY_CORE:
        run(['taskkill', '/F', '/IM', LEGACY_CORE], timeout=20); time.sleep(1)
        owner = port_owner(2080) if port_taken(2080) else None
    return owner if owner and owner.lower() != os.path.basename(CORE).lower() else None


def blocker_hint(owner):
    return (f'{owner} uses port 2080, so the PC tunnel cannot start. '
            + ('It is the tunnel of an old Reality Hop test build, started at boot: press Run setup again, which removes it '
               '(one UAC prompt).' if owner.lower() == LEGACY_CORE else 'Close that program, then press Restart PC tunnel.'))


def core_restart():
    """Plain sing-box runs the PC tunnel: restart it on pc-config.json. No UAC.
    If the task-started copy does not come up, a direct start is the fallback, so the tunnel is never left down."""
    if router_only():
        say('router-only mode: no tunnel on this PC'); return True
    owner = port_blocker()
    if owner:
        say('PC tunnel: ' + blocker_hint(owner)); hist_add(event=('error', f'{owner} holds port 2080'))
        return False
    if not core_stop():
        say('an old rh-core would not stop; trying anyway')
    how = core_start()
    if core_up():
        say(f'PC tunnel restarted ({how})'); return True
    if how == 'task':
        say('the boot task did not bring rh-core up; starting it directly instead')
        core_stop(); core_direct()
        if core_up():
            say('PC tunnel restarted (direct)'); return True
    chk = run([CORE, 'check', '-c', PC_CFG], timeout=30, cwd=os.path.dirname(CORE))
    say('PC tunnel did not come back after the restart'
        + (f": config check says {(chk.stderr or chk.stdout).strip()[:200]}" if chk.returncode else '; see logs/core.log'))
    hist_add(event=('error', 'PC tunnel did not come back after a restart'))
    return False


def core_log_trim():
    """Keep logs/core.log to its last 200 lines once it passes 256 KB (sing-box appends, so trimming in place is safe)."""
    try:
        if os.path.getsize(CORE_LOG) > 256_000:
            lines = open(CORE_LOG, encoding='utf-8', errors='replace').readlines()[-200:]
            with open(CORE_LOG, 'r+', encoding='utf-8') as f:
                f.seek(0); f.writelines(lines); f.truncate()
    except OSError:
        pass


def regroup():
    """Ask the PC tunnel to re-test its group now, so the lowest-ping node takes over without a restart."""
    try:   # (the router's sing-box 1.9 answers this with an empty reply; it re-tests on its own every 10 min)
        pc_get(f"/group/auto/delay?url={TEST_URL}&timeout=5000", 20)
    except Exception:
        pass


def core_version():
    try:
        return run([CORE, 'version'], timeout=20).stdout.split()[2]
    except Exception:
        return None


def vkey(tag):
    """Sort key for sing-box versions: 1.13.0-alpha.2 < 1.13.0-beta.1 < 1.13.0-rc.1 < 1.13.0."""
    m = re.match(r'v?(\d+)\.(\d+)\.(\d+)(?:-(alpha|beta|rc)\.?(\d+)?)?', tag or '')
    if not m:
        return (0,)
    return int(m[1]), int(m[2]), int(m[3]), {'alpha': 0, 'beta': 1, 'rc': 2}.get(m[4], 3), int(m[5] or 0)


def core_release():
    """The newest sing-box release on the chosen channel (Settings): stable = the newest full release, testing = the
    newest of all, alpha and beta included. From the official releases of github.com/SagerNet/sing-box."""
    rels = json.loads(gh_get('https://api.github.com/repos/SagerNet/sing-box/releases?per_page=40', 30))
    testing = settings_load()['singbox'] == 'testing'
    rels = [r for r in rels if not r.get('draft') and (testing or not r.get('prerelease'))
            and any(a['name'] == f"sing-box-{r['tag_name'].lstrip('v')}-windows-amd64.zip" for a in r.get('assets', []))]
    if not rels:
        raise RuntimeError('no sing-box release for Windows found on GitHub')
    return max(rels, key=lambda r: vkey(r['tag_name']))


def core_update(force=False):
    """Keep sing-box current on the chosen channel, also when Reality Hop itself has no new release (the watcher and the
    window check daily; switching the channel installs that channel's build at once, newer or older).
    SHA-256 checked against GitHub's digest, then the new build must accept the PC config before it replaces the old one."""
    import tempfile, zipfile
    rel, channel = core_release(), settings_load()['singbox']
    ver = rel['tag_name'].lstrip('v')
    set_this_pc(core_checked=time.time(), core_channel=channel)
    if ver == core_version() and not force:
        say(f'sing-box {ver} is current ({channel})'); return None
    a = next(x for x in rel['assets'] if x['name'] == f'sing-box-{ver}-windows-amd64.zip')
    with tempfile.TemporaryDirectory() as d:
        z = os.path.join(d, a['name'])
        say(f"downloading sing-box {ver} ({a['size'] // 1_000_000} MB) from the official releases (github.com/SagerNet/sing-box)...")
        with open(z, 'wb') as f:
            f.write(gh_get(a['browser_download_url'], 180))
        if a.get('digest') != 'sha256:' + hashlib.sha256(open(z, 'rb').read()).hexdigest():
            raise RuntimeError('download does not match the checksum GitHub publishes; not installed')
        new = os.path.join(d, 'rh-core.exe')
        with zipfile.ZipFile(z) as zf, zf.open(f'sing-box-{ver}-windows-amd64/sing-box.exe') as src, open(new, 'wb') as dst:
            shutil.copyfileobj(src, dst)
        chk = run([new, 'check', '-c', PC_CFG], timeout=30, cwd=os.path.dirname(CORE)) if nodes_of(PC_CFG) else None
        if chk and chk.returncode:
            raise RuntimeError(f'sing-box {ver} rejects the PC config, keeping the current build: {chk.stderr.strip()[:200]}')
        was = core_running()
        try:
            core_stop(); shutil.copy(new, CORE)
            say(f'sing-box {ver} ({channel}) installed as {os.path.basename(CORE)}')
        finally:
            if was:
                core_restart()   # back up even if the copy failed
    hist_add(event=('restart', f'rh-core updated to {ver}'))
    return ver


def port_owner(port):
    """Image name of the process listening on a local port, or None."""
    r = run(['powershell', '-NoProfile', '-Command',
             f'(Get-Process -Id (Get-NetTCPConnection -LocalPort {port} -State Listen -ErrorAction SilentlyContinue | '
             'Select-Object -First 1).OwningProcess -ErrorAction SilentlyContinue).ProcessName'], timeout=30)
    name = r.stdout.strip()
    return name + '.exe' if name else None


def hidden():
    si = subprocess.STARTUPINFO(); si.dwFlags |= subprocess.STARTF_USESHOWWINDOW; si.wShowWindow = 0   # SW_HIDE
    return si


def core_running():
    """Is rh-core alive? Asks the process list, not the port, so a busy PC right after boot is not mistaken for a dead tunnel."""
    r = run(['tasklist', '/FI', f'IMAGENAME eq {os.path.basename(CORE)}', '/NH'], timeout=20)
    return os.path.basename(CORE).lower() in r.stdout.lower()


def core_update_due():
    """True when the daily sing-box check is due, or the channel was changed since the last one."""
    t = this_pc()
    return core_mode() and (t.get('core_channel') != settings_load()['singbox'] or time.time() - t.get('core_checked', 0) > 24 * 3600)


def core_ensure():
    """Core mode: start the PC tunnel only if it is really not running: no rh-core process and port 2080 closed
    on three tries 5 s apart. Only local checks, no traffic. Never while Reality Hop is stopped."""
    if stopped() or router_only() or core_running():
        return True
    for _ in range(3):
        if port_open(2080):
            return True
        time.sleep(5)
    say('PC tunnel not running; starting it'); return core_restart()


BOOT_TASK = 'RealityHop tunnel'


def boot_task_exists():
    return run(['schtasks', '/query', '/tn', BOOT_TASK], timeout=15).returncode == 0


def boot_task(on):
    """Windows starts rh-core at boot, before logon and before the VPN app, as this user ("do not store password" mode),
    so the user-level watcher can still restart it. Creating or removing the task needs one UAC prompt."""
    if on:
        ps = boot_task_ps(CORE, PC_CFG)
    else:
        ps = f"Unregister-ScheduledTask -TaskName '{BOOT_TASK}' -Confirm:$false"
    say(('adding' if on else 'removing') + ' the boot-time start of rh-core; answer the UAC prompt...')
    elevated(ps)
    ok = boot_task_exists() == on
    say(('rh-core now starts at boot, before the VPN app' if on else 'boot-time start removed') if ok else 'not changed (UAC declined or failed)')
    return ok


def boot_task_ps(core, cfg):
    """PowerShell (needs admin) that creates the boot task, or points it at a new rh-core; a task switched off by Stop
    stays off."""
    user = f"{os.environ.get('USERDOMAIN', '')}\\{os.environ.get('USERNAME', '')}".lstrip('\\')
    exe_, args, wd = psq(core), psq(f'run -c "{cfg}"'), psq(os.path.dirname(core))
    return (f"$was = (Get-ScheduledTask -TaskName '{BOOT_TASK}' -ErrorAction SilentlyContinue).State; "
            f"$a = New-ScheduledTaskAction -Execute {exe_} -Argument {args} -WorkingDirectory {wd}; "
            "$t = New-ScheduledTaskTrigger -AtStartup; "
            f"$p = New-ScheduledTaskPrincipal -UserId {psq(user)} -LogonType S4U -RunLevel Limited; "
            "$s = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 5 "
            "-RestartInterval (New-TimeSpan -Minutes 1) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable; "
            f"Register-ScheduledTask -TaskName '{BOOT_TASK}' -Action $a -Trigger $t -Principal $p -Settings $s -Force | Out-Null; "
            f"if ($was -eq 'Disabled') {{ Disable-ScheduledTask -TaskName '{BOOT_TASK}' | Out-Null }}")


def elevated(ps):
    """Run a PowerShell snippet as administrator (one UAC prompt). Returns the exit code (2 = declined)."""
    import base64
    enc = base64.b64encode(ps.encode('utf-16-le')).decode()
    return run(['powershell', '-NoProfile', '-Command',
                "try { $p = Start-Process powershell -Verb RunAs -Wait -PassThru -WindowStyle Hidden -ErrorAction Stop "
                f"-ArgumentList '-NoProfile','-EncodedCommand','{enc}'; exit $p.ExitCode }} catch {{ exit 2 }}"], timeout=300).returncode


def stopped():
    """True after Stop: nothing starts the PC tunnel or the watcher until Start."""
    return bool(this_pc().get('stopped'))


def set_stopped(on):
    set_this_pc(stopped=bool(on))


def task_state():
    r = run(['schtasks', '/query', '/tn', BOOT_TASK, '/fo', 'list'], timeout=15)
    m = re.search(r'Status:\s*(\S+)', r.stdout)
    return m.group(1) if r.returncode == 0 and m else None   # Ready / Running / Disabled / None (no task)


def manager_stop():
    """Stop everything on this PC for when you are away: the PC tunnel, the watcher (and its tray icon), the start
    at logon and the start at boot. The router and the files are not touched. Returns lines for the window."""
    set_stopped(True); out = []
    if boot_task_exists():
        elevated(f"schtasks /end /tn '{BOOT_TASK}'; schtasks /change /tn '{BOOT_TASK}' /disable")
        out.append('boot start: switched off' if task_state() == 'Disabled' else
                   'boot start: NOT switched off (UAC declined); rh-core will still start at boot')
    startup(False); out.append('background watcher: stopped, no longer starts at logon')
    core_stop(); out.append('PC tunnel: stopped' if not core_running() else 'PC tunnel: could not be stopped')
    if not router_only():
        out.append('VPN app: turn its proxy off now (PIA: Settings > Multi-Hop, untick "Multi-hop and Obfuscation"), or it cannot connect')
    for l in out:
        say(l)
    return out


def manager_start():
    """Undo Stop: boot start on again, PC tunnel running, watcher on at logon. Returns lines for the window."""
    set_stopped(False); out = []
    if boot_task_exists() and task_state() == 'Disabled':
        elevated(f"schtasks /change /tn '{BOOT_TASK}' /enable")
        out.append('boot start: on' if task_state() != 'Disabled' else 'boot start: still OFF (UAC declined)')
    if not router_only():
        out.append('PC tunnel: running' if core_restart() else 'PC tunnel: did not come up, see the log')
    startup(True); out.append('background watcher: running, starts at every logon')
    if not router_only():
        out.append('VPN app: turn its proxy back on (PIA: Settings > Multi-Hop, tick "Multi-hop and Obfuscation", SOCKS5 127.0.0.1:2080)')
    for l in out:
        say(l)
    return out


def core_fast_start():
    """Logon fast path: if rh-core exists and nothing is on 2080, start it now; no checks, no waiting."""
    if os.path.exists(CORE) and not stopped() and not router_only() and not core_running() and not port_open(2080):
        if boot_task_exists() and run(['schtasks', '/run', '/tn', BOOT_TASK], timeout=20).returncode == 0:
            return True
        core_direct(); return True
    return False


def pc_push():
    """pc-config.json -> the running PC tunnel (rh-core restart, about 2 s). Nothing in router-only mode."""
    if router_only():
        return True
    if not core_mode():
        say('rh-core.exe is missing; run the setup again'); return False
    return core_restart()


pc_apply = lambda background=False: pc_push()


def deploy_files():
    """Deploy the configs: router-config.json -> router over ssh (push), pc-config.json -> the PC tunnel (pc_push).
    Returns (router ok, router output)."""
    out = push(); say(out)
    pc_push()
    return 'sing-box service: running' in out or pc_only(), out


def pending_deploy():
    """True when the running PC tunnel has a different node set than pc-config.json (None if it cannot tell)."""
    try:
        return set_mark(nodes_of(PC_CFG)) not in json.loads(pc_get('/proxies', 3))['proxies']
    except Exception:
        return None


def deploy(best, tested=None):
    """Write the node set everywhere, push it to the router, restart the PC tunnel on it. True if the router took it."""
    write_configs(best)
    write_txt(tested or best, best)
    say(f'wrote {len(best)} nodes to both configs; deploying...')
    return deploy_files()[0]


def find_spares(top, exclude):
    """Working nodes from the feed that are not in use, best tunnel latency first. Also returns everything tested."""
    c = feed(); say(f'feed: {len(c)} candidates with plausible fronts')
    live = probe([n for n in c if n['server'] not in exclude])
    say(f'answering on 443: {len(live)}, handshake-testing the {min(top, len(live))} fastest on this PC...')
    cand = router_test(live[:top])
    return sorted([n for n in cand if n['ok']], key=lambda n: n['ms']), cand


def scan(top=None):
    """The full ranking: the nodes in use plus the fastest candidates, all handshake-tested."""
    top = top or TOP
    cur = inuse(); used = {n['server'] for n in cur}
    for n in cur:
        n['inuse'] = True
    c = [n for n in feed() if n['server'] not in used]; say(f'feed: {len(c)} candidates with plausible fronts')
    live = [n for n in probe(cur + c) if not n.get('inuse')]; say(f'answering on 443: {len(live)}, handshake-testing the {top} fastest plus the {len(cur)} in use...')
    nodes = cur + [n for n in live if n['server'] not in used][:top]
    router_test(nodes); table(nodes)
    say(f"\n{sum(n['ok'] for n in nodes)} of {len(nodes)} pass")
    return nodes


def vault_spares(top, exclude):
    """Working nodes from the vault (nodes that passed a test before), tested again now, best tunnel latency first."""
    test = vault_nodes(exclude)[:top]
    if test:
        say(f'testing {len(test)} nodes that worked before (vault)...'); router_test(test)
    return sorted([n for n in test if n['ok']], key=lambda n: n['ms']), test


def replace(slots, top=None, vault=False):
    """Swap the in-use nodes at the given slot indexes (0-based) for the best tested spares, from the online node list
    or (vault=True) from the nodes that worked before, then deploy."""
    top = top or TOP
    if router_up():
        s = router_sync()
        if s:   # the slots now hold other nodes: what was picked no longer applies
            raise RuntimeError(s + ' Nothing replaced; check again and pick the node to fix.')
    cur = inuse(); ex = {n['server'] for n in cur}
    try:
        spares, tested = (vault_spares if vault else find_spares)(top, ex)
    except Exception as e:
        if vault:
            raise
        say(f'node list unavailable ({e}); trying the nodes that worked before (vault)')
        spares, tested, vault = *vault_spares(top, ex), True
    if not spares and not vault:
        say('no working spare in the node list; trying the nodes that worked before (vault)')
        spares, tested, vault = *vault_spares(top, ex), True
    if not spares:
        raise RuntimeError(f"no working spare found {'among the nodes that worked before' if vault else 'in the online node list'}; "
                           'nothing changed')
    if len(spares) < len(slots):
        say(f'only {len(spares)} working spare(s) for {len(slots)} slot(s); replacing what can be replaced')
    swaps = []
    for i, s in zip(slots, spares):
        say(f"vless-{i + 1}: {cur[i]['server']} -> {s['server']}  ({s['ms']}ms, {s.get('where', '?')})")
        swaps.append((f'vless-{i + 1}', cur[i]['server'], s)); cur[i] = s
    kept = [n for n in cur if n.get('ms') is None]
    if kept and (os.path.exists(CORE) or router_only()):
        node_test(kept)   # same test as the spares got, so the order below compares like with like
    for n in geolocate(cur):
        n['ok'] = True
    cur.sort(key=lambda n: n.get('ms') or 9e9)   # fastest first: vless-1 is the lowest ping
    say('slots ordered fastest first: ' + ', '.join(f"vless-{i + 1} {n['server']} {n.get('ms') or '?'} ms" for i, n in enumerate(cur)))
    deploy(cur, cur + [t for t in tested if t not in cur])
    return swaps


def swap_files(pairs):
    """Configs only, nothing deployed: each (slot index, node) goes into both configs. Returns [(tag, old server, node)]."""
    cur, meta, out = inuse(), known_where(full=True), []
    for i, node in pairs:
        if i < len(cur):
            out.append((f'vless-{i + 1}', cur[i]['server'], node)); cur[i] = node
        else:   # a set of fewer than 6: the node fills the next free slot
            out.append((f'vless-{len(cur) + 1}', '(empty)', node)); cur.append(node)
    for n in cur:
        n.setdefault('ok', True); n.setdefault('ms', n.get('phone_ms'))
        for k, v in meta.get(n['server'], {}).items():
            n.setdefault(k, v)
    write_configs(cur); write_txt(cur, cur)
    say('configs updated' + ''.join(f'; {t}: {o} -> {n["server"]}' for t, o, n in out))
    return out


def fastest_local(k, exclude):
    """Feed candidates not in use and not known-failed, ranked by 443 ping from this PC only. Not tunnel-tested."""
    where, skip = known_where(), exclude | known_failed()
    live = probe([n for n in feed() if n['server'] not in skip])[:k]
    for n in live:
        n['where'] = where.get(n['server'], '?')
    return live


def replace_local(slots):
    """Files only, no router: the fastest 443-ping candidates go into the slots. Deploy later with deploy_files()."""
    cands = fastest_local(len(slots), {n['server'] for n in inuse()})
    if not cands:
        raise RuntimeError('no candidate answers on 443 from this PC; nothing changed')
    return swap_files(list(zip(slots, cands)))


def put(slot, node, deploy_now=True):
    """One chosen node into one slot (0-based) of both configs; deploys unless deploy_now is False."""
    sw = swap_files([(slot, node)])[0]
    if deploy_now:
        deploy_files()
    return sw


def apply(nodes, k):
    best = sorted([n for n in nodes if n['ok']], key=lambda n: n['ms'])[:k]
    if len(best) < 2:
        raise RuntimeError('fewer than 2 passing nodes, not applying')
    deploy(best, nodes)


def check_all(nodes):
    """Both live checks; returns (phone status, pc status)."""
    geolocate(nodes)
    return check_phone(nodes), check_pc(nodes)


def status_table(nodes, ph, pc):
    fmt = lambda ms, why: f'{ms}ms' if ms is not None else f'FAIL {why}'
    say(f"\n{'slot':8} {'server':16} {'front':20} {'where':24} {'phone':>10}  {'PC':>10}")
    for n in nodes:
        say(f"{n['tag']:8} {n['server']:16} {n['sni']:20} {n.get('where', '?'):24} "
            f"{fmt(n['phone_ms'], n['phone_why']):>10}  {fmt(n['pc_ms'], n['pc_why']):>10}")
    say(f"\nrouter: {'service running' if ph['service'] else 'SERVICE NOT RUNNING'}, active {ph['now'] or '?'}")
    say(f"PC: active {pc['now'] or '?'}, exit {pc['exit'] or '?'}" if pc else
        'PC: control API not reachable; rh-core is not running the current config yet (restart the PC tunnel)')


# ---- background: one repair round every few minutes, from the watcher (no window) or the window's auto-check
def port_open(port):
    s = socket.socket(); s.settimeout(0.5)
    try:
        s.connect(('127.0.0.1', port)); return True
    except OSError:
        return False
    finally:
        s.close()


PIACTL = r'C:\Program Files\Private Internet Access\piactl.exe'
FULL_EVERY = 3600       # full check of every node, hourly
SLOW = 200              # a working node slower than this (ms) shows amber in the tray ring (settings)
TICK = 30 * 60           # how often the watcher wakes up; between wake-ups it does nothing at all


def pia_state():
    """PIA's own connection state (Connected, Reconnecting, ...). Asks the PIA app only; sends nothing through the tunnel.
    Other VPN apps: no state feed, the hourly check covers them."""
    try:
        return run([PIACTL, 'get', 'connectionstate'], timeout=15).stdout.strip()
    except Exception:
        return ''


def wait_api(limit):
    """Wait (up to limit seconds) until the PC tunnel's control API answers; True when it does."""
    t = time.time()
    while time.time() - t < limit:
        try:
            pc_get('/proxies', 3); return True
        except Exception:
            time.sleep(2)
    return False


def port_taken(port):
    """True when another process holds this loopback port (the watcher's and the window's "running" locks).
    A bind test: no connection is made, so nothing piles up in the lock's queue."""
    s = socket.socket()
    try:
        s.bind(('127.0.0.1', port)); return False
    except OSError:
        return True
    finally:
        s.close()


def watch_cycle(st, background=True):
    """Check the nodes in use and fix what is broken right away: a dead node is re-checked after 30 s and replaced,
    every node dead -> rescue. The router's live tunnel is the judge when it answers, else the PC tunnel.
    Returns a sentence when something changed, else None."""
    if stopped():
        say('Reality Hop is stopped; no check'); return None
    if core_mode():
        core_ensure()
    cur = inuse()
    if not cur:   # nothing in use yet (a setup that could not finish): find the first set
        nodes = scan(); apply(nodes, 6); out = 'No nodes were in use; the 6 best from a fresh scan are in now.'
        say(out); notify(out); return out

    def test():
        if side == 'router':
            check_phone(cur); return {n['tag']: n['phone_ms'] for n in cur}
        check_pc(cur); return {n['tag']: n['pc_ms'] for n in cur}
    if router_up():
        healed = router_sync()
        if healed:
            say(healed); notify(healed); hist_add(event=('heal', healed)); return healed
        try:
            ssh('date +%s > /tmp/pc-alive')   # the router's self-repair stands down while this PC looks after the nodes
        except Exception:
            pass
        if ssh('[ -d /tmp/heal.lock ] && echo busy').strip() == 'busy':
            say('router self-repair is running; leaving it to finish'); return None
    r = check_pc(cur)
    if r is not None and not r.get('stale'):
        side = 'PC'           # this PC's own tunnel is the judge
    elif router_up():
        side = 'router'
    else:
        say('the PC tunnel cannot test the current set and the router is unreachable; nothing to judge'); return None
    res = test(); dead = [t for t, ms in res.items() if ms is None]
    say(f"{side} check: {len(res) - len(dead)} of {len(res)} nodes working" + (f"; dead: {', '.join(dead)}" if dead else ''))
    st['last'] = ('bad' if len(dead) == len(res) else 'warn' if dead else 'ok', f'{len(res) - len(dead)} of {len(res)} nodes working',
                  time.strftime('%H:%M'))
    st['segs'] = ['bad' if ms is None else 'warn' if ms > SLOW else 'ok' for ms in res.values()]
    if dead:
        say('re-checking in 30 s so one dropped probe does not cost a node...'); time.sleep(30)
        res = test(); dead = [t for t, ms in res.items() if ms is None]
    if dead and (os.path.exists(CORE) or router_only()):   # second opinion: a fresh test instance, not the running tunnel
        again = [dict(n) for n in cur if n['tag'] in dead]; node_test(again)
        fine = [n['tag'] for n in again if n['ok']]
        if fine:
            say(f"independent test: {', '.join(fine)} work after all; not replacing them")
            dead = [t for t in dead if t not in fine]
            for n in again:
                if n['ok']:
                    res[n['tag']] = n['ms']
    st['last'] = ('bad' if len(dead) == len(res) else 'warn' if dead else 'ok', f'{len(res) - len(dead)} of {len(res)} nodes working',
                  time.strftime('%H:%M'))
    st['segs'] = ['bad' if t in dead else 'warn' if (ms or 0) > SLOW else 'ok' for t, ms in res.items()]
    hist_add(pings={n['server']: (None if n['tag'] in dead else res.get(n['tag'])) for n in cur})
    out = None
    if dead and len(dead) == len(res):
        out = rescue()
    elif dead:
        swaps = replace([i for i, n in enumerate(cur) if n['tag'] in dead])
        out = 'Replaced ' + ', '.join(f'{t} ({o} -> {n["server"]})' for t, o, n in swaps) + '.'
    if out:
        say(out); notify(out); hist_add(event=('replace' if out.startswith('Replaced') else 'rescue', out))
    else:
        regroup()
    if os.path.exists(CORE) and time.time() - st.get('vault_t', 0) > 6 * 3600:   # keep the vault fresh
        st['vault_t'] = time.time(); v = vault_nodes({n['server'] for n in cur})[:TOP]
        if v:
            router_test(v); say(f"vault refresh: {sum(n['ok'] for n in v)} of {len(v)} still work")
    return out


def gui_open():
    """Start the Reality Hop window (or do nothing if it is already open)."""
    if not port_taken(GUI_PORT):
        cmd = [exe()] if FROZEN else [pythonw(), os.path.join(HERE, 'realityhop.pyw')]
        subprocess.Popen(cmd, cwd=DATA, env=own_env(), creationflags=subprocess.DETACHED_PROCESS)


_procs_feed = []
KILL_FEEDS = ['powershell', '-NoProfile', '-Command', "Get-CimInstance Win32_Process -Filter \"Name='piactl.exe'\" | Where-Object { $_.CommandLine -like '*monitor connectionstate*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }"]   # stray PIA monitors left by a watcher that was stopped


def pia_feed(on_change):
    """PIA's own state feed (piactl monitor): one line per change, no polling and no network traffic.
    Restarts itself if PIA is not running yet."""
    if not os.path.exists(PIACTL):
        return   # another VPN app: no state feed, the hourly check covers it
    def loop():
        while True:
            try:
                p = subprocess.Popen([PIACTL, 'monitor', 'connectionstate'], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                     stdin=subprocess.DEVNULL, text=True, **NOWIN)
                _procs_feed[:] = [p]
                for line in p.stdout:
                    if line.strip():
                        on_change(line.strip())
            except Exception:
                pass
            time.sleep(20)
    threading.Thread(target=loop, daemon=True).start()


def watch():
    """The background watcher (--watch, started at logon) with its tray icon. Pauses its checks while the
    window is open. Node check every hour (settings). PIA's state feed wakes it the moment PIA drops: still not
    connected a minute later -> the nodes are checked and fixed at once. Daily rh-core beta update.
    Closing the window does not stop it; Quit in the tray menu does."""
    global say
    lock = socket.socket()
    for i in range(10):   # after an update the old watcher may still be exiting
        try:
            lock.bind(('127.0.0.1', WATCH_PORT)); break
        except OSError:
            if i == 9:
                print('the watcher is already running'); return
            time.sleep(2)
    if stopped():
        return   # stopped from the window: stay off until Start
    lnk_migrate(STARTUP_LNK); api_lockdown()
    started = core_fast_start()   # before anything else, so PIA finds the proxy when it connects
    try:
        run(KILL_FEEDS, timeout=30)
    except Exception:
        pass
    say = file_log('watch'); started_at = time.time()
    say('router-only mode: no tunnel on this PC' if router_only() else 'rh-core started at logon' if started else 'rh-core already running')
    settings_load()
    say(f'watcher started: node check every {FULL_EVERY // 60} min, PIA state feed {"on" if os.path.exists(PIACTL) else "not available (PIA not installed)"}, {"core" if core_mode() else "NO rh-core"} mode')
    st, last_full, last_fix = {}, 0, 0   # first node check as soon as the tunnel answers
    last_app = time.time() - 24 * 3600 + 900   # app update check 15 min after logon, then daily
    try:   # the last known state at once, so the tray icon is not grey while the first check runs
        t = json.load(open(TRAY_LAST)); st['last'], st['segs'] = tuple(t['last']), t.get('segs') or ()
    except (OSError, ValueError, KeyError, TypeError):
        pass
    wake, want, pia = threading.Event(), set(), {'state': '', 'bad_since': None, 'drops': []}

    def ask(what):
        want.add(what); wake.set()

    def on_pia(state):
        prev = pia['state']; pia['state'] = state
        if state in ('Connected', 'Disconnected'):
            pia['bad_since'] = None
        elif pia['bad_since'] is None:
            pia['bad_since'] = time.time()   # PIA passes through several "connecting" states; count from the first
        if prev == 'Connected' and state != 'Connected':
            if not pia['drops'] or time.time() - pia['drops'][-1] > 30:   # PIA often blips twice in a row: one drop
                pia['drops'].append(time.time()); hist_add(event=('drop', f'PIA {state.lower()}'))
            say(f'PIA dropped ({state})'); wake.set()
        elif state == 'Connected' and prev and prev != 'Connected':
            hist_add(event=('back', 'PIA connected')); say('PIA connected again')
        show()

    def quit_():
        say('watcher stopped from the tray menu'); tray.remove(); os.makedirs(DATA, exist_ok=True)
        for p in _procs_feed:
            p.kill()
        os._exit(0)
    tray = Tray({1: gui_open, 2: lambda: ask('check'), 4: lambda: ask('restart'), 3: lambda: ask('rescue'), 9: quit_})

    def show():
        state, text, at = st.get('last', ('unk', 'not checked yet', ''))
        pia['drops'] = [t for t in pia['drops'] if t > time.time() - 3600]
        drops = len(pia['drops'])
        if 'last' in st:
            jsave(TRAY_LAST, {'last': list(st['last']), 'segs': list(st.get('segs', ()))})
        tray.update(state, f"Reality Hop  ·  {text}" + (f'  ·  {at}' if at else '')
                    + (f"  ·  PIA dropped {drops}x in the last hour" if drops else '')
                    + ('' if core_mode() else '  ·  rh-core missing'), st.get('segs', ()))
    pia_feed(on_pia); show()
    threading.Timer(15, tray_promote).start()   # Windows lists the icon a moment after it appears
    wait_api(90)
    while True:
        CANCEL.clear(); now = time.time(); settings_load()
        try:
            if core_update_due() and now - started_at > 600:   # sing-box: daily, 10 min after logon at the earliest
                set_this_pc(core_checked=now)
                try:
                    core_update()
                except Exception as e:   # no internet, GitHub down...: try again tomorrow, the nodes are not the problem
                    say(f'rh-core update check failed: {e}')
            if FROZEN and now - last_app > 24 * 3600:
                last_app = now
                try:
                    import install
                    rel = install.check_update()
                    if rel and settings_load()['auto_update'] and not port_taken(GUI_PORT):
                        say(f"installing Reality Hop {rel['ver']}"); install.apply_update(rel)
                        tray.remove(); lock.close(); install.start_watcher(); os._exit(0)
                    elif rel:
                        notify(f"Reality Hop {rel['ver']} is available; open Reality Hop to install it.")
                except Exception as e:
                    say(f'app update check failed: {e}')
            pia_bad = pia['bad_since'] is not None
            if 'restart' in want:
                want.discard('restart'); say('restart requested from the tray')
                ok = core_restart(); hist_add(event=('restart', 'PC tunnel restarted from the tray'))
                notify('PC tunnel restarted.' if ok else 'PC tunnel did not come back; open Reality Hop.')
            elif 'rescue' in want:
                want.discard('rescue'); out = rescue(); say(out); notify(out); hist_add(event=('rescue', out)); last_full = now
            elif 'check' in want:
                want.discard('check'); last_full = now; watch_cycle(st)
            elif pia_bad and now - pia['bad_since'] >= 60 and now - last_fix > 300:
                say(f'PIA still "{pia["state"]}" after a minute; checking the nodes now')
                last_fix = last_full = now
                if not watch_cycle(st) and core_mode():
                    say('every node checks fine; restarting the PC tunnel'); core_restart()
                    hist_add(event=('restart', 'PC tunnel restarted because PIA could not reconnect'))
            elif port_taken(GUI_PORT):
                last_full = now   # the window does the checks while it is open
            elif now - last_full > FULL_EVERY:
                last_full = now; watch_cycle(st)
            elif core_mode():
                core_ensure()
            core_log_trim()
        except Exception as e:
            say(f'error: {e}'); st['last'] = ('bad', f'error: {e}'[:80], time.strftime('%H:%M'))
        show()
        # sleep until the next check is due, a tray request, or a PIA change (re-checked a minute after a drop)
        due = 60 if pia['bad_since'] is not None else max(30, min(TICK, last_full + FULL_EVERY - time.time()))
        wake.wait(due); wake.clear()


PIA_SETTINGS = r'C:\Program Files\Private Internet Access\data\settings.json'
PIA_HOW = ('PIA > Settings > Protocols: OpenVPN, Transport TCP, Remote Port 443. Then Settings > Multi-Hop: tick '
           '"Multi-hop and Obfuscation", choose SOCKS5 Proxy, EDIT: host 127.0.0.1, port 2080, no username or password')


def is_setup():
    return bool(this_pc().get('setup_done'))


def set_this_pc(**kw):
    d = this_pc(); d.update(kw); jsave(THIS_PC, d)


def prepare():
    """Data folder, the two configs from the templates (each with a fresh random API secret), sing-box from the
    official releases. Leaves anything that already exists alone."""
    for d in (PC_DIR, ROUTER_DIR, LOGS):
        os.makedirs(d, exist_ok=True)
    for name, dst in (('pc-config.json', PC_CFG), ('router-config.json', ROUTER_CFG)):
        if not os.path.exists(dst):
            c = json.load(open(os.path.join(RES, name), encoding='utf-8'))
            c['experimental']['clash_api']['secret'] = secrets.token_hex(16)
            if dst == PC_CFG:
                c['log']['output'] = CORE_LOG
            json.dump(c, open(dst, 'w', encoding='utf-8'), indent=2)
    for d, text in ((PC_DIR, PC_README), (ROUTER_DIR, ROUTER_README)):
        open(os.path.join(d, 'README.txt'), 'w', encoding='utf-8').write(text)
    if not os.path.exists(CORE):
        core_update(force=True)
    api_lockdown()


API_ORIGIN = ['http://127.0.0.1:9090']   # web pages allowed to call the PC tunnel's control API: none


def api_lockdown():
    """Configs made before 1.1.0 let a script on any web page reach the control API (it still needed the secret).
    Allow no web page at all. rh-core checks the change first; it applies from the next tunnel restart."""
    try:
        c = json.load(open(PC_CFG)); api = c['experimental']['clash_api']
        if api.get('access_control_allow_origin') == API_ORIGIN or not os.path.exists(CORE):
            return
        api.update(access_control_allow_origin=API_ORIGIN, access_control_allow_private_network=False)
        tmp = PC_CFG + '.check.json'; jsave(tmp, c, indent=2)
        if run([CORE, 'check', '-c', tmp], timeout=30, cwd=os.path.dirname(CORE)).returncode == 0:
            os.replace(tmp, PC_CFG); say('control API: closed to web pages (from the next tunnel restart)')
        else:
            os.remove(tmp)
    except (OSError, ValueError, KeyError, RuntimeError):
        pass


PC_README = '''Reality Hop: PC tunnel files

pc-config.json   the sing-box config of the PC tunnel: the 6 nodes (vless-1 to vless-6), the SOCKS5 proxy on
                 127.0.0.1:2080, DNS (Quad9 through the tunnel) and the local control API on 127.0.0.1:9090
rh-core.exe      sing-box from its official releases (github.com/SagerNet/sing-box), under Reality Hop's own name

You can read and edit pc-config.json; press Restart tunnel in the Reality Hop window to apply it. Reality Hop rewrites
the vless-1 to vless-6 entries whenever it replaces nodes. The logs are in ..\\logs.
Do not share pc-config.json: it holds node keys and the local API secret. For a problem report use
Report a problem in the window; it takes everything personal out.
'''
ROUTER_README = '''Reality Hop: router tunnel files (GL.iNet, only used with router features on)

router-config.json   the sing-box config that Push to router sends to the router (/etc/sing-box/config.json)
nft-devices.sh       the firewall rule sent to the router: it diverts only the chosen devices into sing-box
heal.sh              the router's self-repair, run there by cron every 30 minutes

Press Push to router in the Reality Hop window to apply changes. Do not share router-config.json: it holds node keys
and the router's local API secret.
'''


def vpn_check():
    """One line about the VPN app: PIA is read from its own settings file (OpenVPN over TCP 443 through SOCKS5
    127.0.0.1:2080); other apps cannot be checked."""
    try:
        st = json.load(open(PIA_SETTINGS))
    except (OSError, ValueError):
        return ('VPN app: PIA not found. Point your VPN app at SOCKS5 127.0.0.1:2080 and use OpenVPN over TCP port 443 '
                '(see the VPN step in the setup)')
    pc, wrong = st.get('proxyCustom') or {}, []
    if st.get('method') != 'openvpn':
        wrong.append('protocol is not OpenVPN')
    if st.get('protocol') != 'tcp':
        wrong.append('transport is not TCP')
    if st.get('remotePortTCP') != 443:
        wrong.append('remote port is not 443')
    if not (st.get('proxyType') == 'custom' and st.get('proxyEnabled', True) and pc.get('host') == '127.0.0.1' and pc.get('port') == 2080):
        wrong.append('the SOCKS5 proxy is not 127.0.0.1:2080')
    if not wrong:
        return 'VPN app: PIA uses OpenVPN over TCP 443 through SOCKS5 127.0.0.1:2080, correct'
    return f"VPN app: SET THIS IN PIA BY HAND ({', '.join(wrong)}): " + PIA_HOW


def setup_pc():
    """The PC part of the setup: configs, sing-box, the first 6 working nodes, the PC tunnel running.
    Returns the list of results, one line per step."""
    res = []
    prepare(); res.append(f'rh-core: sing-box {core_version() or "MISSING"} (official releases, {settings_load()["singbox"]} channel, updated daily)')
    if not nodes_of(PC_CFG):
        say('finding the first nodes: downloading the public node list and testing the fastest candidates on this PC...')
        nodes = scan(); best = sorted([n for n in nodes if n['ok']], key=lambda n: n['ms'])[:6]
        if len(best) < 2:
            raise RuntimeError('fewer than 2 working nodes found; check that this PC has internet and press Install again')
        write_configs(best); write_txt(nodes, best)
        res.append(f"nodes: the {len(best)} fastest working ones are in use (best {best[0]['ms']} ms)")
    else:
        res.append(f'nodes: {len(nodes_of(PC_CFG))} already in use, kept')
    owner = None if router_only() else port_blocker()
    if router_only():
        res.append('PC tunnel: off (router-only mode: the router tunnels your devices)')
    elif owner:
        res.append('PC tunnel: NOT started. ' + blocker_hint(owner))
    else:
        res.append('PC tunnel: running on 127.0.0.1:2080' if core_restart() else 'PC tunnel: rh-core did not come up, see logs/core.log')
    if not router_only():
        res.append(vpn_check())
    for l in res:
        say(l)
    return res


def router_info():
    """Model, firmware, sing-box version and helper tools on the router (over ssh)."""
    out = ssh('cat /tmp/sysinfo/model 2>/dev/null; echo "|"; cat /etc/glversion 2>/dev/null; echo "|"; '
              'sing-box version 2>/dev/null | head -1; echo "|"; for t in jq curl; do command -v $t >/dev/null && echo $t; done', timeout=30)
    model, fw, sb, tools = (out.split('|') + ['', '', '', ''])[:4]
    m = re.search(r'(\d+)\.(\d+)\.(\d+)', sb)
    return {'model': model.strip() or '?', 'firmware': fw.strip() or '?', 'singbox': m.group(0) if m else None,
            'too_new': bool(m) and (int(m.group(1)), int(m.group(2))) >= (1, 13), 'tools': set(tools.split())}


def router_install():
    """Install sing-box, jq and curl from the router's package feed (GL.iNet: same as APPLICATIONS > Plug-ins)."""
    return ssh('opkg update >/dev/null 2>&1; opkg install sing-box jq curl 2>&1 | tail -4; sing-box version 2>/dev/null | head -1',
               timeout=600).strip()


def router_clients():
    """[(ip, name)] from the router's DHCP leases, for picking devices. Only read, nothing kept."""
    out = []
    for l in ssh('cat /tmp/dhcp.leases 2>/dev/null', timeout=30).splitlines():
        p = l.split()
        if len(p) >= 4 and re.fullmatch(r'\d+\.\d+\.\d+\.\d+', p[2]):
            out.append((p[2], p[3] if p[3] != '*' else '(no name)'))
    return sorted(out, key=lambda x: tuple(int(k) for k in x[0].split('.')))


def router_off():
    """Turn the router tunnel off: sing-box disabled, the device diversion removed, the self-repair cron line gone."""
    return ssh("uci set sing-box.main.enabled=0; uci commit sing-box; /etc/init.d/sing-box stop; "
               "sed -i '/heal.sh/d' /etc/crontabs/root; /etc/init.d/cron restart; fw4 reload >/dev/null 2>&1; "
               "echo 'router tunnel: off'", timeout=60).strip()


def make_key():
    """An SSH key for the router, made with Windows' own ssh-keygen. Returns the public key line."""
    if not os.path.exists(ROUTER_KEY):
        os.makedirs(DATA, exist_ok=True)
        r = run([ssh_exe('ssh-keygen'), '-q', '-t', 'ed25519', '-N', '', '-C', 'realityhop', '-f', ROUTER_KEY], timeout=30)
        if r.returncode:
            raise RuntimeError('ssh-keygen failed: ' + (r.stderr or r.stdout).strip()[:200])
    return open(ROUTER_KEY + '.pub').read().strip()


def add_key_console(host):
    """Opens a console that adds this PC's key to the router. The person types the router password into ssh there;
    Reality Hop never sees it."""
    pub, kh = make_key(), os.path.join(DATA, 'known_hosts')
    k = pub.split()[1]
    remote = (f"mkdir -p /etc/dropbear; touch /etc/dropbear/authorized_keys; grep -q {k} /etc/dropbear/authorized_keys "
              f"|| echo {pub} >> /etc/dropbear/authorized_keys; chmod 600 /etc/dropbear/authorized_keys; echo; echo Key added. Close this window.")
    bat, esc = os.path.join(DATA, 'add-router-key.cmd'), lambda p: p.replace('%', '%%')   # % is special in a .cmd
    open(bat, 'w', encoding='utf-8').write('@echo off\r\nchcp 65001 >nul\r\ntitle Reality Hop: add key to router\r\n'
                         'echo Type the router admin password (the one for the GL.iNet web panel) and press Enter.\r\n'
                         'echo Nothing shows while you type. Reality Hop never sees it.\r\necho.\r\n'
                         f'"{esc(ssh_exe())}" -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile="{esc(kh)}" root@{host} "{remote}"\r\n'
                         'echo.\r\npause\r\n')
    subprocess.Popen(['cmd', '/c', bat], creationflags=subprocess.CREATE_NEW_CONSOLE)


def watcher_kill():
    """Stop every running watcher (and the PIA monitors it started)."""
    run(['powershell', '-NoProfile', '-Command',
         "Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -like '*--watch*' -and "
         f"($_.CommandLine -like '*engine.py*' -or $_.Name -eq 'RealityHop.exe') -and $_.ProcessId -ne {os.getpid()} }} | "
         "ForEach-Object { Stop-Process -Id $_.ProcessId -Force }"], timeout=30)
    run(KILL_FEEDS, timeout=30)


def startup(on):
    """Start the watcher at logon (a shortcut in the Startup folder, no admin needed); also starts or stops it now.
    It runs on the newest installed Python (or RealityHop.exe when frozen)."""
    cmd = self_cmd('--watch')
    if on:
        os.makedirs(DATA, exist_ok=True); lnk_migrate(STARTUP_LNK)
        args = ' '.join(f'"{a}"' if ' ' in a else a for a in cmd[1:])
        ps = (f"$s = (New-Object -ComObject WScript.Shell).CreateShortcut({psq(STARTUP_LNK)}); $s.TargetPath = {psq(cmd[0])}; "
              f"$s.Arguments = {psq(args)}; $s.WorkingDirectory = {psq(DATA)}; $s.Save()")
        run(['powershell', '-NoProfile', '-Command', ps], timeout=30)
        if not port_taken(WATCH_PORT):
            subprocess.Popen(cmd, cwd=DATA, env=own_env(), creationflags=subprocess.DETACHED_PROCESS)
        say('background watcher: on, starts at every logon')
    else:
        lnk_migrate(STARTUP_LNK)
        if os.path.exists(STARTUP_LNK):
            os.remove(STARTUP_LNK)
        watcher_kill()
        say('background watcher: off')


def lnk_migrate(path):
    """Up to 1.0.4 the shortcuts were called RealityHop.lnk; now they carry the name people see (Windows search too)."""
    old = os.path.join(os.path.dirname(path), APP + '.lnk')
    try:
        if os.path.exists(old):
            os.remove(old) if os.path.exists(path) else os.replace(old, path)
    except OSError:
        pass


# ---- tray icon for the watcher (Windows notification area), plain Win32 through ctypes, no extra packages
def tray_icon_file(state, segs=()):
    """Tray icon: the center shows the overall state, the outer ring one segment per node (green working,
    amber slow, red dead, grey not checked yet). Drawn at 256 px and scaled down; cached in the data folder."""
    from PIL import Image, ImageDraw
    col = {'ok': (95, 211, 141), 'warn': (245, 165, 36), 'bad': (255, 77, 79)}
    segs = list(segs) or ['unk'] * 6
    path = os.path.join(DATA, f"tray-{state}-{''.join(x[0] for x in segs)}.ico")
    if not os.path.exists(path):
        os.makedirs(os.path.dirname(path), exist_ok=True)
        im = Image.new('RGBA', (256, 256), (0, 0, 0, 0)); d = ImageDraw.Draw(im)
        d.ellipse((6, 6, 250, 250), fill=(18, 18, 22, 255))
        step, gap = 360 / len(segs), 10 if len(segs) > 1 else 0
        for i, sg in enumerate(segs):
            a = -90 + i * step + gap / 2
            d.arc((14, 14, 242, 242), a, a + step - gap, fill=col.get(sg, (70, 70, 80)) + (255,), width=34)
        d.ellipse((84, 84, 172, 172), fill=col.get(state, (120, 120, 130)) + (255,))
        im.save(path, sizes=[(16, 16), (20, 20), (24, 24), (32, 32), (48, 48), (64, 64)])
    return path


def tray_promote():
    """Windows 11 puts a new tray icon in the hidden overflow (^). Show Reality Hop's on the taskbar once, unless the
    person already chose (Windows Settings > Personalization > Taskbar > Other system tray icons); their choice wins."""
    try:
        import winreg
        tail = '\\' + '\\'.join(os.path.normcase(sys.executable).split(os.sep)[-2:])   # \\<install folder>\\realityhop.exe
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r'Control Panel\NotifyIconSettings') as root:
            for i in range(winreg.QueryInfoKey(root)[0]):
                with winreg.OpenKey(root, winreg.EnumKey(root, i), 0, winreg.KEY_READ | winreg.KEY_SET_VALUE) as k:
                    try:
                        if not os.path.normcase(winreg.QueryValueEx(k, 'ExecutablePath')[0]).endswith(tail):
                            continue
                    except OSError:
                        continue
                    try:
                        winreg.QueryValueEx(k, 'IsPromoted')
                    except OSError:
                        winreg.SetValueEx(k, 'IsPromoted', 0, winreg.REG_DWORD, 1); say('tray icon: shown on the taskbar')
    except OSError:
        pass


class Tray:
    """Notification-area icon. Double-click opens Reality Hop; right-click menu: open, restart, check now, rescue, quit."""
    def __init__(self, actions):
        self.actions, self.hwnd, self.state, self.tip, self.segs = actions, None, 'unk', 'Reality Hop', ()
        threading.Thread(target=self.loop, daemon=True).start()

    def loop(self):
        import ctypes
        from ctypes import wintypes as W
        u32, s32, k32 = ctypes.windll.user32, ctypes.windll.shell32, ctypes.windll.kernel32
        LRESULT = ctypes.c_ssize_t
        WNDPROC = ctypes.WINFUNCTYPE(LRESULT, W.HWND, W.UINT, W.WPARAM, W.LPARAM)
        u32.DefWindowProcW.argtypes = [W.HWND, W.UINT, W.WPARAM, W.LPARAM]; u32.DefWindowProcW.restype = LRESULT
        u32.CreateWindowExW.restype = W.HWND
        u32.CreateWindowExW.argtypes = [W.DWORD, W.LPCWSTR, W.LPCWSTR, W.DWORD, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                                        ctypes.c_int, W.HWND, W.HMENU, W.HINSTANCE, W.LPVOID]
        u32.LoadImageW.restype = W.HANDLE
        u32.LoadImageW.argtypes = [W.HINSTANCE, W.LPCWSTR, W.UINT, ctypes.c_int, ctypes.c_int, W.UINT]
        u32.CreatePopupMenu.restype = W.HMENU
        u32.AppendMenuW.argtypes = [W.HMENU, W.UINT, ctypes.c_size_t, W.LPCWSTR]
        u32.TrackPopupMenu.argtypes = [W.HMENU, W.UINT, ctypes.c_int, ctypes.c_int, ctypes.c_int, W.HWND, W.LPVOID]
        u32.DestroyMenu.argtypes = [W.HMENU]

        class WNDCLASSW(ctypes.Structure):
            _fields_ = [('style', W.UINT), ('lpfnWndProc', WNDPROC), ('cbClsExtra', ctypes.c_int), ('cbWndExtra', ctypes.c_int),
                        ('hInstance', W.HINSTANCE), ('hIcon', W.HICON), ('hCursor', W.HANDLE), ('hbrBackground', W.HBRUSH),
                        ('lpszMenuName', W.LPCWSTR), ('lpszClassName', W.LPCWSTR)]

        class GUID(ctypes.Structure):
            _fields_ = [('a', ctypes.c_ulong), ('b', ctypes.c_ushort), ('c', ctypes.c_ushort), ('d', ctypes.c_ubyte * 8)]

        class NID(ctypes.Structure):
            _fields_ = [('cbSize', W.DWORD), ('hWnd', W.HWND), ('uID', W.UINT), ('uFlags', W.UINT), ('uCallbackMessage', W.UINT),
                        ('hIcon', W.HICON), ('szTip', W.WCHAR * 128), ('dwState', W.DWORD), ('dwStateMask', W.DWORD),
                        ('szInfo', W.WCHAR * 256), ('uVersion', W.UINT), ('szInfoTitle', W.WCHAR * 64), ('dwInfoFlags', W.DWORD),
                        ('guidItem', GUID), ('hBalloonIcon', W.HICON)]
        s32.Shell_NotifyIconW.argtypes = [W.DWORD, ctypes.POINTER(NID)]
        CB, MENU = 0x8001, [(1, 'Open Reality Hop'), (4, 'Restart PC tunnel'), (2, 'Check now'), (3, 'Rescue from verified vault'), (0, None), (9, 'Quit background watcher')]
        created = u32.RegisterWindowMessageW('TaskbarCreated')
        self.nid = NID(); self.nid.cbSize = ctypes.sizeof(NID); self.nid.uID = 1; self.nid.uFlags = 0x1 | 0x2 | 0x4; self.nid.uCallbackMessage = CB

        def icon(state, segs):
            return u32.LoadImageW(None, tray_icon_file(state, segs), 1, 0, 0, 0x10 | 0x40)   # IMAGE_ICON, LR_LOADFROMFILE | LR_DEFAULTSIZE

        def menu():
            h = u32.CreatePopupMenu()
            for i, t in MENU:
                u32.AppendMenuW(h, 0x800 if t is None else 0, i, t)   # MF_SEPARATOR / MF_STRING
            pt = W.POINT(); u32.GetCursorPos(ctypes.byref(pt)); u32.SetForegroundWindow(self.hwnd)
            cmd = u32.TrackPopupMenu(h, 0x100 | 0x80, pt.x, pt.y, 0, self.hwnd, None)   # TPM_RETURNCMD | TPM_NONOTIFY
            u32.DestroyMenu(h)
            if cmd in self.actions:
                threading.Thread(target=self.actions[cmd], daemon=True).start()

        def proc(hwnd, msg, wp, lp):
            if msg == CB:
                if lp == 0x203:      # WM_LBUTTONDBLCLK
                    threading.Thread(target=self.actions[1], daemon=True).start()
                elif lp == 0x205:    # WM_RBUTTONUP
                    menu()
                return 0
            if msg == created:       # Explorer restarted: put the icon back
                s32.Shell_NotifyIconW(0, ctypes.byref(self.nid)); return 0
            if msg == 0x8002:        # our own "refresh" message
                self.nid.hIcon = icon(self.state, self.segs); self.nid.szTip = self.tip[:127]
                s32.Shell_NotifyIconW(1, ctypes.byref(self.nid)); return 0
            return u32.DefWindowProcW(hwnd, msg, wp, lp)
        self._proc = WNDPROC(proc)   # keep a reference, or the callback is garbage-collected
        wc = WNDCLASSW(); wc.lpfnWndProc = self._proc; wc.lpszClassName = 'RealityHopTray'; wc.hInstance = k32.GetModuleHandleW(None)
        u32.RegisterClassW(ctypes.byref(wc))
        self.hwnd = u32.CreateWindowExW(0, 'RealityHopTray', 'Reality Hop', 0, 0, 0, 0, 0, None, None, wc.hInstance, None)
        self.nid.hWnd = self.hwnd; self.nid.hIcon = icon(self.state, self.segs); self.nid.szTip = self.tip
        s32.Shell_NotifyIconW(0, ctypes.byref(self.nid))   # NIM_ADD
        self._remove = lambda: s32.Shell_NotifyIconW(2, ctypes.byref(self.nid))
        msg = W.MSG()
        while u32.GetMessageW(ctypes.byref(msg), None, 0, 0) > 0:
            u32.TranslateMessage(ctypes.byref(msg)); u32.DispatchMessageW(ctypes.byref(msg))

    def update(self, state, tip, segs=()):
        self.state, self.tip, self.segs = state, tip, tuple(segs)
        if self.hwnd:
            import ctypes
            ctypes.windll.user32.PostMessageW(self.hwnd, 0x8002, 0, 0)

    def remove(self):
        if getattr(self, '_remove', None):
            self._remove()


settings_load()


if __name__ == '__main__':
    if '--watch' in sys.argv:
        watch(); sys.exit(0)
    a = argparse.ArgumentParser()
    a.add_argument('--check', action='store_true'); a.add_argument('--apply', type=int, metavar='N'); a.add_argument('--top', type=int, default=None)
    args = a.parse_args()
    try:
        if args.check:
            nodes = inuse(); say(f'checking the {len(nodes)} nodes in use through the live tunnels...')
            if pc_only():
                geolocate(nodes); check_pc(nodes)
                for n in nodes:
                    n['phone_ms'], n['phone_why'] = n['pc_ms'], n['pc_why']
                status_table(nodes, {'service': False, 'now': None}, None); sys.exit(0)
            status_table(nodes, *check_all(nodes))
            dead = [n['tag'] for n in nodes if n['phone_ms'] is None]
            if dead:
                say(f"\ndead on the router side: {', '.join(dead)}  ->  replace them in the Reality Hop window")
            sys.exit(0)
        nodes = scan(args.top)
        ok = [n for n in nodes if n['ok']]
        if args.apply:
            apply(nodes, args.apply)
        elif ok and sys.stdin.isatty():
            k = input('apply the best N to both configs and deploy? (number, or Enter to skip): ').strip()
            if k.isdigit() and int(k) > 0:
                apply(nodes, int(k))
    except RuntimeError as e:
        sys.exit(str(e))
