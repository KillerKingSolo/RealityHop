"""check-backend.py  -  refuses a release while the build backend is out of date.

Checks the GitHub Actions in .github/workflows (newest release), the Rust toolchain in rust/rust-toolchain.toml (newest
stable), the exact (=) crate pins in rust/Cargo.toml and rust/*/Cargo.toml (newest on crates.io) and the cargo-deny
version the CI installs. Runs first in the release workflow, and before a v* tag is pushed (.githooks/pre-push).
Exit code 1 = out of date. Standard library only.
"""
import glob, json, os, re, sys, urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
problems = []


def get(url):
    h = {'User-Agent': 'RealityHop-check-backend', 'Accept': 'application/vnd.github+json'}
    if os.environ.get('GITHUB_TOKEN') and 'api.github.com' in url:
        h['Authorization'] = 'Bearer ' + os.environ['GITHUB_TOKEN']
    with urllib.request.urlopen(urllib.request.Request(url, headers=h), timeout=30) as r:
        return json.load(r)


def report(ok, what, detail):
    print(f"{'OK      ' if ok else 'OUTDATED'}  {what}: {detail}")
    if not ok:
        problems.append(what)


def ver(s):
    return tuple(int(x) for x in re.findall(r'\d+', s)[:3])


def actions():
    seen = set()
    for wf in glob.glob(os.path.join(ROOT, '.github', 'workflows', '*.yml')):
        # pinned to a commit, with the version it is in a comment: a moved tag cannot change what the release runs
        for repo, sha, tag in re.findall(r'uses:\s*([\w.-]+/[\w.-]+)@([\w.-]+)[ \t]*#[ \t]*(v[\w.-]+)', open(wf, encoding='utf-8').read()):
            if (repo, tag) in seen:
                continue
            seen.add((repo, tag))
            latest = get(f'https://api.github.com/repos/{repo}/releases/latest')['tag_name']
            real = get(f'https://api.github.com/repos/{repo}/commits/{tag}')['sha']
            report(ver(tag) >= ver(latest) and real == sha, repo, f'uses {tag}, newest {latest}' + ('' if real == sha else f', but {tag} is commit {real}'))


def text(url):
    req = urllib.request.Request(url, headers={'User-Agent': 'RealityHop-check-backend'})
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read().decode()


def toolchain():
    used = re.search(r'channel\s*=\s*"([\d.]+)"', open(os.path.join(ROOT, 'rust', 'rust-toolchain.toml')).read()).group(1)
    stable = re.search(r'\[pkg\.rust\]\s*version\s*=\s*"([\d.]+)', text('https://static.rust-lang.org/dist/channel-rust-stable.toml')).group(1)
    report(ver(used) >= ver(stable), 'Rust toolchain', f'rust-toolchain.toml uses {used}, newest stable {stable}')


def crates():
    seen = set()
    for f in [os.path.join(ROOT, 'rust', 'Cargo.toml')] + sorted(glob.glob(os.path.join(ROOT, 'rust', '*', 'Cargo.toml'))):
        for name, pin in re.findall(r'^([\w-]+)\s*=\s*(?:\{[^}\n]*?version\s*=\s*)?"=([\w.-]+)"', open(f, encoding='utf-8').read(), re.M):
            if name in seen:
                continue
            seen.add(name)
            latest = get(f'https://crates.io/api/v1/crates/{name}')['crate']['max_stable_version']
            report(ver(pin) >= ver(latest), name, f'pinned {pin}, newest {latest}')


def tools():
    for wf in glob.glob(os.path.join(ROOT, '.github', 'workflows', '*.yml')):
        for name, pin in re.findall(r'cargo install --locked ([\w-]+)@([\d.]+)', open(wf, encoding='utf-8').read()):
            latest = get(f'https://crates.io/api/v1/crates/{name}')['crate']['max_stable_version']
            report(ver(pin) >= ver(latest), name, f'{os.path.basename(wf)} installs {pin}, newest {latest}')


for check in (actions, toolchain, crates, tools):
    try:
        check()
    except Exception as e:
        report(False, check.__name__, f'could not check ({e})')
if problems:
    print(f'\nBackend out of date ({", ".join(problems)}). Update it before releasing.')
    sys.exit(1)
print('\nBackend up to date.')
