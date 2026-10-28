"""report.py  -  anonymous problem reports.

The three logs (window.log, watch.log, core.log) plus a short summary of the setup, with everything personal replaced by
neutral tokens: addresses, MAC addresses, user and computer names, home folder paths, e-mail addresses, keys, IDs and
secrets. The same address always gets the same token, so a report still shows which node failed when.
Written to the anonymous-logs folder in the install folder. Nothing is sent anywhere: the person reads the file and
attaches it to a GitHub issue themselves.
"""
import getpass, glob, ipaddress, json, os, platform, re, socket, time, urllib.parse
import engine as N

KEEP_IPS = {'0.0.0.0', '9.9.9.9', '149.112.112.112', '1.1.1.1', '8.8.8.8'}   # public DNS resolvers say nothing about anyone
KEEP_HOSTS = {'api.ipify.org', 'www.gstatic.com', 'ip-api.com', 'api.github.com', 'github.com', 'raw.githubusercontent.com',
              'objects.githubusercontent.com', 'release-assets.githubusercontent.com', 'cdn.jsdelivr.net',
              'fastly.jsdelivr.net'} | N.FRONTS   # what Reality Hop itself uses
LINES = 400   # per log


class Scrubber:
    """Replaces personal data with tokens: node-N (a public node), lan-N (an address on the local network),
    ip-N (any other internet address), host-N (a website), ipv6-N, mac-N, <user>, <pc>, <email>, <uuid>, <key>, <secret>."""
    def __init__(self):
        self.maps, self.exact = {}, {}
        self.nodes = set()
        for cfg in (N.PC_CFG, N.ROUTER_CFG):
            try:
                if N.secret(cfg):
                    self.exact[N.secret(cfg)] = '<secret>'
                self.nodes |= {n['server'] for n in N.nodes_of(cfg)}
            except (OSError, ValueError, KeyError):
                pass
        try:
            self.nodes |= set(N.vault_load())
        except Exception:
            pass
        home = os.path.expanduser('~')
        self.exact[home] = os.path.join(os.path.dirname(home), '<user>')
        for v, t in ((os.environ.get('USERNAME') or getpass.getuser(), '<user>'), (os.path.basename(home), '<user>'),
                     (os.environ.get('COMPUTERNAME') or socket.gethostname(), '<pc>'), (os.environ.get('USERDOMAIN'), '<pc>')):
            if v and len(v) > 2 and v.lower() not in ('user', 'admin', 'administrator', 'windows'):
                self.exact.setdefault(v, t)

    def tok(self, kind, value):
        m = self.maps.setdefault(kind, {})
        return m.setdefault(value, f'{kind}-{len(m) + 1}')

    def host(self, m):
        h = m.group(0).lower()
        return h if h in KEEP_HOSTS or h.rsplit('.', 1)[1] in ('json', 'py', 'pyw', 'exe', 'log', 'txt', 'sh') else self.tok('host', h)

    def ipv4(self, m):
        ip = m.group(0)
        try:
            a = ipaddress.IPv4Address(ip)
        except ValueError:
            return ip
        if a.is_loopback or ip in KEEP_IPS:
            return ip
        if ip in self.nodes:
            return self.tok('node', ip)
        return self.tok('lan' if a.is_private or a.is_link_local else 'ip', ip)

    def __call__(self, text):
        for v in sorted(self.exact, key=len, reverse=True):
            text = re.sub(r'(?<![A-Za-z0-9])' + re.escape(v) + r'(?![A-Za-z0-9])', self.exact[v].replace('\\', '\\\\'), text, flags=re.I)
        text = re.sub(r'(?m)^[+-]\d{4} (?=\d{4}-\d\d-\d\d)', '', text)   # sing-box's time zone offset tells where you are
        text = re.sub(r'\b(?:[A-Za-z0-9-]+\.)+[A-Za-z]{2,}(?=:\d{1,5}\b)', self.host, text)   # sites the tunnel connected to
        text = re.sub(r'[\w.+-]+@[\w-]+\.[\w.-]+', '<email>', text)
        text = re.sub(r'\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b', '<uuid>', text)
        text = re.sub(r'\b(?:[0-9a-fA-F]{2}[:-]){5}[0-9a-fA-F]{2}\b', lambda m: self.tok('mac', m.group(0).lower()), text)
        text = re.sub(r'\b(?:[0-9a-fA-F]{4}\.){2}[0-9a-fA-F]{4}\b', lambda m: self.tok('mac', m.group(0).lower()), text)   # aabb.ccdd.eeff
        text = re.sub(r'(?<![\w:])(?:[0-9a-fA-F]{0,4}:){2,7}[0-9a-fA-F]{0,4}(?![\w:])',
                      lambda m: self.tok('ipv6', m.group(0)) if m.group(0).count(':') >= 4 or '::' in m.group(0) else m.group(0), text)
        text = re.sub(r'(?<![\d.])\d{1,3}(?:\.\d{1,3}){3}(?!\d|\.\d)', self.ipv4, text)
        text = re.sub(r'\b[A-Za-z0-9_-]{43}\b', '<key>', text)      # REALITY public keys
        text = re.sub(r'\b[0-9a-fA-F]{32,64}\b', '<secret>', text)   # API secrets, hashes
        return text


def tail(path, n=LINES):
    try:
        return open(path, encoding='utf-8', errors='replace').readlines()[-n:]
    except OSError:
        return []


def summary():
    """Plain facts about the setup; nothing personal (the scrubber runs over this too)."""
    s, t = N.settings_load(), N.this_pc()
    try:
        tray = json.load(open(N.TRAY_LAST))['last']
    except Exception:
        tray = None
    lines = [f'Reality Hop {N.VERSION} ({"installed" if N.FROZEN else "from source"})',
             f'Windows: {platform.platform()}',
             f"sing-box on the PC: {N.core_version() or 'missing'} (channel: {s['singbox']})",
             N.vpn_check(),
             f"nodes in use: {len(N.nodes_of(N.PC_CFG)) if os.path.exists(N.PC_CFG) else 0}"
             + (f'; last check: {tray[1]} at {tray[2]} ({tray[0]})' if tray else ''),
             f"settings: check every {s['check_min']} min, slow above {s['slow_ms']} ms, switch margin {s['tolerance_ms']} ms, "
             f"{s['top']} candidates per scan, app auto-update {'on' if s['auto_update'] else 'off'}",
             f"state: {'stopped' if t.get('stopped') else 'running'}, setup {'done' if t.get('setup_done') else 'not finished'}, "
             f"boot start {N.task_state() or 'off'}, background at logon {'on' if os.path.exists(N.STARTUP_LNK) else 'off'}",
             f"router features: {'on' if s['router'] else 'off'}" + (', router only (no PC tunnel)' if N.router_only() else '')
             + ((', every device on the main network' if N.all_devices() else f', {len(N.devices())} device(s) chosen') if s['router'] else '')]
    if s['router']:
        try:
            i = N.router_info()
            lines.append(f"router: {i['model']}, firmware {i['firmware']}, sing-box {i['singbox'] or 'not installed'}")
        except Exception as e:
            lines.append(f'router: not reachable ({e})')
    return lines


def make():
    """Write a new anonymous report and return (path, text). Keeps the last 5 reports."""
    sc = Scrubber()
    out = ['REALITY HOP PROBLEM REPORT (anonymous)', f"made {time.strftime('%Y-%m-%d %H:%M')}", '']
    out += [sc(l) for l in summary()]
    for name in ('window', 'watch', 'core'):
        out += ['', f'==== {name}.log (last {LINES} lines) ====']
        out += [sc(l.rstrip('\n')) for l in tail(os.path.join(N.LOGS, f'{name}.log'))] or ['(empty)']
    out += ['', '==== what the tokens mean ====',
            'node-N = a public node from the node list    lan-N = an address on your own network    ip-N = another internet address',
            'host-N = a website the tunnel connected to    ipv6-N, mac-N = an IPv6 or MAC address',
            '<user>, <pc> = your Windows user and computer name    <key>, <secret>, <uuid> = keys and IDs',
            'The same token always means the same address within this report. Nothing was sent anywhere.']
    text = '\n'.join(out) + '\n'
    os.makedirs(N.REPORTS, exist_ok=True)
    path = os.path.join(N.REPORTS, f"report-{time.strftime('%Y%m%d-%H%M%S')}.txt")
    open(path, 'w', encoding='utf-8').write(text)
    for old in sorted(glob.glob(os.path.join(N.REPORTS, 'report-*.txt')))[:-5]:
        try:
            os.remove(old)
        except OSError:
            pass
    return path, text


def issue_url(path, text):
    """A new GitHub issue with the summary filled in; the person attaches the report file."""
    head = text.split('\n==== ', 1)[0].strip()
    body = ('**What happened**\n\n\n**What you expected**\n\n\n**Summary (anonymous)**\n```\n' + head[:3000] + '\n```\n\n'
            f'**Report:** drag `{os.path.basename(path)}` from the anonymous-logs folder into this box.\n')
    return f'https://github.com/{N.REPO}/issues/new?' + urllib.parse.urlencode({'labels': 'bug', 'title': 'Problem: ', 'body': body})
