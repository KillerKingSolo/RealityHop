"""mac-smoke.py  -  checks the macOS side from source, on a real Mac (the release workflow runs it on both chips).

Run with HOME pointed at an empty folder: everything it writes (data folder, login items) goes there, never into the
real home. It downloads sing-box and the public node list, tests nodes, starts and stops the PC tunnel, writes and removes
the login items, starts the watcher with its menu bar icon, and uninstalls. Exit code 1 = a check failed.
"""
import json, os, plistlib, subprocess, sys, time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'src'))
fails = []


def check(what, ok, detail=''):
    print(f"{'PASS' if ok else 'FAIL'}  {what}" + (f': {detail}' if detail else ''), flush=True)
    if not ok:
        fails.append(what)


home = os.path.expanduser('~')
if os.environ.get('RH_SMOKE_HOME') != home or os.listdir(home):
    sys.exit('set HOME and RH_SMOKE_HOME to the same empty test folder first')
import engine as N, install, macos, report

check('platform', N.MAC and N.EXE == '', sys.platform)
check('data folder in the test home', N.DATA == os.path.join(home, 'Library', 'Application Support', 'RealityHop'), N.DATA)
check('sing-box asset', N.core_asset('v1.14.4') == f'sing-box-1.14.4-darwin-{macos.ARCH}.tar.gz', N.core_asset('v1.14.4'))
check('arp parse', macos.mac_from_arp('? (192.168.8.1) at 94:83:c4:a:b:1c on en0 ifscope [ethernet]') == '94-83-c4-0a-0b-1c')
check('arp parse, no entry', macos.mac_from_arp('? (192.168.8.1) at (incomplete) on en0') == '')
check('release file', install.ASSET == macos.DMG, install.ASSET)
check('nothing to install', not install.needs_install() and not install.needs_move() and not install.stale_onefile())

res = N.setup_pc()   # data folder, configs, sing-box download and SHA-256 check, node scan, tunnel start
check('setup', any('running on 127.0.0.1:2080' in l for l in res), ' | '.join(res))
check('data folder only for this user', oct(os.stat(N.DATA).st_mode & 0o777) == '0o700', oct(os.stat(N.DATA).st_mode & 0o777))
check('sing-box runs', bool(N.core_version()), N.core_version() or 'none')
check('tunnel process found', N.core_running(), str(N.pids('rh-core')))
check('port owner', N.port_owner(2080) == 'rh-core', str(N.port_owner(2080)))
ip = N.pc_exit()
check('traffic through the tunnel', bool(ip), ip or 'no exit address')
check('restart', N.core_restart())
check('stop', N.core_stop() and not N.core_running())

check('login item for the tunnel', N.boot_task(True) and os.path.isfile(macos.agent_path(macos.TUNNEL)))
with open(macos.agent_path(macos.TUNNEL), 'rb') as f:
    check('login item content', plistlib.load(f)['ProgramArguments'][0] == N.CORE)
check('plist lint', subprocess.run(['/usr/bin/plutil', '-lint', macos.agent_path(macos.TUNNEL)]).returncode == 0)
check('task state ready', N.task_state() == 'Ready', str(N.task_state()))
out = N.manager_stop()
check('Stop switches the login start off', N.task_state() == 'Disabled' and N.stopped(), ' | '.join(out))
out = N.manager_start()
check('Start switches it on again', N.task_state() == 'Ready' and N.core_running(), ' | '.join(out))
time.sleep(8)
check('watcher running with its menu bar icon', N.port_taken(N.WATCH_PORT))
subprocess.run(['/usr/sbin/screencapture', '-x', os.path.join(os.environ.get('SHOTS', home), 'source-watcher.png')])
check('watcher log', os.path.isfile(os.path.join(N.LOGS, 'watch.log')))
N.notify('Reality Hop smoke test')

p = subprocess.Popen(['/bin/sleep', '60'])
check('process list', any(pid == p.pid and name == 'sleep' for pid, name in N.procs()))
N.kill_name('sleep'); time.sleep(0.5)
check('kill by name', p.poll() is not None)

text = report.Scrubber()(open(os.path.join(N.LOGS, 'watch.log'), encoding='utf-8').read() + ' ' + home)
check('report hides the home folder', home not in text)

out = install.uninstall(keep_data=False)
check('uninstall: login items gone', not os.path.exists(macos.agent_path(macos.TUNNEL)) and
      not os.path.exists(macos.agent_path(macos.WATCHER)), ' | '.join(out))
time.sleep(3)
check('uninstall: tunnel and watcher stopped', not N.core_running() and not N.port_taken(N.WATCH_PORT))
print(json.dumps({'fails': fails}))
sys.exit(1 if fails else 0)
