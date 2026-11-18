"""parity-dump.py  -  records what the Python code returns, as fixtures for the Rust port (rust/parity/<fn>.json).

Runs pure functions only. HOME, USERPROFILE, APPDATA and LOCALAPPDATA point at an empty temp folder first, so the
import reads and writes nothing of a real install. Windows only (engine.py reads the registry on import).
  python tools/parity-dump.py           write the fixtures
  python tools/parity-dump.py --check   exit 1 if a fixture differs from what the Python code returns now
"""
import contextlib, datetime as _dt, json, ntpath, os, shutil, sys, tempfile, time, types

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, 'rust', 'parity')
SANDBOX = tempfile.mkdtemp(prefix='rh-parity-')
PRIVATE = [x for x in (SANDBOX, os.environ.get('USERNAME'), os.environ.get('USERPROFILE')) if x]   # never in a fixture
for k in ('HOME', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA'):
    os.environ[k] = SANDBOX
sys.path.insert(0, os.path.join(ROOT, 'src'))
import engine as N, install, report, i18n, macos
N.set_paths(SANDBOX)

# feed() on a given node list text instead of a download
def feed_parse(text):
    N._feed[:] = [time.time(), text]
    return N.feed()

# Node list lines for feed_parse: one good line, then one change each
U, K = 'a1b2c3d4-e5f6-7890-abcd-ef1234567890', 'Ab' * 20 + 'c_-'
def vl(host='8.8.8.8', port='443', user=U, q=None, frag='#n'):
    q = q if q is not None else f'security=reality&flow=xtls-rprx-vision&pbk={K}&sid=0a1b&sni=www.apple.com&fp=firefox&type=tcp'
    return f'vless://{user}@{host}:{port}?{q}{frag}'
BASE = f'security=reality&flow=xtls-rprx-vision&pbk={K}&sni=www.apple.com'
FEED = [
    vl(), vl() + '\n' + vl(), vl('1.1.1.1') + '\r\n' + vl('9.9.9.9') + '\r' + vl('8.8.4.4'),
    vl('1.1.1.1') + '\x0b' + vl('9.9.9.9') + '\x1c' + vl('8.8.4.4') + ' ' + vl('4.4.4.4') + '\x85' + vl('4.2.2.2'),
    vl(port='8443'), vl(port='99999'), vl(port='0443'), vl(port=''), vl(port='44 3'), vl(port='٤٤٣'),
    vl('10.0.0.1'), vl('224.0.0.1'), vl('100.64.0.1'), vl('01.1.1.1'), vl('1.1.\t1.1'), vl('[::1]'), vl('[1.1.1.1'),
    vl('١.1.1.1'), vl(user='A1B2C3D4-E5F6-7890-ABCD-EF1234567890'), vl(user=''), vl(user='x' * 36),
    vl(user=U + ':pw'), vl(user='a1b2c3d4%2De5f6-7890-abcd-ef1234567890'), vl(user='é' + U[1:]),
    vl(q=BASE), vl(q=BASE + '&sid='), vl(q=BASE + '&sid=abc'), vl(q=BASE + '&sid=' + '0a' * 8),
    vl(q=BASE + '&sid=' + '0a' * 9), vl(q=BASE + '&fp=unknown'), vl(q=BASE + '&fp=qq'), vl(q=BASE + '&fp='),
    vl(q=BASE.replace('www.apple.com', 'WWW.APPLE.COM')), vl(q=BASE.replace('www.apple.com', 'example.com')),
    vl(q=BASE.replace(K, K[:-1])), vl(q=BASE.replace(K, K[:-1] + '+')), vl(q=BASE.replace(K, '%41' + K[1:])),
    vl(q=BASE.replace('security=reality', 'security=REALITY')), vl(q=BASE.replace('&flow=xtls-rprx-vision', '')),
    vl(q='security=none&' + BASE), vl(q=BASE + '&security=none'), vl(q='junk&&=x&' + BASE),
    vl(q=BASE.replace('sni=', 'sni=%77ww.apple.com&x=')), vl(q=BASE + '&pbk=' + 'z' * 43),
    vl(frag='#reality?vision'), vl(q=BASE.replace('security=reality', 'security=x'), frag='#reality'),
    vl() + '　', '  ' + vl(), vl() + ' \t', 'VLESS://' + vl()[8:], vl().replace('vision', 'Vision'),
    vl().replace('reality', 'Reality'), vl('8.8.8.8') + '\n' + vl('8.8.8.8', q=BASE + '&sid=ff'), '', 'garbage\nmore',
]

# A file's text in the sandbox data folder, or None
def text(path):
    return open(path, encoding='utf-8').read() if os.path.exists(path) else None

# Write JSON into the sandbox the way the engine's own files look (indent 2)
def put(path, data, indent=2):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, 'w', encoding='utf-8') as f:
        json.dump(data, f, indent=indent)

# The two templates as prepare() makes them, with a fixed secret
def templates():
    out = []
    for name in ('pc-config.json', 'router-config.json'):
        c = json.load(open(os.path.join(ROOT, 'src', 'resources', name), encoding='utf-8'))
        c['experimental']['clash_api']['secret'] = '0123456789abcdef0123456789abcdef'
        if name == 'pc-config.json':
            c['log']['output'] = 'C:\\Program Files\\RealityHop\\logs\\core.log'
        out.append(c)
    return out
PC_T, ROUTER_T = templates()

# json.dumps as jsave writes it
def json_dumps(data, indent):
    return json.dumps(data, indent=indent)

# write_configs on given configs; the two files' text after it
def write_configs(pc, router, best):
    put(N.PC_CFG, pc); put(N.ROUTER_CFG, router); N.write_configs(best)
    return [text(N.PC_CFG), text(N.ROUTER_CFG)]

# dns_apply on given configs and Settings 'dns'; (changed, file texts)
def dns_apply(pc, router, dns):
    put(N.SETTINGS, {'dns': dns}, 1); put(N.PC_CFG, pc); put(N.ROUTER_CFG, router)
    return [N.dns_apply(), text(N.PC_CFG), text(N.ROUTER_CFG)]

# api_lockdown's edits, with rh-core present, its version and its config check given
def api_lockdown(pc, store, check_ok):
    put(N.PC_CFG, pc); os.makedirs(os.path.dirname(N.CORE), exist_ok=True); open(N.CORE, 'w').close()
    run, store_ok = N.run, N.store_ok
    N.run = lambda *a, **k: type('R', (), {'returncode': 0 if check_ok else 1})()
    N.store_ok = lambda: store
    try:
        N.api_lockdown()
    finally:
        N.run, N.store_ok = run, store_ok
        os.remove(N.CORE)
    return text(N.PC_CFG)

# settings_load on a given settings.json and this-pc.json; the settings and the settings.json text after it
def settings_load(raw, top50):
    put(N.SETTINGS, raw, 1); put(N.THIS_PC, {'top50': True} if top50 else {}, None)
    return [N.settings_load(), text(N.SETTINGS)]

# A fixed clock: today 2026-11-18, now 12:34:56.789, time() 1800000000
class _Date(_dt.date):
    @classmethod
    def today(cls):
        return cls(2026, 11, 18)
class _DateTime(_dt.datetime):
    @classmethod
    def now(cls, tz=None):
        return cls(2026, 11, 18, 12, 34, 56, 789)
NOW = 1_800_000_000.0

# Run the engine with the fixed clock and with say() collected
@contextlib.contextmanager
def frozen():
    said, keep = [], (N.datetime, N.time, N.say)
    N.datetime = types.SimpleNamespace(date=_Date, datetime=_DateTime)
    N.time = types.SimpleNamespace(time=lambda: NOW, sleep=lambda s: None, perf_counter=time.perf_counter, strftime=lambda f: '12:34')
    N.say = lambda *a: said.append(' '.join(map(str, a)))
    try:
        yield said
    finally:
        N.datetime, N.time, N.say = keep

# table(): the lines it prints
def table(nodes):
    with frozen() as said:
        N.table(json.loads(json.dumps(nodes)))
    return said

# status_table(): the lines it prints
def status_table(nodes, ph, pc):
    with frozen() as said:
        N.status_table(nodes, ph, pc)
    return said

# write_txt(): the nodes.txt it writes
def write_txt(nodes, best):
    nodes = json.loads(json.dumps(nodes)); best = [nodes[i] if isinstance(i, int) else i for i in best]
    with frozen():
        N.write_txt(nodes, best)
    return text(N.NODES_TXT)

# known_where / known_failed on a given nodes.txt
def known_where(txt, full):
    open(N.NODES_TXT, 'w', encoding='utf-8').write(txt)
    return N.known_where(full)
def known_failed(txt):
    open(N.NODES_TXT, 'w', encoding='utf-8').write(txt)
    return sorted(N.known_failed())

# vault_seed on a given nodes.txt and PC config (router features off); the vault and vault.json after it
def vault_seed(txt, pc):
    for p in (N.VAULT, N.SETTINGS):
        if os.path.exists(p): os.remove(p)
    open(N.NODES_TXT, 'w', encoding='utf-8').write(txt); put(N.PC_CFG, pc)
    with frozen():
        v = N.vault_seed()
    return [v, text(N.VAULT)]

# vault_record on a given vault and test results; vault.json after it
def vault_record(vault, nodes):
    put(N.VAULT, vault, 1)
    with frozen():
        N.vault_record(nodes)
    return text(N.VAULT)

# vault_nodes on a given vault
def vault_nodes(vault, exclude):
    put(N.VAULT, vault, 1)
    return N.vault_nodes(exclude)

# floor() and speed_rank() with a given MIN_MBPS and this-pc.json top_mbps
def floor(min_mbps, top):
    put(N.THIS_PC, {'top_mbps': top} if top else {}, None); keep = N.MIN_MBPS; N.MIN_MBPS = min_mbps
    try:
        with frozen():
            return N.floor()
    finally:
        N.MIN_MBPS = keep
def speed_rank(min_mbps, top, n):
    put(N.THIS_PC, {'top_mbps': top} if top else {}, None); keep = N.MIN_MBPS; N.MIN_MBPS = min_mbps
    try:
        with frozen():
            return N.speed_rank(n)
    finally:
        N.MIN_MBPS = keep

# mbps() on a given curl output (floor 100)
def mbps(stdout):
    run, fl = N.run, N.floor
    N.run = lambda *a, **k: types.SimpleNamespace(stdout=stdout, returncode=0); N.floor = lambda: 100
    try:
        return N.mbps(23000)
    finally:
        N.run, N.floor = run, fl

# speed_test() with mbps() faked from a table {port: [shared result, alone result]}, MIN_MBPS and this-pc.json
# top_mbps: [nodes after, shared tests (sorted), alone tests in order, what it said, this-pc.json after]
def speed_test(nodes, table, min_mbps, top):
    put(N.THIS_PC, {'top_mbps': top} if top else {}, None)
    keep, shared, alone = (N.mbps, N.MIN_MBPS), [], []
    def mbps(port, secs=4, give_up=True):
        (alone if give_up else shared).append([port, secs])
        return table[str(port)][1 if give_up else 0]
    N.mbps, N.MIN_MBPS = mbps, min_mbps
    nodes = json.loads(json.dumps(nodes))
    try:
        with frozen() as said:
            N.speed_test(nodes, 23000)
        return [nodes, sorted(shared), alone, said, N.jload(N.THIS_PC)]
    finally:
        N.mbps, N.MIN_MBPS = keep

# Run fn with ssh() replaced: each call is recorded and answered with the next canned reply. [result, calls]
def over_ssh(fn, replies, *args, devices='', router_cfg=None, settings=None):
    put(N.SETTINGS, dict({'router': 1, 'devices': devices}, **(settings or {})), 1); put(N.ROUTER_CFG, router_cfg or ROUTER_NODES)
    calls, replies, keep = [], list(replies), (N.ssh, N.CANCEL)
    def ssh(cmd, stdin=None, timeout=240):
        calls.append({'cmd': cmd, 'stdin': stdin if isinstance(stdin, str) or stdin is None else '<bytes>', 'timeout': timeout})
        if not replies:
            raise RuntimeError('router unreachable over ssh')
        r = replies.pop(0)
        if isinstance(r, dict):
            raise RuntimeError(r['error'])
        return r
    N.ssh = ssh
    try:
        with frozen():
            out = fn(*args)
        return [json.loads(json.dumps(out, default=sorted)), calls]
    except RuntimeError as e:
        return [{'error': 'RuntimeError', 'message': str(e)}, calls]
    finally:
        N.ssh = keep[0]

# handshake(): [nodes after, calls]
def handshake(nodes, replies):
    nodes = json.loads(json.dumps(nodes)); r = over_ssh(N.handshake, replies, nodes)
    return [nodes if not isinstance(r[0], dict) else r[0], r[1]]

def router_ping(nodes, replies):
    nodes = json.loads(json.dumps(nodes)); r = over_ssh(N.router_ping, replies, nodes)
    return [[n['server'] for n in r[0]] if isinstance(r[0], list) else r[0], [n.get('tcp_ms') for n in nodes], r[1]]

def check_phone(nodes, replies):
    nodes = json.loads(json.dumps(nodes)); r = over_ssh(N.check_phone, replies, nodes)
    return [r[0], [[n.get('phone_ms'), n.get('phone_why')] for n in nodes], r[1]]

def router_call(name, replies):
    return over_ssh(getattr(N, name), replies)

# push(): [answer, calls, this-pc.json router_md5]
def push(devices, replies, router_cfg, vault):
    for p in (N.THIS_PC, N.VAULT):
        if os.path.exists(p): os.remove(p)
    put(N.VAULT, vault, 1); put(N.PC_CFG, PC_NODES)
    r = over_ssh(N.push, replies, devices=devices, router_cfg=router_cfg)
    return r + [N.this_pc().get('router_md5')]

# router_sync() on a router answer, with the PC tunnel restart left out: [result, ssh calls, router-config.json,
# pc-config.json, nodes.txt, Settings 'devices', this-pc.json with router_local as "matches router-config.json"]
def router_sync(reply, me, vault, devices, router_cfg, pc_cfg):
    for p in (N.THIS_PC, N.VAULT, N.NODES_TXT):
        if os.path.exists(p): os.remove(p)
    put(N.THIS_PC, me, None); put(N.VAULT, vault, 1); put(N.PC_CFG, pc_cfg)
    keep, N.pc_push = N.pc_push, lambda: True
    try:
        r = over_ssh(N.router_sync, [reply, ''], devices=devices, router_cfg=router_cfg)
    finally:
        N.pc_push = keep
    me = N.jload(N.THIS_PC)
    if 'router_local' in me:
        me['router_local'] = me['router_local'] == N.file_md5(N.ROUTER_CFG)
    return r + [text(N.ROUTER_CFG), text(N.PC_CFG), text(N.NODES_TXT), N.settings_load()['devices'], me]

# macos.key_console(): the add-router-key.command it writes (Terminal is not opened)
def key_console(ssh, host, remote):
    keep, macos.subprocess.Popen = macos.subprocess.Popen, lambda *a, **k: None
    try:
        macos.key_console(SANDBOX, ssh, host, remote)
        return text(os.path.join(SANDBOX, 'add-router-key.command'))
    finally:
        macos.subprocess.Popen = keep

# nft_script() for a Settings 'devices' value
def nft_script(devices):
    put(N.SETTINGS, {'router': 1, 'devices': devices}, 1)
    return N.nft_script()

# rc_script() for a router build manifest
def rc_script(m, router_cfg):
    put(N.ROUTER_CFG, router_cfg)
    return N.rc_script(m)

# Run fn with run() replaced: each call's command and stdin recorded, each answered with the next canned stdout
def over_run(fn, outs, *args, **env):
    calls, outs, keep, old_env = [], list(outs), N.run, {k: os.environ.get(k) for k in env}
    def run(cmd, stdin=None, timeout=240, **kw):
        calls.append({'cmd': [os.path.basename(cmd[0])] + list(cmd[1:]), 'stdin': stdin, 'timeout': timeout})
        o = outs.pop(0) if outs else ''
        return types.SimpleNamespace(stdout=o, stderr='', returncode=0 if o is not None else 1)
    N.run = run; os.environ.update(env)
    try:
        with frozen():
            out = fn(*args)
        return [json.loads(json.dumps(out)), calls]
    finally:
        N.run = keep
        for k, v in old_env.items():
            os.environ.pop(k) if v is None else os.environ.__setitem__(k, v)

# boot_task_ps for a given user
def boot_task_ps(core, cfg, domain, user):
    os.environ.update(USERDOMAIN=domain, USERNAME=user)
    return N.boot_task_ps(core, cfg)

# logon_task(): the PowerShell it runs, for a given user and command
def logon_task(cmd, domain, user):
    keep = N.DATA; N.DATA = 'C:\\Program Files\\RealityHop'   # a fixed folder: no temp path in the fixture
    try:
        return over_run(N.logon_task, ['ok'], cmd, USERDOMAIN=domain, USERNAME=user)
    finally:
        N.DATA = keep

# The command line elevated() hands to ShellExecuteExW
def elevated_params(ps):
    return N.subprocess.list2cmdline(['-NoProfile', '-NonInteractive', '-WindowStyle', 'Hidden', '-Command', ps])

# shortcuts(): the PowerShell runs for a given target exe and Settings
def shortcuts(target, start_menu, desktop_icon):
    put(N.SETTINGS, {'start_menu': start_menu, 'desktop_icon': desktop_icon}, 1)
    keep = (N.exe, install.desktop_lnk, install.MENU_LNK, N.lnk_migrate)
    N.exe = lambda: target; install.desktop_lnk = lambda: 'C:\\Users\\Pat O\u2019Neil\\Desktop\\Reality Hop.lnk'
    install.MENU_LNK = 'C:\\Users\\Pat\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Reality Hop.lnk'
    N.lnk_migrate = lambda p: None
    try:
        return over_run(install.shortcuts, [])[1]
    finally:
        N.exe, install.desktop_lnk, install.MENU_LNK, N.lnk_migrate = keep

# add_key_console(): the .cmd file (as written, before text mode doubles each CR) and the console it opens
def add_key_console(host, pub):
    keep = (N.make_key, N.subprocess.Popen); opened = []
    N.make_key = lambda: pub
    N.subprocess.Popen = lambda args, **kw: opened.append([os.path.basename(args[0])] + args[1:])
    try:
        N.add_key_console(host)
    finally:
        N.make_key, N.subprocess.Popen = keep
    raw = open(os.path.join(N.DATA, 'add-router-key.cmd'), 'rb').read().decode('utf-8').replace('\r\r\n', '\r\n')
    return [raw.replace(N.ssh_exe(), '<ssh>'), opened]

# router_key_remove() for a given public key
def router_key_remove(pub, own_key):
    open(N.ROUTER_KEY + '.pub', 'w').write(pub + '\n')
    return over_ssh(N.router_key_remove, ['router: the key of this PC is removed\n'], settings={'router_key': own_key})

# user_sid() from whoami's output
def user_sid(out):
    return over_run(install.user_sid, [out])[0]

# latest() and changes() on a given GitHub API answer
def latest(answer):
    keep = N.gh_get; N.gh_get = lambda *a, **k: json.dumps(answer).encode()
    try:
        return install.latest()
    finally:
        N.gh_get = keep
def changes(answer, since):
    keep = N.gh_get; N.gh_get = lambda *a, **k: json.dumps(answer).encode()
    try:
        return install.changes(since)
    finally:
        N.gh_get = keep

# geolocate() with a given vault, ip-api answer and router-only flag: [nodes after, curl calls]
def geolocate(nodes, vault, answer, router_only):
    put(N.VAULT, vault, 1); put(N.SETTINGS, {'router': 1 if router_only else 0, 'pc_tunnel': 0 if router_only else 1}, 1)
    nodes = json.loads(json.dumps(nodes))
    r = over_run(N.geolocate, [answer], nodes)
    return [nodes, r[1]]

# One Scrubber over several texts in turn, for a made-up person and PC; the configs and vault in the sandbox
FAKE_ME = {'USERNAME': 'patricia', 'COMPUTERNAME': 'DESKTOP-7Q2', 'USERDOMAIN': 'DESKTOP-7Q2', 'USERPROFILE': 'C:\\Users\\patricia'}
def scrub(texts, me):
    put(N.PC_CFG, PC_NODES); put(N.ROUTER_CFG, ROUTER_NODES); put(N.VAULT, VAULT, 1)
    old = {k: os.environ.get(k) for k in FAKE_ME}
    for k in FAKE_ME:
        os.environ.pop(k, None)
    os.environ.update(me)
    try:
        sc = report.Scrubber()
        return [sc(t) for t in texts]
    finally:
        for k, v in old.items():
            os.environ.pop(k, None) if v is None else os.environ.__setitem__(k, v)

# i18n.T in a language, on Windows or a Mac
def t(lang, mac, text, kw):
    keep = i18n.MAC; i18n.MAC = mac; i18n.use(lang)
    try:
        return i18n.T(text, **kw)
    finally:
        i18n.MAC = keep; i18n.use('en')

# tray_mark() for test results {tag: ms or None}, slow above 200 ms
def tray_mark(res, dead):
    st, keep = {}, N.SLOW; N.SLOW = 200
    try:
        with frozen():
            N.tray_mark(st, res, dead)
        return st
    finally:
        N.SLOW = keep

# vpn_state() on a given PIA settings file (Windows), and vpn_check()'s line in English
def vpn_state(pia):
    keep = N.PIA_SETTINGS; N.PIA_SETTINGS = os.path.join(SANDBOX, 'pia-settings.json')
    try:
        if pia is None:
            os.path.exists(N.PIA_SETTINGS) and os.remove(N.PIA_SETTINGS)
        else:
            put(N.PIA_SETTINGS, pia)
        return [N.vpn_state(), N.vpn_check()]
    finally:
        N.PIA_SETTINGS = keep

RH_DIR = ntpath.join('C:' + ntpath.sep, 'Program Files', 'RealityHop')   # an install as 1.7.x lays it out
RH_CORE, RH_TEST = ntpath.join(RH_DIR, 'PC', 'rh-core.exe'), ntpath.join(RH_DIR, 'rh-test.exe')

# pia_bypass() on given PIA settings (None: no file) and this-pc.json, piactl faked:
# [result, piactl arguments, what it said, this-pc.json after]
def pia_bypass(st, me, mac, servers, extra, app, in_use, fail=False):
    keep = (N.MAC, N.PIACTL, N.PIA_SETTINGS, N.FROZEN, N.CORE, N.DATA, N.nodes_of, macos.bundle, N.run)
    N.macos = macos   # engine imports it only on a Mac
    calls = []
    def run(cmd, stdin=None, timeout=240, **kw):
        calls.append(list(cmd[1:]))
        return types.SimpleNamespace(stdout='', stderr='bad settings' if fail else '', returncode=1 if fail else 0)
    N.PIACTL = os.path.join(SANDBOX, 'piactl'); open(N.PIACTL, 'w').close()
    N.PIA_SETTINGS = os.path.join(SANDBOX, 'pia-settings.json')
    if st is None:
        os.path.exists(N.PIA_SETTINGS) and os.remove(N.PIA_SETTINGS)
    else:
        put(N.PIA_SETTINGS, st)
    put(N.THIS_PC, me, None)
    N.MAC, N.FROZEN, N.run = mac, app, run
    N.CORE, N.DATA = RH_CORE, RH_DIR
    N.nodes_of = lambda cfg: [{'server': x} for x in servers]
    macos.bundle = lambda exe=None: '/Applications/RealityHop.app'
    try:
        with frozen() as said:
            ok = N.pia_bypass(extra, in_use)
        return [ok, calls, said, N.jload(N.THIS_PC)]
    finally:
        N.MAC, N.PIACTL, N.PIA_SETTINGS, N.FROZEN, N.CORE, N.DATA, N.nodes_of, macos.bundle, N.run = keep

# core_release() and core_versions() on a given GitHub answer and channel (Windows builds)
def core_release(rels, channel):
    keep = N.gh_get
    N.gh_get = lambda url, timeout=30: json.dumps(rels).encode()
    put(N.SETTINGS, {'singbox': channel}, 1); put(N.THIS_PC, {}, None)
    try:
        try:
            tag = N.core_release()['tag_name']
        except RuntimeError as e:
            tag = {'error': str(e)}
        return [tag, N.core_versions()]
    finally:
        N.gh_get = keep

# The MAC address in Windows arp -a output, as router_mac() reads it
def arp_windows(out):
    m = N.re.search(r'\b([0-9a-f]{2}(?:-[0-9a-f]{2}){5})\b', out.lower())   # the same pattern as router_mac()
    return m.group(1) if m else ''

# dns_proxies() on given scutil --dns output
def dns_proxies(out):
    keep = macos.subprocess.run
    macos.subprocess.run = lambda *a, **k: types.SimpleNamespace(stdout=out)
    try:
        return macos.dns_proxies()
    finally:
        macos.subprocess.run = keep

# agent_write(): the plist file it writes
def agent_plist(label, args, cwd):
    keep = macos.AGENTS; macos.AGENTS = os.path.join(SANDBOX, 'LaunchAgents')
    try:
        macos.agent_write(label, args, cwd)
        return open(macos.agent_path(label), 'rb').read().decode('utf-8')
    finally:
        macos.AGENTS = keep

# settings_save() on a given settings.json and configs: [settings, settings.json, pc-config.json, router-config.json]
def settings_save(raw, changes, pc, router):
    put(N.SETTINGS, raw, 1); put(N.PC_CFG, pc); put(N.ROUTER_CFG, router); put(N.THIS_PC, {}, None)
    d = N.settings_save(changes)
    return [d, text(N.SETTINGS), text(N.PC_CFG), text(N.ROUTER_CFG)]

# hist_add() on a given history.json at the fixed time: history.json after it
def hist_add(hist, pings, event):
    if hist is None:
        os.path.exists(N.HISTORY) and os.remove(N.HISTORY)
    else:
        put(N.HISTORY, hist, None)
    with frozen():
        N.hist_add(pings, tuple(event) if event else None)
    return text(N.HISTORY)

NODE = {'server': '8.8.8.8', 'uuid': U, 'pbk': K, 'sid': '0a1b', 'sni': 'www.apple.com', 'fp': 'firefox'}
NODES = [NODE, dict(NODE, server='1.1.1.1', sid='', fp='chrome'), dict(NODE, server='9.9.9.9', sni='cdnjs.com')]
TESTED = [dict(NODE, ok=True, ms=120, exit='8.8.8.8', where='Mountain View, US', isp='Google', tcp_ms=15),
          dict(NODES[1], ok=True, ms=80, exit='1.1.1.1', mbps=250, where='Łódź, PL', isp='Cloudflare', tcp_ms=9, inuse=True),
          dict(NODES[2], ok=False, ms=None, exit='-', why='timeout', tcp_ms=None),
          dict(NODE, server='4.4.4.4', ok=True, ms=0, exit='4.4.4.4', mbps=None, where='A place name that is far too long', isp='?'),
          dict(NODE, server='123.123.123.123', sni='www.microsoft.com', ok=True, ms=12345, exit='x', mbps=3, tcp_ms=1234567),
          dict(NODE, server='5.5.5.5', ok=False, ms=None, why='reality verification failed', where='?')]
WT = write_txt(TESTED, [1, 0])
PC_NODES = json.loads(json.dumps(PC_T)); PC_NODES['outbounds'] = [N.outbound19(n, f'vless-{i}') for i, n in enumerate(NODES, 1)] + PC_NODES['outbounds']
VAULT = {'8.8.8.8': dict(NODE, where='US', isp='G', ms=50, last_ok='2026-11-17 10:00:00', passes=3, fail_streak=0),
         '1.1.1.1': dict(NODES[1], where=None, isp=None, ms=None, last_ok='2026-11-18', passes=1, fail_streak=2),
         '9.9.9.9': dict(NODES[2], fp='', ms=20, last_ok='2026-11-17 09:00:00', passes=1, fail_streak=0)}
ROUTER_NODES = json.loads(json.dumps(ROUTER_T))
ROUTER_NODES['outbounds'] = [N.outbound19(n, f'vless-{i}') for i, n in enumerate(NODES, 1)] + ROUTER_NODES['outbounds']
ROUTER_NODES['outbounds'][3]['outbounds'] = ['vless-1', 'vless-2', 'vless-3']
TAGGED = [dict(n, tag=f'vless-{i}') for i, n in enumerate(NODES, 1)]
HS_LOG = ('---LOG---\nERROR [n-1] outbound/vless[n-1]: REALITY verification failed\n'
          'ERROR [in-2] inbound/mixed[in-2]: dial tcp 9.9.9.9:443: i/o timeout\n')
VAULT_MORE = dict(VAULT, **{s: dict(NODE, server=s, where='X', isp='Y', ms=i, last_ok='2026-11-1' + str(i % 9), passes=1, fail_streak=0)
                           for i, s in enumerate(f'2.2.{k // 250}.{k % 250 + 1}' for k in range(70))})
MANIFEST = {'channel': 'stable', 'singbox': '1.14.4', 'sha256': 'ab' * 32, 'asset': 'rh-router-core-linux-arm64'}
PC_HAND = dict(json.loads(json.dumps(PC_T)), dns={'servers': [{'type': 'https', 'tag': 'mine', 'server': '8.8.8.8'}], 'final': 'mine'})
PC_OLD_API = json.loads(json.dumps(PC_T)); PC_OLD_API['experimental']['clash_api'] = {'external_controller': '127.0.0.1:9090', 'secret': 'x'}
SETTINGS_RAW = [
    {}, {'top': 40}, {'top': 40.0}, {'top': '40'}, {'top': 5}, {'top': 500}, {'check_min': 0}, {'check_min': '30'},
    {'check_min': ' 30 '}, {'check_min': '1_000'}, {'check_min': '30.5'}, {'check_min': 29.9}, {'check_min': None},
    {'check_min': True}, {'check_min': [1]}, {'slow_ms': -5}, {'tolerance_ms': '+7'}, {'min_mbps': 2000},
    {'router': 'yes'}, {'router': '1'}, {'router': 1.7}, {'router_host': 'bad host'}, {'router_host': '10.0.0.1'},
    {'router_host': None}, {'router_key': None}, {'router_key': 5}, {'router_key': True}, {'router_key': 1.5},
    {'router_key': [1, 'a']}, {'router_key': {'a': 1}}, {'router_key': 1e20}, {'router_key': 1e-7},
    {'singbox': 'testing'}, {'router_singbox': 'nightly'}, {'router_singbox': 'stable'}, {'dns': 'cloudflare'},
    {'dns': 'google'}, {'dns': ''}, {'lang': 'fa'}, {'lang': 'xx'}, {'lang': None}, {'extra': 1, 'auto_update': 0},
    {'devices': '192.168.8.10 192.168.8.11'},
]

# Function -> (source file, argument lists). Each argument list is one call.
WIN_ASSET = lambda v: {'name': f'sing-box-{v}-windows-amd64.zip'}
REL = [{'tag_name': 'v1.15.0-alpha.13', 'prerelease': True, 'assets': [WIN_ASSET('1.15.0-alpha.13')]},
       {'tag_name': 'v1.14.4', 'prerelease': False, 'assets': [{'name': 'x.txt'}, WIN_ASSET('1.14.4')]},
       {'tag_name': 'v1.14.5', 'prerelease': False, 'assets': [{'name': 'sing-box-1.14.5-darwin-arm64.tar.gz'}]},
       {'tag_name': 'v1.16.0', 'prerelease': False, 'draft': True, 'assets': [WIN_ASSET('1.16.0')]},
       {'tag_name': 'v1.13.9', 'assets': [WIN_ASSET('1.13.9')]}]
# Nodes for speed_test: every fifth one failed; the rest by latency. A speed table entry: [shared, alone]
SPEED_NODES = [{'server': f'203.0.113.{i}', 'ok': i % 5 != 4, 'ms': None if i % 5 == 4 else 300 - 20 * i} for i in range(12)]
SPEED_KINDS = [[150, None], [5, None], [60, 120], [None, 90], [40, None], [0, 0], [99, 101], [12, 30]]
SPEED_TABLE = {str(23000 + i): SPEED_KINDS[i % len(SPEED_KINDS)] for i in range(40)}
SPEED_MANY = [{'server': f'198.51.100.{i}', 'ok': True, 'ms': 100 + i % 3} for i in range(30)]
# Router answers for router_sync: md5, ---, rh-devices, ---, heal.state, ---, config.json
def sync_reply(cfg, md5='c' * 32, devs='', heal=''):
    body = cfg if isinstance(cfg, str) else json.dumps(cfg, indent=2)
    return '\n'.join([md5, '---', devs, '', '---', heal, '', '---', body, ''])
SYNC_NEW = [dict(NODE, server='4.4.4.4'), dict(NODES[1], server='5.5.5.5', fp='safari'), NODES[2]]
ROUTER_OTHER = json.loads(json.dumps(ROUTER_NODES))
ROUTER_OTHER['outbounds'] = [N.outbound19(n, f'vless-{i}') for i, n in enumerate(SYNC_NEW, 1)] + ROUTER_OTHER['outbounds'][3:]
ROUTER_OTHER['experimental']['clash_api']['secret'] = 'fedcba9876543210fedcba9876543210'
ROUTER_BAD = json.loads(json.dumps(ROUTER_OTHER)); ROUTER_BAD['outbounds'][1]['server'] = '10.0.0.5'
ROUTER_BADKEY = json.loads(json.dumps(ROUTER_OTHER)); ROUTER_BADKEY['experimental']['clash_api']['secret'] = 'not hex'
ROUTER_NOAPI = json.loads(json.dumps(ROUTER_OTHER)); del ROUTER_NOAPI['experimental']
SYNC_VAULT = dict(VAULT, **{'4.4.4.4': dict(NODE, server='4.4.4.4', where='Paris, FR', isp='OVH', ms=33, fp='', last_ok='2026-11-17', passes=1, fail_streak=0)})
CASES = {
    'host_ok': (N.host_ok, 'src/engine.py', [[h] for h in (
        '192.168.8.1', 'router.lan', 'gl-mt6000', '-bad', '.bad', '', None, 'a b', 'a;rm', 'a_b', 'ä.com', '1.2.3.4\n',
        'A' * 300)]),
    'public_ip': (N.public_ip, 'src/engine.py', [[h] for h in (
        '8.8.8.8', '1.1.1.1', '203.0.113.5', '100.100.100.100', '192.168.1.1', '10.0.0.1', '172.16.5.4', '127.0.0.1',
        '100.64.0.1', '169.254.1.1', '0.0.0.0', '0.1.2.3', '255.255.255.255', '224.0.0.1', '239.255.255.250', '240.0.0.1',
        '198.18.0.1', '198.51.100.7', '192.0.0.9', '192.0.0.10', '192.0.0.170', '192.0.2.1', '192.31.196.1',
        '192.52.193.1', '192.88.99.1', '192.175.48.1', '01.2.3.4', '1.2.3', '1.2.3.4.5', '256.1.1.1', ' 1.1.1.1',
        '1.1.1.1 ', '', '\u0661.1.1.1', '1.1.1.1\n')]),
    'lan_ip': (N.lan_ip, 'src/engine.py', [[h] for h in (
        '192.168.8.100', '10.1.2.3', '172.16.0.1', '172.31.255.255', '172.32.0.1', '172.15.255.255', '8.8.8.8',
        '127.0.0.1', '192.168.1', 'abc', '', '01.1.1.1', '192.168.001.1', '192.168.0.0', '10.255.255.255')]),
    'mirrors': (N.mirrors, 'src/engine.py', [[u] for u in (
        'https://raw.githubusercontent.com/rtwo2/FastNodes/main/sub/verified_tls.txt',
        'https://raw.githubusercontent.com/a/b/c/d/e/f.txt', 'https://raw.githubusercontent.com/a/b',
        'https://example.com/a/b/c/d.txt', 'https://RAW.githubusercontent.com/a/b/main/x.txt', '')]),
    'psq': (N.psq, 'src/engine.py', [[s] for s in (
        'plain', "O'Brien", '\u2018x\u2019', 'a\u201ab\u201bc', '', '$(evil)', "''", 'C:\\Program Files\\Reality Hop')]),
    'set_mark': (N.set_mark, 'src/engine.py', [[n] for n in (
        [{'server': '1.2.3.4'}, {'server': '5.6.7.8'}], [{'server': '5.6.7.8'}, {'server': '1.2.3.4'}], [],
        [{'server': '9.9.9.9', 'uuid': 'x'}])]),
    'parse_delay': (N.parse_delay, 'src/engine.py', [[t] for t in (
        '{"delay": 123}', '{"delay": 0}', '{"delay": 12.5}', '{"message": "timeout"}', '{}', 'garbage', '', '[1]',
        '{"delay": null}')]),
    'vkey': (N.vkey, 'src/engine.py', [[t] for t in (
        '1.13.0', 'v1.13.0', '1.13.0-alpha.2', '1.13.0-beta.1', '1.13.0-rc.1', '1.13.0-rc1', '1.13.0-rc', '1.13.0-dev',
        '1.12', '', None, 'x', '10.0.0-beta.10', '1.13.0.5')]),
    'feed_parse': (feed_parse, 'src/engine.py', [[t] for t in FEED]),
    'node_ok': (N.node_ok, 'src/engine.py', [[n] for n in (
        NODE, dict(NODE, server='10.0.0.1'), dict(NODE, uuid='x'), dict(NODE, pbk=K[:-1]), dict(NODE, sid='abc'),
        dict(NODE, sni='bad host'), dict(NODE, sni='example.com'), dict(NODE, fp='opera'), dict(NODE, sid=''))]),
    'outbound19': (N.outbound19, 'src/engine.py', [[n, f'vless-{i}'] for i, n in enumerate(NODES, 1)]),
    'from_outbound': (N.from_outbound, 'src/engine.py', [[N.outbound19(n, 'vless-2')] for n in NODES]),
    'router_cfg_new': (N.router_cfg_new, 'src/engine.py', [[ROUTER_T], [json.loads(N.json.dumps(ROUTER_T).replace('"sniff": true', '"sniff": false'))]]),
    'json_dumps': (json_dumps, 'src/engine.py', [[d, i] for i in (None, 1, 2) for d in (
        PC_T, {}, [], {'a': []}, {'a': {}}, [1, [2, []]], 'café \U0001F600 ‫', '\x00\x1f\x7f"\\/\n\t\b\f\r',
        1.0, 0.1, 1e-05, 1e16, 123456789.123, -0.0, 1e22, 5e-324, True, None, {'k': [None, False, 3]})]),
    'write_configs': (write_configs, 'src/engine.py', [
        [PC_T, ROUTER_T, NODES], [PC_T, ROUTER_T, NODES[:1]], [PC_T, ROUTER_T, []]]),
    'dns_apply': (dns_apply, 'src/engine.py', [
        [PC_T, ROUTER_T, 'cloudflare'], [PC_T, ROUTER_T, 'quad9'], [PC_T, ROUTER_T, ''], [PC_HAND, ROUTER_T, 'cloudflare']]),
    'api_lockdown': (api_lockdown, 'src/engine.py', [
        [PC_T, True, True], [PC_T, False, True], [PC_OLD_API, False, True], [PC_OLD_API, True, True],
        [PC_OLD_API, True, False]]),
    'settings_load': (settings_load, 'src/engine.py', [[r, False] for r in SETTINGS_RAW] + [[{'top': 40}, True]]),
    'table': (table, 'src/engine.py', [[TESTED], [[]]]),
    'status_table': (status_table, 'src/engine.py', [
        [[dict(NODE, tag='vless-1', where='US', phone_ms=50, phone_why='', pc_ms=None, pc_why='timeout'),
          dict(NODES[1], tag='vless-2', phone_ms=None, phone_why='no-response', pc_ms=7, pc_why='')],
         {'service': True, 'now': 'vless-1'}, {'now': 'vless-2', 'exit': '8.8.8.8'}],
        [[], {'service': False, 'now': None}, None], [[], {'service': True, 'now': ''}, {'now': None, 'exit': ''}]]),
    'write_txt': (write_txt, 'src/engine.py', [[TESTED, [1, 0]], [TESTED, []], [[], []],
                                               [TESTED, [dict(NODE, server='7.7.7.7', sid='')]]]),
    'known_where': (known_where, 'src/engine.py', [[WT, False], [WT, True], ['', True],
        ['  vless-1  1.2.3.4  www.apple.com  ?  Somewhere   ISP X\n  5.6.7.8 x 12ms  ?\n 9.9.9.9 a 5ms Here  There\n', True],
        ['\tvless-12 2.2.2.2 s 1ms  City,  Land    Isp  Co\n  300.1.1.1 s ?   Big\n', False]]),
    'known_failed': (known_failed, 'src/engine.py', [[WT], [''], ['FAILED\n  1.2.3.4 x\n 5.6.7.8\n  9.9.9.9\tz\nFAILEDX\n']]),
    'vault_seed': (vault_seed, 'src/engine.py', [[WT, PC_T], [WT, PC_NODES], ['', PC_NODES], ['', PC_T]]),
    'vault_record': (vault_record, 'src/engine.py', [
        [VAULT, [dict(NODE, ok=True, ms=33, where='Here', isp=None), dict(NODES[1], ok=False), NODES[2],
                 dict(NODE, server='2.2.2.2', ok=False), dict(NODE, server='3.3.3.3', ok=True, ms=None)]],
        [VAULT, [dict(NODES[1], ok=False), dict(NODES[2], ok=False)]]]),
    'vault_nodes': (vault_nodes, 'src/engine.py', [[VAULT, []], [VAULT, ['8.8.8.8']]]),
    'floor': (floor, 'src/engine.py', [[100, None], [100, [NOW - 100, 50]], [100, [NOW - 100, 120]],
                                       [100, [NOW - 90000, 50]], [0, [NOW, 50]], [100, [NOW - 10, 0]], [101, [NOW, 200]],
                                       [101, [NOW, 30]], [100, [NOW, 107]]]),
    'speed_rank': (speed_rank, 'src/engine.py', [[m, top, n] for m, top in ((100, None), (0, None), (100, [NOW, 60]))
        for n in ({'mbps': None, 'ms': 50}, {'mbps': 150, 'ms': 50}, {'mbps': 10, 'ms': 50}, {'ms': 0, 'mbps': 200},
                  {}, {'mbps': 42, 'ms': 7}, {'mbps': 0, 'ms': 5})]),
    'mbps': (mbps, 'src/engine.py', [[o] for o in (
        ' 200 12500000 4.0 1.5', ' 000 0 4.0 0', ' 404 100 0.5 0.4', 'garbage', ' 200 1000 4.0 0', ' 200 125000000 4.0 3.95',
        ' 200 312500 2 1', ' 200 937500 2 1', 'x y 200 1e3 1 0.5', ' 200 1 2', '')]),
    'handshake': (handshake, 'src/engine.py', [
        [NODES, ['nt-0 PASS 8.8.8.8 0.5123\nnt-1 FAIL\nnt-2 FAIL\njunk\n' + HS_LOG]],
        [NODES, ['nt-2 PASS 9.9.9.9 1,5\nnt-0 PASS 1.2.3.4\n---LOG---\n']],
        [NODES, ['DEAD\nFATAL outbound[1]: bad key', 'nt-0 PASS 8.8.8.8 0.2\nnt-2 FAIL\n---LOG---\nx [n-2] EOF\n']],
        [NODES, ['DEAD\nstart n-2 failed', 'nt-0 FAIL\nnt-1 PASS 1.1.1.1 0.3\n---LOG---\n']],
        [NODES, ['DEAD\nsomething else broke']], [NODES, [{'error': 'router unreachable over ssh'}]],
        [[dict(NODE, server=f'{i}.{i}.{i}.{i}') for i in range(1, 8)], ['---LOG---\n']]]),
    'router_ping': (router_ping, 'src/engine.py', [
        [NODES, ['0 0.031\n1 0,120\n2 0.000\n']], [NODES, ['2 0.5\nx 1\n1 abc\n0\n']],
        [[dict(NODE, server=f'1.1.1.{i}') for i in range(34)], ['5 0.0101\n33 0.2\n']]]),
    'check_phone': (check_phone, 'src/engine.py', [
        [TAGGED, ['svc 1234\nvless-1 {"delay":45}\nvless-2 {"message":"timeout"}\nvless-3 \n'
                  'now {"now":"vless-1","type":"URLTest"}\ncfg ' + 'a' * 32 + '\n']],
        [TAGGED, ['svc \nnow garbage\n']], [TAGGED[:1], ['']]]),
    'router_now': (router_call, 'src/engine.py', [['router_now', ['{"now": "vless-2"}']], ['router_now', ['nope']],
                                                  ['router_now', []]]),
    'router_feed': (router_call, 'src/engine.py', [['router_feed', ['vless://a\nvless://b\n']], ['router_feed', ['']]]),
    'router_info': (router_call, 'src/engine.py', [
        ['router_info', ['GL.iNet GL-MT6000\n|\n4.6.8\n|\nsing-box version 1.14.4\n|\njq\ncurl\n|\n']],
        ['router_info', ['GL.iNet GL-MT6000\n|\n4.6.8\n|\nsing-box version 1.15.0-beta.1\n|\ncurl\n|\nstable 1.15.0 ab\n']],
        ['router_info', ['GL.iNet GL-MT6000\n|\n4.6.8\n|\nsing-box version 1.15.0\n|\n|\n']], ['router_info', ['']]]),
    'router_core_state': (router_call, 'src/engine.py', [
        ['router_core_state', ['sing-box version 1.14.4\n|\nstable 1.14.4 ' + 'ab' * 32 + '\n|\naarch64\n']],
        ['router_core_state', ['sing-box version 1.9.7\n|\n|\nmips\n']], ['router_core_state', ['']]]),
    'router_clients': (router_call, 'src/engine.py', [
        ['router_clients', ['1700000000 aa:bb 192.168.8.20 phone 01:aa\n1700000000 cc:dd 192.168.8.3 * *\n'
                            'bad line\n1 2 fe80::1 x\n1 2 192.168.8.100 laptop extra\n']], ['router_clients', ['']]]),
    'router_off': (router_call, 'src/engine.py', [['router_off', ['router tunnel: off\n']]]),
    'router_install': (router_call, 'src/engine.py', [['router_install', ['  installed\nsing-box version 1.9.7\n']]]),
    'push': (push, 'src/engine.py', [
        ['192.168.8.10, 192.168.8.11; 8.8.8.8', ['sing-box service: running\ntproxy: listening\npushed-md5 ' + 'c' * 32 + '\n',
                                                 'self-repair: installed, 3 vault nodes on the router\n'], ROUTER_NODES, VAULT_MORE],
        ['all', ['CONFIG REJECTED, nothing deployed\npushed-md5 ' + 'd' * 32 + '\n'], ROUTER_NODES, VAULT],
        ['', ['sing-box service: running\n', {'error': 'router unreachable over ssh'}], ROUTER_NODES, VAULT],
        ['', [''], ROUTER_NODES, VAULT], ['', [{'error': 'router unreachable over ssh'}], ROUTER_NODES, VAULT]]),
    'nft_script': (nft_script, 'src/engine.py', [['all'], [' ALL '], ['192.168.8.10 192.168.8.11'], [''], ['10.0.0.5,,8.8.8.8;172.16.0.2']]),
    'rc_script': (rc_script, 'src/engine.py', [[MANIFEST, ROUTER_NODES], [dict(MANIFEST, channel='preview', singbox='1.15.0-beta.2'), ROUTER_T]]),
    'boot_task_ps': (boot_task_ps, 'src/engine.py', [
        ['C:\\Program Files\\RealityHop\\PC\\rh-core.exe', 'C:\\Program Files\\RealityHop\\PC\\pc-config.json', 'DESKTOP-1', 'pat'],
        ["D:\\O'Brien's Apps\\RealityHop\\PC\\rh-core.exe", "D:\\O'Brien's Apps\\RealityHop\\PC\\pc-config.json", '', 'O\u2019Brien']]),
    'logon_task': (logon_task, 'src/engine.py', [
        [['C:\\Program Files\\RealityHop\\RealityHop.exe', '--watch', '--logon'], 'DESKTOP-1', 'pat'],
        [['C:\\Python315\\pythonw.exe', 'C:\\My Code\\engine.py', '--watch'], '', "o'neil"]]),
    'lock_ps': (install.lock_ps, 'src/install.py', [['S-1-5-21-1111111111-2222222222-3333333333-1001']]),
    'legacy_ps': (install.legacy_ps if hasattr(install, 'legacy_ps') else N.legacy_ps, 'src/engine.py', [[]]),
    'elevated_params': (elevated_params, 'src/engine.py', [
        ["$d = 'C:\\Program Files\\RealityHop'; icacls $d"], ['say "hi" \\"there\\" \\\\'], ['tab\there'], ['']]),
    'shortcuts': (shortcuts, 'src/install.py', [
        ['C:\\Program Files\\RealityHop\\RealityHop.exe', 1, 1], ["D:\\Pat's\\RealityHop\\RealityHop.exe", 1, 0],
        ['C:\\x\\RealityHop.exe', 0, 0]]),
    'add_key_console': (add_key_console, 'src/engine.py', [
        ['192.168.8.1', 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHk realityhop'],
        ['router.lan', 'ssh-ed25519 AAAA%B realityhop']]),
    'router_key_remove': (router_key_remove, 'src/engine.py', [
        ['ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHk realityhop', ''], ['ssh-ed25519 AAAA realityhop', 'C:\\keys\\mine']]),
    'user_sid': (user_sid, 'src/install.py', [
        ['"desktop-1\\pat","S-1-5-21-1111111111-2222222222-3333333333-1001"\r\n'], ['nothing here'], ['']]),
    'entry': (install.entry, 'src/install.py', [
        ['C:\\Program Files\\RealityHop', 'C:\\Program Files\\RealityHop\\RealityHop.exe', None],
        ['D:\\Apps\\RealityHop', 'D:\\Apps\\RealityHop\\RealityHop.exe', '1.7.3']]),
    'latest': (latest, 'src/install.py', [
        [{'tag_name': 'v1.7.3', 'body': 'Why update: x\n\nSHA-256 of RealityHop.exe: `' + 'a' * 64 + '`\n---\nfooter',
          'assets': [{'name': 'RealityHop.exe', 'size': 1}, {'name': 'other.dmg'}]}],
        [{'tag_name': '1.8.0', 'body': None, 'assets': [{'name': 'RealityHop.exe'}]}],
        [{'tag_name': 'v1.8.1', 'body': 'x' * 2000, 'assets': [{'name': 'RealityHop.exe'}]}],
        [{'tag_name': 'v1.8.2', 'assets': []}]]),
    'changes': (changes, 'src/install.py', [
        [[{'tag_name': 'v1.7.4', 'published_at': '2026-11-20T10:00:00Z', 'body': ' notes 4 \n---\nsha'},
          {'tag_name': 'v1.7.3', 'published_at': None, 'body': None},
          {'tag_name': 'v1.8.0', 'published_at': '2026-12-01T00:00:00Z', 'body': 'big', 'prerelease': True},
          {'tag_name': 'router-core-stable', 'body': 'x'}, {'tag_name': 'v1.7.2', 'body': 'old'},
          {'tag_name': 'v1.10.0', 'body': 'ten', 'draft': False}, {'tag_name': 'v2.0.0-beta.1', 'body': 'beta'}], '1.7.2']]),
    'geolocate': (geolocate, 'src/engine.py', [
        [NODES, VAULT, json.dumps([{'query': '1.1.1.1', 'city': 'Sydney', 'regionName': 'New\tSouth  Wales\x00', 'countryCode': 'AU',
                                    'isp': 'Cloudflare, Inc. and a very long name'}, {'query': '9.9.9.9', 'city': '', 'isp': None}]), False],
        [NODES, {}, 'not json', True], [[dict(NODE, where='Known', isp='I')], {}, '[]', False],
        [[dict(NODE, server=f'3.3.{i}.1') for i in range(102)], {}, '[]', False]]),
    'scrub': (scrub, 'src/report.py', [[[
        'user patricia on DESKTOP-7Q2 opened C:\\Users\\patricia\\AppData\\Local\\RealityHop\\settings.json and PATRICIA.txt',
        'D:\\Users\\Someone Else\\x.txt and /Users/sam/Library and /Users/Shared/x and a/Users/kept',
        '+0100 2026-11-18 10:00:00 ERROR [12 3.2ms] outbound/vless[vless-1]: dial 8.8.8.8:443 via 192.168.8.20 to 10.1.2.3',
        'connected to 1.1.1.1, 127.0.0.1, 9.9.9.9, 100.64.1.1, 169.254.3.4, 203.0.113.7, 01.2.3.4, 999.1.1.1, 1.2.3.4.5, 172.16.0.9',
        'node 8.8.8.8 again and 1.1.1.1 and 9.9.9.9 and lan 192.168.8.20 again',
        'sid=0a1b&pbk=x secret: abc token=xyz Bearer 0123456789abcdef password = hunter2 short_id:ff',
        'mail pat.o-neil+rh@example.co.uk now; www.apple.com and cdnjs.com, evil-site.org, sub.domain.example.net, engine.py, heal.sh, crt.sh, report.pyw, obsidian.md, readme.md',
        'sing-box-1.14.4-windows-amd64.zip RealityHop-mac-apple-silicon.dmg com.realityhop.watcher.plist realityhop.app x.y',
        'uuid a1b2c3d4-e5f6-7890-abcd-ef1234567890 mac AA:BB:CC:DD:EE:FF and aa-bb-cc-dd-ee-ff and aabb.ccdd.eeff again AA:BB:CC:DD:EE:FF',
        'ipv6 fe80::1 2001:db8:0:0:0:0:2:1 and ::1 and 10:30:45 and a:b:c and 2001:db8::1%eth0',
        'key ' + K + ' hash ' + 'a' * 32 + ' sha ' + 'B' * 64 + ' too long ' + 'c' * 65 + ' id 0123456789abcdef0123456789abcdef',
        'path $1 \\1 C:\\Users\\patricia\\$x', ''], FAKE_ME],
        [['admin on WINDOWS as user, pc ab, home C:\\Users\\admin\\x'], {'USERNAME': 'admin', 'COMPUTERNAME': 'ab',
                                                                          'USERPROFILE': 'C:\\Users\\admin'}]]),
    'core_lines': (report.core_lines, 'src/report.py', [[[
        '2026-11-18 10:00:00 ERROR [1234 5.2ms] outbound/vless[vless-1]: dial tcp 1.1.1.1:443->x: i/o timeout\n',
        '2026-11-18 10:00:05 ERROR [99 1m2.5s] outbound/vless[vless-1]: dial tcp 1.1.1.1:5555->x: i/o timeout\n',
        'not a log line\n', '2026-11-18 10:01:00 WARN [7 300µs] something else\n',
        '2026-11-18 10:02:00 ERROR [1 2ns] outbound/vless[vless-1]: dial tcp 1.1.1.1:80->x: i/o timeout\n',
        '2026-11-18 10:03:00 INFO port :8->y kept\n']], [[]]]),
    'issue_url': (report.issue_url, 'src/report.py', [
        ['C:\\x\\anonymous-logs\\report-20261118-120000.txt', 'Reality Hop problem report\nSetup\n-----\nä & = + ? # /\n\nWatcher log (watch.log)\nlots'],
        ['report.txt', 'x' * 3500]]),
    't': (t, 'src/i18n.py', [[lang, mac, text, kw] for lang, mac, text, kw in (
        ('en', False, 'Restart PC tunnel', {}), ('es', False, 'Restart PC tunnel', {}), ('fa', False, 'Restart PC tunnel', {}),
        ('ar', False, 'For when every node is dead and nothing else works.\n\nThis PC tests the nodes in the vault (every node that ever passed a test), puts the best 6 into the configs, and updates the tunnel(s). If too few work, it scans the node list for more.', {}),
        ('es', False, '{ok} of {n} candidates answer on 443 from this PC. None is tunnel-tested yet.', {'ok': 3, 'n': 50}),
        ('ru', False, 'next auto-check {at}', {'at': '12:30'}), ('zh', False, 'answers on 443 in {ms} ms; not tunnel-tested yet', {'ms': 42}),
        ('en', False, 'not in any catalog {x} {{literal}}', {'x': 'PC'}), ('en', False, 'missing {y}', {'y': None}),
        ('en', True, 'Restart PC tunnel', {}), ('tr', True, 'Restart PC tunnel', {}),
        ('tr', True, "The router's tunnel tests every node. If another PC changed the router, its nodes are taken first.", {}),
        ('es', True, 'Opens the install folder with the PC, Router and logs folders.', {}), ('vi', True, 'Restart PC tunnel', {}),
        ('uk', True, 'Restart PC tunnel', {}), ('fa', True, 'Restart PC tunnel', {}), ('en', True, 'another PC {n}', {'n': 'PC'}),
        ('es', False, '', {}), ('fa', False, 'line one\n\n  \nline two', {}))]),
    'mac_words': (i18n.mac_words, 'src/i18n.py', [[s, l] for s, l in (
        ('PC tunnel on this PC, PC, Router and logs', 'en'), ('another PC and PCs and PC.', 'en'), ("Başka bir PC'den PC'ye PC'nin", 'tr'),
        ("başka bir PC PC'deki PC'de PC'yi", 'tr'), ('otro PC y PC ، Router', 'es'), ('другой ПК и ПК', 'ru'), ('máy PC khác PC', 'vi'),
        ('PC', 'zh'), ('PC 、 Router', 'en'))]),
    'tray_mark': (tray_mark, 'src/engine.py', [
        [{'vless-1': 50, 'vless-2': 250, 'vless-3': None}, None], [{'vless-1': 50, 'vless-2': 120}, None],
        [{'vless-1': None, 'vless-2': None}, None], [{'vless-1': 0, 'vless-2': 300}, ['vless-2']], [{}, None]]),
    'pia_bypass': (pia_bypass, 'src/engine.py', [
        [{}, {}, False, [], [], False, True],
        [{'splitTunnelRules': [{'path': 'c:/program files/RealityHop/PC/RH-CORE.EXE', 'linkTarget': '', 'mode': 'include'},
                               {'path': ntpath.join('C:' + ntpath.sep, 'Games', 'game.exe'), 'linkTarget': '', 'mode': 'exclude'}]}, {}, False, [], [], False, True],
        [{'splitTunnelRules': [{'path': RH_CORE, 'linkTarget': '', 'mode': 'exclude'},
                               {'path': RH_TEST, 'linkTarget': '', 'mode': 'exclude'}]},
         {'pia_apps': [RH_CORE]}, False, [], [], False, True],
        [{'splitTunnelRules': [{'path': RH_CORE, 'linkTarget': '', 'mode': 'exclude'},
                               {'path': RH_TEST, 'linkTarget': '', 'mode': 'exclude'}, 'junk']},
         {'pia_apps': [RH_CORE]}, False, [], [], False, False],
        [{}, {'pia_apps': ['x']}, False, [], [], False, True, True],
        [{'bypassSubnets': [{'subnet': '8.8.8.8/32', 'mode': 'exclude'}, {'subnet': '5.5.5.5/32', 'mode': 'exclude'}, 'junk'],
          'splitTunnelEnabled': False}, {'pia_ips': ['5.5.5.5']}, True, ['8.8.8.8', '1.1.1.1'],
         ['100.0.0.1', '9.9.9.9', '10.0.0.1'], True, True],
        [{'bypassSubnets': [{'subnet': '1.1.1.1/32', 'mode': 'exclude'}],
          'splitTunnelRules': [{'path': '/Applications/RealityHop.app', 'linkTarget': '', 'mode': 'exclude'}],
          'splitTunnelEnabled': True}, {'pia_ips': ['1.1.1.1'], 'pia_apps': ['/Applications/RealityHop.app']}, True,
         ['1.1.1.1'], [], True, True],
        [{'bypassSubnets': [{'subnet': '1.1.1.1/32', 'mode': 'exclude'}],
          'splitTunnelRules': [{'path': '/Applications/RealityHop.app', 'linkTarget': '', 'mode': 'exclude'}],
          'splitTunnelEnabled': True}, {'pia_ips': ['1.1.1.1'], 'pia_app': True}, True, ['1.1.1.1'], [], True, False],
        [{}, {'pia_app': True}, True, ['1.1.1.1'], [], False, True],
        [None, {}, False, [], [], False, True],
        [[1, 2], {}, False, [], [], False, True],
    ]),
    'core_release': (core_release, 'src/engine.py', [
        [REL, 'stable'], [REL, 'testing'], [REL[:1] + REL[2:3], 'stable'], [[], 'stable'],
        [[REL[1], dict(REL[1], tag_name='1.14.4', body='second')], 'stable'],
    ]),
    'vpn_state': (vpn_state, 'src/engine.py', [[None], [{'method': 'openvpn', 'protocol': 'tcp', 'remotePortTCP': 443,
        'proxyType': 'custom', 'proxyCustom': {'host': '127.0.0.1', 'port': 2080}}],
        [{'method': 'wireguard', 'protocol': 'udp', 'remotePortTCP': 8443, 'proxyType': 'shadowsocks', 'proxyEnabled': False}],
        [{'method': 'openvpn', 'protocol': 'tcp', 'remotePortTCP': 443, 'proxyType': 'custom', 'proxyEnabled': False,
          'proxyCustom': {'host': '127.0.0.1', 'port': 2080}}], [{'proxyCustom': None}]]),
    'arp_windows': (arp_windows, 'src/engine.py', [
        ['\r\nInterface: 192.168.8.20 --- 0x5\r\n  Internet Address      Physical Address      Type\r\n  192.168.8.1           94-83-C4-1A-2B-3C     dynamic\r\n'],
        ['No ARP Entries Found.'], ['xx94-83-c4-1a-2b-3c-dd']]),
    'mac_from_arp': (macos.mac_from_arp, 'src/macos.py', [
        ['? (192.168.8.1) at 94:83:c4:1a:2b:3c on en0 ifscope [ethernet]'], ['? (192.168.8.1) at 0:1A:2b:3:4c:5d on en0'],
        ['? (192.168.8.1) at (incomplete) on en0'], ['']]),
    'dns_proxies': (dns_proxies, 'src/macos.py', [
        ['resolver #1\n  nameserver[0] : 127.0.0.1\n  nameserver[1] : 192.0.2.42\nresolver #2\n  nameserver[0] : 9.9.9.9\n  nameserver[0]: ::1\n  nameserver[3] :127.0.0.1\n'],
        ['']]),
    'agent_plist': (agent_plist, 'src/macos.py', [
        ['com.realityhop.watcher', ['/Applications/RealityHop.app/Contents/MacOS/RealityHop', '--watch', '--logon'],
         '/Users/pat/Library/Application Support/RealityHop'],
        ['com.realityhop.tunnel', ['/x/rh-core', 'run', '-c', "/x/a&b <c> \"d\" 'e'.json"], '/x']]),
    'settings_save': (settings_save, 'src/engine.py', [
        [{}, {'tolerance_ms': 40, 'check_min': '30', 'nope': 1}, PC_NODES, ROUTER_NODES],
        [{'top': 60, 'lang': 'es'}, {'tolerance_ms': 5000, 'devices': '192.168.8.10'}, PC_NODES, ROUTER_NODES],
        [{'tolerance_ms': 25}, {'router': True, 'auto_update': 0}, PC_NODES, ROUTER_NODES]]),
    'hist_add': (hist_add, 'src/engine.py', [
        [None, {'8.8.8.8': 50, '1.1.1.1': None}, None],
        [{'pings': {'8.8.8.8': [[int(NOW) - 200000, 40], [int(NOW) - 100, 41]], '9.9.9.9': [[int(NOW) - 200000, 3]]},
          'events': [[int(NOW) - 200000, 'drop', 'old'], [int(NOW) - 10, 'back', 'new']]}, {'8.8.8.8': 42}, ['replace', 'vless-1 now 1.1.1.1']],
        [{'pings': {}, 'events': [[int(NOW) - 5, 'x', str(i)] for i in range(2005)]}, None, ['restart', 'r']],
        [{'other': 1}, None, None]]),
    'speed_test': (speed_test, 'src/engine.py', [
        [SPEED_NODES, SPEED_TABLE, 100, None],
        [SPEED_NODES, SPEED_TABLE, 100, [NOW - 100, 60]],
        [SPEED_NODES, {k: [80 if v[0] == 150 else v[0], v[1]] for k, v in SPEED_TABLE.items()}, 100, [NOW - 90000, 300]],
        [SPEED_NODES, SPEED_TABLE, 100, [NOW - 100, 500]],
        [[{'server': '203.0.113.1', 'ok': False, 'ms': None}], {}, 100, None],
        [SPEED_MANY, {str(23000 + i): [70 + i % 4, 150 - i] for i in range(30)}, 100, None],
        [SPEED_MANY, {str(23000 + i): [None, None] for i in range(30)}, 100, None]]),
    'router_sync': (router_sync, 'src/engine.py', [
        [sync_reply(ROUTER_OTHER, md5='a' * 32), {'router_md5': 'a' * 32}, VAULT, '', ROUTER_NODES, PC_NODES],
        ['', {}, VAULT, '', ROUTER_NODES, PC_NODES],
        [sync_reply(ROUTER_OTHER, devs='192.168.8.10 192.168.8.11'), {'router_md5': 'a' * 32}, SYNC_VAULT, '', ROUTER_NODES, PC_NODES],
        [sync_reply(ROUTER_OTHER, devs='all', heal='2026-11-18 vless-2 swapped'), {}, SYNC_VAULT, '192.168.8.20', ROUTER_NODES, PC_NODES],
        [sync_reply(ROUTER_NODES, devs='bad; rm -rf /'), {'router_md5': 'b' * 32}, VAULT, '192.168.8.20', ROUTER_NODES, PC_NODES],
        [sync_reply(ROUTER_BAD), {}, VAULT, '', ROUTER_NODES, PC_NODES],
        [sync_reply(ROUTER_BADKEY), {}, VAULT, '', ROUTER_NODES, PC_NODES],
        [sync_reply(ROUTER_NOAPI), {}, VAULT, '', ROUTER_NODES, PC_NODES],
        [sync_reply('{"outbounds": [}'), {}, VAULT, '', ROUTER_NODES, PC_NODES],
        [sync_reply({'outbounds': [], 'experimental': {'clash_api': {'secret': 'ab' * 16}}}), {}, VAULT, '', ROUTER_NODES, PC_NODES]]),
    'key_console': (key_console, 'src/macos.py', [
        ['/usr/bin/ssh', '192.168.8.1', 'mkdir -p /etc/dropbear; echo ssh-ed25519 AAAA realityhop >> x'],
        ["/Users/O'Brien/my ssh", 'router.lan', "it's"], ['/usr/bin/ssh', 'gl-mt6000', '']]),
    'vtuple': (install.vtuple, 'src/install.py', [[v] for v in (
        '1.7.2', 'v1.7.2', '1.7', '', None, '1.2.3.4', 'abc', '2.0.0-beta.1', '10.20.30')]),
}

# One call as {"args", "out"}; an exception is recorded as {"error": its type}
def run(fn, args):
    try:
        return {'args': args, 'out': json.loads(json.dumps(fn(*args)))}
    except Exception as e:
        return {'args': args, 'out': {'error': type(e).__name__}}

# The fixture text for one function
def fixture(name, fn, source, calls):
    return json.dumps({'fn': name, 'source': source, 'cases': [run(fn, a) for a in calls]}, indent=1, ensure_ascii=False) + '\n'

# Write every fixture, or with --check list the ones that differ
def main():
    check, stale = '--check' in sys.argv[1:], []
    os.makedirs(OUT, exist_ok=True)
    for name, (fn, source, calls) in CASES.items():
        path, text = os.path.join(OUT, name + '.json'), fixture(name, fn, source, calls)
        leak = [x for x in PRIVATE if x.lower() in text.lower() or json.dumps(x)[1:-1].lower() in text.lower()]
        if leak:
            raise SystemExit(f'{name}: the fixture holds a local path or user name; fix the case before writing')
        old = open(path, encoding='utf-8').read() if os.path.exists(path) else None
        if old == text:
            continue
        stale.append(name)
        if not check:
            with open(path, 'w', encoding='utf-8', newline='\n') as f:
                f.write(text)
    shutil.rmtree(SANDBOX, ignore_errors=True)
    if check and stale:
        print('Fixtures differ from the Python code: ' + ', '.join(stale) + '. Run python tools/parity-dump.py')
        return 1
    print(('Up to date: ' if check else 'Written: ') + (', '.join(stale) if stale and not check else f'{len(CASES)} fixtures'))
    return 0

if __name__ == '__main__':
    sys.exit(main())
