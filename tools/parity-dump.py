"""parity-dump.py  -  records what the Python code returns, as fixtures for the Rust port (rust/parity/<fn>.json).

Runs pure functions only. HOME, USERPROFILE, APPDATA and LOCALAPPDATA point at an empty temp folder first, so the
import reads and writes nothing of a real install. Windows only (engine.py reads the registry on import).
  python tools/parity-dump.py           write the fixtures
  python tools/parity-dump.py --check   exit 1 if a fixture differs from what the Python code returns now
"""
import json, os, shutil, sys, tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, 'rust', 'parity')
SANDBOX = tempfile.mkdtemp(prefix='rh-parity-')
for k in ('HOME', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA'):
    os.environ[k] = SANDBOX
sys.path.insert(0, os.path.join(ROOT, 'src'))
import engine as N, install
N.set_paths(SANDBOX)

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
