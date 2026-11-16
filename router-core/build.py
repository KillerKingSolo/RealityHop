"""build.py - Reality Hop's router build of sing-box: the official source with everything the router does not use
left out (see README.md). The same source, Go version and flags give the same file, so anyone can check a release.

  python build.py                 build every channel in version.json for linux/arm64 into out/, with manifests
  python build.py --verify        the same, and fail unless each SHA-256 matches version.json
  python build.py --newest        print the newest sing-box (stable, preview) and Go versions as JSON
  python build.py --update        move version.json to the newest versions, build, write the new SHA-256
  python build.py --pin           build the versions in version.json and write their SHA-256 (after an overlay change)
  python build.py --test CHANNEL  build linux/amd64 and run it with the test configs (Linux; tproxy runs through sudo)
  python build.py --plan          CI: print go=<version> and new=yes|no (a newer sing-box or Go than version.json)

Needs git and the Go version in version.json on PATH. Standard library only.
"""
import hashlib, json, os, re, shutil, subprocess, sys, tempfile, time, urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, 'out')
VERSIONS = os.path.join(HERE, 'version.json')
UPSTREAM = 'https://github.com/SagerNet/sing-box.git'
TAGS = ['with_utls', 'with_clash_api']   # uTLS for Reality, the Clash API that Reality Hop reads the router through
CMD_KEEP = {'main.go', 'cmd.go', 'cmd_check.go', 'cmd_run.go', 'cmd_run_userns_linux.go', 'cmd_run_userns_other.go',
            'cmd_netns_holder.go', 'cmd_version.go'}
INCLUDE_KEEP = {'clashapi.go', 'clashapi_stub.go'}
BOX_EDITS = [   # box.go: no cache file (bbolt); exact text, so a changed upstream stops the build instead of guessing
    ('\t"github.com/sagernet/sing-box/experimental/cachefile"\n', ''),
    ('\t\tcacheFile := cachefile.New(ctx, logFactory.NewLogger("cache-file"), common.PtrValueOrDefault(experimentalOptions.CacheFile))\n'
     '\t\tservice.MustRegister[adapter.CacheFile](ctx, cacheFile)\n'
     '\t\tinternalServices = append(internalServices, cacheFile)\n',
     '\t\treturn nil, E.New("cache file not in this build")\n'),
]
ARCHES = {'arm64': 'arm64'}   # router `uname -m` aarch64 -> GOARCH


def asset(arch):
    return f'rh-router-core-linux-{arch}'


def get(url):
    h = {'User-Agent': 'RealityHop-router-core', 'Accept': 'application/vnd.github+json'}
    if os.environ.get('GITHUB_TOKEN') and 'api.github.com' in url:
        h['Authorization'] = 'Bearer ' + os.environ['GITHUB_TOKEN']
    with urllib.request.urlopen(urllib.request.Request(url, headers=h), timeout=30) as r:
        return json.load(r)


def vkey(tag):
    """1.13.0-alpha.2 < 1.13.0-beta.1 < 1.13.0-rc.1 < 1.13.0 (as the app sorts them)."""
    m = re.match(r'v?(\d+)\.(\d+)\.(\d+)(?:-(alpha|beta|rc)\.?(\d+)?)?', tag or '')
    if not m:
        return (0,)
    return int(m[1]), int(m[2]), int(m[3]), {'alpha': 0, 'beta': 1, 'rc': 2}.get(m[4], 3), int(m[5] or 0)


def newest():
    rels = [r for r in get('https://api.github.com/repos/SagerNet/sing-box/releases?per_page=40') if not r.get('draft')]
    stable = max((r for r in rels if not r.get('prerelease')), key=lambda r: vkey(r['tag_name']))
    preview = max(rels, key=lambda r: vkey(r['tag_name']))   # the newest of all, as the PC's testing channel
    go = next(g['version'] for g in get('https://go.dev/dl/?mode=json') if g.get('stable'))
    return {'go': go.removeprefix('go'), 'stable': stable['tag_name'].lstrip('v'), 'preview': preview['tag_name'].lstrip('v')}


def run(cmd, **kw):
    print('+', ' '.join(cmd), flush=True)
    return subprocess.run(cmd, check=True, **kw)


def go_check(want):
    have = subprocess.run(['go', 'env', 'GOVERSION'], capture_output=True, text=True, check=True).stdout.strip()
    if have != 'go' + want:
        sys.exit(f'Go {want} is needed (version.json), but PATH has {have}')


def prepare(ver, d):
    """The official source at tag v<ver>, cut down to the router build."""
    run(['git', '-c', 'advice.detachedHead=false', 'clone', '-q', '--depth', '1', '--branch', 'v' + ver, UPSTREAM, d])
    commit = subprocess.run(['git', '-C', d, 'rev-parse', 'HEAD'], capture_output=True, text=True, check=True).stdout.strip()
    for sub, keep in (('cmd/sing-box', CMD_KEEP), ('include', INCLUDE_KEEP), ('transport/v2ray', set())):
        for f in os.listdir(os.path.join(d, sub)):
            if f.endswith('.go') and f not in keep:
                os.remove(os.path.join(d, sub, f))
    for root, _, files in os.walk(os.path.join(HERE, 'overlay')):
        for f in files:
            rel = os.path.relpath(os.path.join(root, f), os.path.join(HERE, 'overlay'))
            shutil.copyfile(os.path.join(root, f), os.path.join(d, rel))
    p = os.path.join(d, 'box.go'); s = open(p, encoding='utf-8').read()
    for old, new in BOX_EDITS:
        if s.count(old) != 1:
            sys.exit(f'box.go in sing-box {ver} changed; update BOX_EDITS in build.py:\n{old}')
        s = s.replace(old, new)
    open(p, 'w', encoding='utf-8', newline='\n').write(s)
    return commit


def build(ver, goarch, out, goos='linux'):
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, 'src'); commit = prepare(ver, src)
        # the official release flags (Makefile, release/LDFLAGS) and its non-feature tags (badlinkname, ...)
        ld = open(os.path.join(src, 'release', 'LDFLAGS'), encoding='utf-8').read().strip()
        extra = [t for t in open(os.path.join(src, 'release', 'DEFAULT_BUILD_TAGS_OTHERS'), encoding='utf-8').read().strip().split(',')
                 if t and not t.startswith('with_')]
        env = dict(os.environ, CGO_ENABLED='0', GOOS=goos, GOARCH=goarch, GOTOOLCHAIN='local')
        run(['go', 'build', '-trimpath', '-tags', ','.join(TAGS + extra),
             '-ldflags', f"-X 'github.com/sagernet/sing-box/constant.Version={ver}' {ld} -s -w -buildid=",
             '-o', os.path.abspath(out), './cmd/sing-box'], cwd=src, env=env)
    return commit, hashlib.sha256(open(out, 'rb').read()).hexdigest()


def build_all(v, verify):
    go_check(v['go']); bad = []
    for ch in ('stable', 'preview'):   # out/<channel>/: the files of the rolling release router-core-<channel>
        c = v[ch]; os.makedirs(os.path.join(OUT, ch), exist_ok=True)
        for arch, goarch in ARCHES.items():
            out = os.path.join(OUT, ch, asset(arch))
            commit, sha = build(c['singbox'], goarch, out)
            print(f'{ch} {c["singbox"]} {arch}: {sha} ({os.path.getsize(out)} bytes)')
            if verify and c['sha256'].get(arch) != sha:
                bad.append(f'{ch} {arch}: built {sha}, version.json has {c["sha256"].get(arch)}')
            c['sha256'][arch] = sha
            json.dump({'channel': ch, 'singbox': c['singbox'], 'go': v['go'], 'source_commit': commit, 'arch': arch,
                       'asset': asset(arch), 'sha256': sha, 'size': os.path.getsize(out)},
                      open(os.path.join(OUT, ch, f'manifest-{arch}.json'), 'w'), indent=2)
    if bad:
        sys.exit('Not the expected build:\n' + '\n'.join(bad))
    return v


def dns_query(name, port):
    """Plain UDP DNS to 127.0.0.1:port; the IPv4 addresses in the answer."""
    import socket, struct
    q = b''.join(bytes([len(p)]) + p.encode() for p in name.split('.')) + b'\0' + struct.pack('>HH', 1, 1)
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.settimeout(10)
    s.sendto(struct.pack('>HHHHHH', 0x5248, 0x0100, 1, 0, 0, 0) + q, ('127.0.0.1', port)); d = s.recv(2048); s.close()
    i, ips = 12 + len(q), []
    for _ in range(struct.unpack('>H', d[6:8])[0]):
        t, _, _, n = struct.unpack('>HHIH', d[i + 2:i + 12]); i += 12
        if t == 1:
            ips.append('.'.join(map(str, d[i:i + 4])))
        i += n
    return ips


def test(ch):
    """Linux CI: the amd64 build must accept both test configs, run the router config (tproxy: root, through sudo),
    answer on its API and resolve through the DNS inbound. The test nodes are fake, so DoH goes out directly here."""
    v = json.load(open(VERSIONS)); go_check(v['go'])
    exe = os.path.join(OUT, 'test-amd64'); os.makedirs(OUT, exist_ok=True)
    build(v[ch]['singbox'], 'amd64', exe)
    for cfg in ('test-router.json', 'test-nodes.json'):
        run([exe, 'check', '-c', os.path.join(HERE, cfg)])
    # the node test's config has no DNS section: it must start too (check alone passed a build without local DNS)
    p = subprocess.Popen([exe, 'run', '-c', os.path.join(HERE, 'test-nodes.json')]); time.sleep(3)
    if p.poll() is not None:
        sys.exit('the node test config did not start')
    p.terminate(); p.wait()
    c = json.load(open(os.path.join(HERE, 'test-router.json')))
    c['dns']['servers'][0].pop('detour', None); c['route']['final'] = 'direct'
    cfg = os.path.join(OUT, 'test-run.json'); json.dump(c, open(cfg, 'w'))
    p = subprocess.Popen(([] if os.geteuid() == 0 else ['sudo', '-n']) + [exe, 'run', '-c', cfg])
    try:
        end = time.time() + 15
        while True:
            try:
                req = urllib.request.Request('http://127.0.0.1:9090/version', headers={'Authorization': 'Bearer test'})
                print('API:', urllib.request.urlopen(req, timeout=2).read().decode()); break
            except OSError:
                if p.poll() is not None or time.time() > end:
                    sys.exit('the router test config did not start')
                time.sleep(0.5)
        ips = dns_query('example.com', 5354); print('DNS through the hijack and Quad9 DoH:', ips)
        if not ips:
            sys.exit('no DNS answer through the DNS inbound')
    finally:
        subprocess.run(([] if os.geteuid() == 0 else ['sudo', '-n']) + ['pkill', '-f', 'test-amd64 run']); p.wait()


def main(a):
    if a[:1] == ['--newest']:
        print(json.dumps(newest())); return
    if a[:1] == ['--test']:
        test(a[1] if len(a) > 1 else 'stable'); return
    v = json.load(open(VERSIONS))
    is_new = lambda n: n['go'] != v['go'] or any(n[ch] != v[ch]['singbox'] for ch in ('stable', 'preview'))
    if a[:1] == ['--plan']:
        n = newest(); print(f"go={n['go'] if is_new(n) else v['go']}\nnew={'yes' if is_new(n) else 'no'}"); return
    if a[:1] == ['--update']:
        n = newest()
        if not is_new(n):
            print('already newest'); return
        v['go'] = n['go']
        for ch in ('stable', 'preview'):
            v[ch] = {'singbox': n[ch], 'sha256': {}}
    v = build_all(v, a[:1] == ['--verify'])
    if a[:1] in (['--update'], ['--pin']):
        open(VERSIONS, 'w', newline='\n').write(json.dumps(v, indent=2) + '\n'); print('version.json written')


if __name__ == '__main__':
    main(sys.argv[1:])
