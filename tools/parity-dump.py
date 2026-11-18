"""parity-dump.py  -  records what the Python code returns, as fixtures for the Rust port (rust/parity/<fn>.json).

Runs pure functions only. HOME, USERPROFILE, APPDATA and LOCALAPPDATA point at an empty temp folder first, so the
import reads and writes nothing of a real install. Windows only (engine.py reads the registry on import).
  python tools/parity-dump.py           write the fixtures
  python tools/parity-dump.py --check   exit 1 if a fixture differs from what the Python code returns now
"""
import contextlib, datetime as _dt, json, os, shutil, sys, tempfile, time, types

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, 'rust', 'parity')
SANDBOX = tempfile.mkdtemp(prefix='rh-parity-')
for k in ('HOME', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA'):
    os.environ[k] = SANDBOX
sys.path.insert(0, os.path.join(ROOT, 'src'))
import engine as N, install
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
    N.time = types.SimpleNamespace(time=lambda: NOW, sleep=lambda s: None, perf_counter=time.perf_counter)
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
