"""check-backend.py  -  refuses a release while the build backend is out of date.

Checks the GitHub Actions in .github/workflows (newest major version), the Python version the release is built with
(the newest one that PyInstaller, Pillow and setup-python all support), and any == pins in requirements.txt.
Runs first in the release workflow, and before a v* tag is pushed (.githooks/pre-push). Exit code 1 = out of date.
Standard library only.
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


def major(tag):
    m = re.match(r'v?(\d+)', tag)
    return int(m.group(1)) if m else 0


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
            report(major(tag) >= major(latest) and real == sha, repo, f'uses {tag}, newest {latest}' + ('' if real == sha else f', but {tag} is commit {real}'))


def python():
    wf = open(os.path.join(ROOT, '.github', 'workflows', 'release.yml'), encoding='utf-8').read()
    used = re.search(r"python-version:\s*'?([\d.]+)", wf).group(1)
    pyi, pil = get('https://pypi.org/pypi/pyinstaller/json')['info'], get('https://pypi.org/pypi/pillow/json')
    wheels = {f['filename'] for f in pil['urls']}
    builds = {v['version'] for v in get('https://raw.githubusercontent.com/actions/python-versions/main/versions-manifest.json')
              if v.get('stable')}
    below = re.search(r'<\s*([\d.]+)', pyi.get('requires_python') or '')
    for c in get('https://endoflife.date/api/python.json'):   # newest first
        cy = c['cycle']; tag = 'cp' + cy.replace('.', '')
        if below and ver(cy) >= ver(below.group(1)):
            continue
        if any(f'-{tag}-{tag}-win_amd64' in w for w in wheels) and any(b.startswith(cy + '.') for b in builds):
            report(used == cy, 'Python for the build', f"release.yml uses {used}, newest supported {cy} "
                   f"(PyInstaller {pyi['version']}, Pillow {pil['info']['version']})")
            return
    report(False, 'Python for the build', 'no Python version found that PyInstaller, Pillow and setup-python all support')


def pins():
    for line in open(os.path.join(ROOT, 'requirements.txt'), encoding='utf-8'):
        m = re.match(r'\s*([\w.-]+)\s*==\s*([\w.]+)', line)
        if m:
            latest = get(f'https://pypi.org/pypi/{m.group(1)}/json')['info']['version']
            report(ver(m.group(2)) >= ver(latest), m.group(1), f'pinned {m.group(2)}, newest {latest}')


for check in (actions, python, pins):
    try:
        check()
    except Exception as e:
        report(False, check.__name__, f'could not check ({e})')
if problems:
    print(f'\nBackend out of date ({", ".join(problems)}). Update it before releasing.')
    sys.exit(1)
print('\nBackend up to date.')
