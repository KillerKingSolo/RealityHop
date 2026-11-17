"""mac-app-test.py  -  checks a built RealityHop-mac-<chip>.dmg on a real Mac (the release workflow runs it on several
macOS versions). Usage: python tools/mac-app-test.py <dmg> <chip> <screenshot folder>

Everything runs with HOME set to fresh folders under the runner's temp folder: nothing touches the real home.
Exit code 1 = a check failed.
"""
import hashlib, os, plistlib, shutil, signal, subprocess, sys, tempfile, time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'src'))
import macos

dmg, chip, shots = os.path.abspath(sys.argv[1]), sys.argv[2], os.path.abspath(sys.argv[3])
os.makedirs(shots, exist_ok=True)
tmp, fails = tempfile.mkdtemp(prefix='rh-app-test-'), []
ARCH = {'apple-silicon': 'arm64', 'intel': 'x86_64'}[chip]


def check(what, ok, detail=''):
    print(f"{'PASS' if ok else 'FAIL'}  {what}" + (f': {detail}' if detail else ''), flush=True)
    if not ok:
        fails.append(what)


def sh(*a):
    return subprocess.run(a, capture_output=True, text=True)


def home(name, lang=None, dns='quad9'):
    h = os.path.join(tmp, name); os.makedirs(h)
    if lang:   # dns '': the France question has not been answered yet
        d = os.path.join(h, 'Library', 'Application Support', 'RealityHop'); os.makedirs(d)
        open(os.path.join(d, 'settings.json'), 'w').write(f'{{"lang": "{lang}", "dns": "{dns}"}}')
    return h


def launch(exe, h, shot, *args, wait=10):   # a start-up crash shows within seconds
    """Start the app with HOME=h, take a screenshot after wait seconds; True when it is still running then."""
    p = subprocess.Popen([exe, *args], env=dict(os.environ, HOME=h), stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    time.sleep(wait)
    sh('/usr/sbin/screencapture', '-x', os.path.join(shots, f'{chip}-{shot}.png'))
    alive = p.poll() is None
    if alive:
        p.send_signal(signal.SIGKILL)
    err = p.communicate()[1].decode(errors='replace')
    if 'Traceback' in err:
        print(err[-2000:])
    return alive and 'Traceback' not in err


def mount(path):
    """Mount like Finder does (under /Volumes), read-only. Returns the mount point."""
    r = sh('/usr/bin/hdiutil', 'attach', '-nobrowse', '-readonly', '-noautoopen', '-plist', path)
    pts = [e['mount-point'] for e in plistlib.loads(r.stdout.encode()).get('system-entities', []) if 'mount-point' in e]
    return pts[0]


# the disk image
check('dmg checksum readable', len(hashlib.sha256(open(dmg, 'rb').read()).hexdigest()) == 64)
vol = mount(dmg)
check('dmg mounts under /Volumes', vol.startswith('/Volumes/'), vol)
check('dmg holds RealityHop.app', os.path.isfile(macos.app_exe(os.path.join(vol, 'RealityHop.app'))))
check('dmg holds the Applications link', os.path.islink(os.path.join(vol, 'Applications')) and
      os.readlink(os.path.join(vol, 'Applications')) == '/Applications')
check('disk icon', os.path.getsize(os.path.join(vol, '.VolumeIcon.icns')) > 1000 and
      sh('/usr/bin/xattr', '-px', 'com.apple.FinderInfo', vol).stdout.split()[8:9] == ['04'])
src = os.path.join(vol, 'RealityHop.app'); exe = macos.app_exe(src)
check('chip', sh('/usr/bin/lipo', '-archs', exe).stdout.split() == [ARCH], sh('/usr/bin/lipo', '-archs', exe).stdout.strip())
check('signature', sh('/usr/bin/codesign', '--verify', '--deep', '--strict', src).returncode == 0)
info = plistlib.load(open(os.path.join(src, 'Contents', 'Info.plist'), 'rb'))
check('bundle id', info.get('CFBundleIdentifier') == 'com.realityhop.app', info.get('CFBundleIdentifier'))
check('minimum macOS 15', info.get('LSMinimumSystemVersion') == '15.0', info.get('LSMinimumSystemVersion'))
check('version', bool(info.get('CFBundleShortVersionString')), info.get('CFBundleShortVersionString'))
print(f"macOS {sh('/usr/bin/sw_vers', '-productVersion').stdout.strip()}, {sh('/usr/bin/uname', '-m').stdout.strip()}")

# run straight from the disk image: it must ask to be moved to Applications, and write nothing
h = home('from-dmg')
check('from the dmg: a message, still open', launch(exe, h, 'from-dmg', wait=8))
check('from the dmg: no login item, no data folder', not os.path.exists(os.path.join(h, 'Library')))

apps = os.path.join(tmp, 'Applications'); os.makedirs(apps)
app = os.path.join(apps, 'RealityHop.app')
check('copy like a drag to Applications', sh('/usr/bin/ditto', src, app).returncode == 0)
sh('/usr/bin/hdiutil', 'detach', vol, '-force')
exe = macos.app_exe(app)

check('first start: language picker', launch(exe, home('first'), 'language'))
check('first setup: France question', launch(exe, home('france', 'en', ''), 'france'))
h = home('wizard', 'en')   # with an rh-core signed by someone else, as sing-box from GitHub is ("a.out")
core = os.path.join(h, 'Library', 'Application Support', 'RealityHop', 'PC', 'rh-core'); os.makedirs(os.path.dirname(core))
shutil.copy('/bin/echo', core)
check('setup wizard', launch(exe, h, 'wizard'))
ident = [l for l in sh('/usr/bin/codesign', '-dv', core).stderr.splitlines() if l.startswith('Identifier=')]
check("the app signs rh-core with Reality Hop's id at start", ident == ['Identifier=' + macos.CORE_ID], str(ident))
check('setup wizard in Persian (right to left)', launch(exe, home('wizard-fa', 'fa'), 'wizard-fa'))
check('watcher with menu bar icon', launch(exe, home('watcher', 'en'), 'watcher', '--watch'))

# self-update: the same dmg as the "new release", into the copy in Applications
data = os.path.join(tmp, 'update-data'); os.makedirs(data)
blob = open(dmg, 'rb').read()
rel = {'ver': 'test', 'asset': {'browser_download_url': 'file://' + dmg, 'size': len(blob),
                                'digest': 'sha256:' + hashlib.sha256(blob).hexdigest()}}
before = os.stat(exe).st_ino
macos.apply_update(rel, data, lambda url, t: blob, print, lambda: None, app=app)
check('update: app replaced', os.path.isfile(exe) and os.stat(exe).st_ino != before)
olds = [f for f in os.listdir(apps) if f.startswith('.RealityHop.old-')]
check('update: old copy kept hidden until it has exited', len(olds) == 1, str(olds))
check('update: no leftovers', not os.path.exists(app + '.new') and not os.path.exists(os.path.join(data, 'update.dmg')))
check('update: signature', sh('/usr/bin/codesign', '--verify', '--deep', '--strict', app).returncode == 0)
check('update: new copy starts', launch(exe, home('after-update', 'en'), 'after-update'))
os.utime(os.path.join(apps, olds[0]), (time.time() - 3600, time.time() - 3600))
macos.cleanup(app)
check('update: old copy removed later', not [f for f in os.listdir(apps) if f.startswith('.RealityHop.')])
bad = dict(rel, asset=dict(rel['asset'], digest='sha256:' + '0' * 64)); before = os.stat(exe).st_ino
try:
    macos.apply_update(bad, data, lambda url, t: blob, print, lambda: None, app=app); refused = False
except RuntimeError as e:
    refused = 'checksum' in str(e)
check('update: wrong checksum refused, app untouched', refused and os.stat(exe).st_ino == before)

# the router key window: a script for Terminal (not opened here)
opened = []
real = macos.subprocess.Popen
macos.subprocess.Popen = lambda a, **k: opened.append(a)
macos.key_console(data, '/usr/bin/ssh', '192.168.8.1', "echo ssh-ed25519 AAAA realityhop >> /etc/dropbear/authorized_keys; echo 'Key added.'")
macos.subprocess.Popen = real
cmd = os.path.join(data, 'add-router-key.command')
check('router key script: valid shell', sh('/bin/sh', '-n', cmd).returncode == 0)
check('router key script: only this user', oct(os.stat(cmd).st_mode & 0o777) == '0o700')
check('router key script: opened in Terminal', opened and opened[0][:3] == ['/usr/bin/open', '-a', 'Terminal'])

shutil.rmtree(tmp, ignore_errors=True)
print(f'{len(fails)} failed' if fails else 'all passed')
sys.exit(1 if fails else 0)
