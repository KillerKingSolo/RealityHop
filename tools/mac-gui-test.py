"""mac-gui-test.py  -  drives the real window code on a Mac (the release workflow runs it on both chips): every page of the
setup wizard, its Install and Finish actions, then the main window, Settings (save, check for updates), Stop and Start,
a problem report and Uninstall. The paths only the built app takes (install, updates) run with FROZEN on.

Run with HOME and RH_SMOKE_HOME set to the same empty folder; SHOTS = a folder for the screenshots.
Fails on any error in the logs that points at Windows-only code, and on any exception. Exit code 1 = a check failed.
"""
import importlib.machinery, importlib.util, os, re, subprocess, sys, time, traceback, urllib.request

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(HERE, 'src'))
home, shots, fails = os.path.expanduser('~'), os.environ.get('SHOTS', '/tmp'), []
if os.environ.get('RH_SMOKE_HOME') != home or os.listdir(home):
    sys.exit('set HOME and RH_SMOKE_HOME to the same empty test folder first')
WINDOWS_ONLY = re.compile(r"winreg|windll|WinDLL|powershell|schtasks|No module named|has no attribute|\.exe\b|"
                          r"CREATE_NO_WINDOW|DETACHED_PROCESS|STARTUPINFO|startfile|unsupported operand", re.I)


def check(what, ok, detail=''):
    print(f"{'PASS' if ok else 'FAIL'}  {what}" + (f': {detail}' if detail else ''), flush=True)
    if not ok:
        fails.append(what)


def guarded(what, fn, *a):
    try:
        r = fn(*a); check(what, True); return r
    except SystemExit:
        check(what, True, 'exit'); return None
    except Exception as e:
        check(what, False, f'{type(e).__name__}: {e}'); traceback.print_exc(); return None


def pump(root, secs, until=None):
    end = time.time() + secs
    while time.time() < end and not (until and until()):
        root.update(); time.sleep(0.05)


def shot(name):
    subprocess.run(['/usr/sbin/screencapture', '-x', os.path.join(shots, f'gui-{name}.png')])


loader = importlib.machinery.SourceFileLoader('realityhop', os.path.join(HERE, 'src', 'realityhop.pyw'))
R = importlib.util.module_from_spec(importlib.util.spec_from_loader('realityhop', loader)); loader.exec_module(R)
N, install = R.N, R.install
if os.environ.get('GITHUB_TOKEN'):   # test only: shared runners hit GitHub's anonymous API limit; the app never sends a token
    _get = N.gh_get

    def gh_get(url, timeout=30):
        if isinstance(url, str) and url.startswith('https://api.github.com/'):
            url = urllib.request.Request(url, headers={'Authorization': 'Bearer ' + os.environ['GITHUB_TOKEN']})
        elif isinstance(url, urllib.request.Request) and url.full_url.startswith('https://api.github.com/'):
            url.add_header('Authorization', 'Bearer ' + os.environ['GITHUB_TOKEN'])
        return _get(url, timeout)
    N.gh_get = gh_get
R.restart_window = lambda *a, **k: None   # the wizard's Finish would start a new window and end this process
answers = []
R.messagebox.askyesno = lambda *a, **k: answers.pop(0) if answers else True
R.messagebox.showinfo = R.messagebox.showerror = R.messagebox.showwarning = lambda *a, **k: None

# the setup wizard, every page, as the built app shows them
N.FROZEN = install.N.FROZEN = True
w = R.Wizard(); pump(w, 2)
w.use_router = True   # the router pages too (no router here: their actions log "unreachable", which is fine)
for page in w.seq():
    guarded(f'wizard page: {page}', w.show, page); pump(w, 3, lambda: not w.busy); shot(f'wizard-{page}')
guarded('install (frozen path): nothing to install on a Mac', lambda: install.install() == 'nothing to install' or 1 / 0)
w.use_router = False; N.settings_save({'router': 0})
guarded('wizard: Install', w.do_install, False, True, False, None); pump(w, 2); shot('wizard-installed')
check('wizard: Install finished', 'install' in w.ok, w.note_lbl.cget('text') if w.note_lbl else '')
N.FROZEN = install.N.FROZEN = False   # from here the watcher and login items start this source copy
guarded('wizard: Finish', w.finish, True, True, True, True); pump(w, 2)
check('setup done', N.is_setup() and N.startup_on() and N.boot_task_exists())
w.lock.close(); w.destroy()   # the window's "open" lock, as when the wizard exits

# the main window
app = guarded('main window opens', R.App)
if app:
    pump(app, 5); pump(app, 90, lambda: not app.busy); shot('main')
    check('main window: first check ran', not app.busy)
    s = guarded('Settings opens', R.SettingsDialog, app); pump(app, 2); shot('settings')
    if s:
        N.FROZEN = install.N.FROZEN = True
        guarded('Settings: check for updates (frozen path)', install.check_update)
        N.FROZEN = install.N.FROZEN = False
        guarded('Settings: save', s.save); pump(app, 2)
    guarded('Stop', app.do_power, True); pump(app, 3); shot('stopped')
    check('Stop: tunnel and login start off', not N.core_running() and N.task_state() == 'Disabled')
    guarded('Start', app.do_power, False); pump(app, 5)
    check('Start: tunnel and login start on', N.core_running() and N.task_state() == 'Ready')
    guarded('Report a problem', R.ReportDialog, app); pump(app, 3); shot('report')
    check('report written', bool(os.listdir(N.REPORTS)) if os.path.isdir(N.REPORTS) else False)
    s = R.SettingsDialog(app); pump(app, 1)
    answers[:] = [True, False]   # remove: yes; keep settings: no
    real_exit = os._exit
    os._exit = lambda code=0: (_ for _ in ()).throw(SystemExit(code))
    guarded('Uninstall from Settings', s.uninstall)
    os._exit = real_exit
    check('Uninstall: login items gone', not N.startup_on() and not N.boot_task_exists())

logs = ''
for f in ('window.log', 'watch.log'):
    try:
        logs += open(os.path.join(N.LOGS, f), encoding='utf-8').read()
    except OSError:
        pass
bad = [l for l in logs.splitlines() if 'error' in l.lower() and WINDOWS_ONLY.search(l)]
check('no Windows-only errors in the logs', not bad, ' | '.join(bad[:5]))
print('errors in the logs (router ones are expected here):', *[l for l in logs.splitlines() if 'error' in l.lower()][:15], sep='\n  ')
print(f'{len(fails)} failed' if fails else 'all passed', flush=True)
os._exit(1 if fails else 0)
