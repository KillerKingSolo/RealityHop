#!/usr/bin/env python3
"""realityhop.pyw  -  the Reality Hop window (RealityHop.exe). Double-click to run; the first run is the setup wizard.

The PC tunnel is rh-core.exe (official sing-box) running pc-config.json; the VPN app connects through it on
127.0.0.1:2080. The optional router tunnel runs on a GL.iNet router from router-config.json. All node testing runs
on this PC. Everything lives in the install folder (C:\\Program Files\\RealityHop unless the setup picked another;
%LOCALAPPDATA%\\RealityHop when run from source).

  Overview               tiles, node tables (double-click a node for details and its ping graph), last-24-hours strip
  Check now              tests the nodes from this PC and through the PC tunnel
  Restart tunnel / Stop  restart rh-core (about 2 s); Stop turns everything off until Start (for when you are away)
  Nodes                  Show candidates, Fix (online / worked before / quick swap), Replace all with best 6, Rescue
  PC tunnel              Restart, Run setup again
  Router                 Test on router, Push to router (only with router features on)
  Help / Files           Report a problem (anonymous logs); with "Show backend files" on: the PC, Router and logs folders
  Automation             hourly auto-check + auto-repair, background watcher at logon (tray icon), start tunnel at
                         boot, Settings (intervals, router features, sing-box channel, updates, uninstall)
  Cancel                 stops the running action; any action is cut off after 6 minutes anyway
  Logs                   logs/window.log, logs/watch.log (1 MB + one older copy each), logs/core.log (sing-box warnings);
                         Report a problem writes an anonymous copy to anonymous-logs/

  RealityHop.exe --watch       the background watcher (started at logon)
  RealityHop.exe --setup       the setup wizard again
  RealityHop.exe --uninstall   remove Reality Hop (Apps & features uses this)
  python realityhop.pyw --once prints one local check and exits
"""
from concurrent.futures import ThreadPoolExecutor
import hashlib, json, os, queue, re, socket, subprocess, sys, threading, time, tkinter as tk
from tkinter import ttk, messagebox, filedialog
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); sys.dont_write_bytecode = True   # no __pycache__ clutter
import engine as N
import install, report

BG, SIDE, CARD, ROW2, LINE, BTN, FG, MUTED, DIM = '#0c0c0e', '#111114', '#16161a', '#1a1a1f', '#26262d', '#202026', '#ececf0', '#8d8d97', '#5c5c66'
ACC, ACC_HI, ACC_LO, SEL = '#e5383b', '#f25457', '#b3262a', '#3b1416'
STATE = {'ok': '#5fd38d', 'bad': '#ff4d4f', 'warn': '#f5a524', 'unk': MUTED}
ROWFG = {'ok': '#5fd38d', 'bad': '#ff6b6d', 'warn': '#f5b84a', 'unk': MUTED, 'broken': '#ffd0d0'}
HOVER = '#1d1d23'
PILL = {'ok': '#123222', 'bad': '#3a1214', 'warn': '#3a2a0c', 'unk': '#1f1f25'}
COLS = (('slot', 70), ('server', 135), ('where', 190), ('tcp', 90), ('tunnel', 95), ('verdict', 420))
HEAD = {'slot': 'SLOT', 'server': 'SERVER', 'front': 'FRONT', 'where': 'LOCATION', 'tcp': '443 PING', 'tunnel': 'TUNNEL',
        'verdict': 'STATUS', 'num': '#', 'result': 'RESULT'}
EVENTS = {'drop': ('#ff4d4f', 'PIA Dropped'), 'back': ('#5fd38d', 'PIA Back'), 'replace': ('#f5a524', 'Node Replaced'),
          'rescue': ('#b57cff', 'Rescue'), 'restart': ('#5b9cff', 'Tunnel Restart'), 'heal': ('#f5a524', 'Router Repaired'),
          'error': ('#ff4d4f', 'error')}
ROUTER = not N.pc_only()   # router features on: router section, router node table, router tile
RONLY = N.router_only()    # router-only mode: no PC tunnel, no PC section
LISTS = ((('pc', 'PC tunnel', 'pc-config.json', N.PC_CFG),) if not RONLY else ()) + \
    ((('phone', 'Router devices', 'router-config.json', N.ROUTER_CFG),) if ROUTER else ())
MAIN = 'phone' if ROUTER else 'pc'   # the list the slot numbers are taken from
BACKEND = bool(N.settings_load()['backend_files'])   # sidebar buttons for the PC, Router and logs folders
PUSH_HINT = ('Use Push to router when you are ready.' if RONLY else 'Use Push to router and Restart tunnel when you are ready.'
             if ROUTER else 'Press Restart tunnel when you are ready.')
UI = 'Segoe UI Variable Text' if sys.platform == 'win32' else 'Segoe UI'
UIB = 'Segoe UI Variable Display Semib' if sys.platform == 'win32' else 'Segoe UI Semibold'
ICONS = None   # icon font of the sidebar groups (theme picks it)


def theme(root):
    import tkinter.font as tkf
    global UI, UIB, ICONS
    fams = set(tkf.families(root))
    UI = UI if UI in fams else 'Segoe UI'; UIB = UIB if UIB in fams else 'Segoe UI Semibold'
    ICONS = next((f for f in ('Segoe Fluent Icons', 'Segoe MDL2 Assets') if f in fams), None)   # Windows 11, Windows 10
    root.bind_class('TButton', '<Enter>', lambda e: Tip.enter(e.widget, str(e.widget.cget('text'))), add='+')
    for ev in ('<Leave>', '<ButtonPress>'):
        root.bind_class('TButton', ev, Tip.leave, add='+')
    s = ttk.Style(root); s.theme_use('clam')
    s.configure('.', background=BG, foreground=FG, font=(UI, 10), borderwidth=0, focuscolor=BG, lightcolor=BG, darkcolor=BG)
    for name, bg in (('TFrame', BG), ('Card.TFrame', CARD), ('Side.TFrame', SIDE)):
        s.configure(name, background=bg)
    s.configure('TLabel', background=BG); s.configure('Card.TLabel', background=CARD)
    s.configure('Title.TLabel', font=(UIB, 20), foreground=FG)
    s.configure('H.TLabel', background=CARD, font=(UIB, 13))
    s.configure('Muted.TLabel', foreground=MUTED); s.configure('CardMuted.TLabel', background=CARD, foreground=MUTED)
    s.configure('Side.TLabel', background=SIDE, foreground=DIM, font=(UIB, 8))
    s.configure('Brand.TLabel', background=SIDE, foreground=FG, font=(UIB, 15))
    s.configure('SideMuted.TLabel', background=SIDE, foreground=MUTED, font=(UI, 9))
    s.configure('TButton', background=BTN, foreground=FG, padding=(14, 7), relief='flat', bordercolor=LINE, lightcolor=BTN, darkcolor=BTN)
    s.map('TButton', background=[('disabled', ROW2), ('pressed', '#33333b'), ('active', '#2a2a31')], foreground=[('disabled', DIM)])
    s.configure('Accent.TButton', background=ACC, foreground='#ffffff', bordercolor=ACC, lightcolor=ACC, darkcolor=ACC, font=(UIB, 10))
    s.map('Accent.TButton', background=[('disabled', '#3d1c1e'), ('pressed', ACC_LO), ('active', ACC_HI)], foreground=[('disabled', DIM)])
    s.configure('Side.TButton', background=SIDE, foreground=FG, anchor='w', padding=(16, 8), bordercolor=SIDE, lightcolor=SIDE, darkcolor=SIDE)
    s.map('Side.TButton', background=[('disabled', SIDE), ('pressed', '#26262d'), ('active', '#1d1d22')], foreground=[('disabled', DIM)])
    s.configure('SideAccent.TButton', background=SIDE, foreground=ACC_HI, anchor='w', padding=(16, 8), bordercolor=SIDE,
                lightcolor=SIDE, darkcolor=SIDE, font=(UIB, 10))
    s.map('SideAccent.TButton', background=[('disabled', SIDE), ('pressed', '#2a1517'), ('active', '#22141a')], foreground=[('disabled', DIM)])
    s.configure('Ghost.TButton', background=CARD, foreground=FG, padding=(12, 6), bordercolor=LINE, lightcolor=CARD, darkcolor=CARD)
    s.map('Ghost.TButton', background=[('disabled', CARD), ('active', '#202026')], foreground=[('disabled', DIM)])
    s.configure('Treeview', background=CARD, fieldbackground=CARD, foreground=FG, rowheight=30, relief='flat', font=(UI, 10))
    s.configure('Treeview.Heading', background=CARD, foreground=DIM, relief='flat', font=(UIB, 8), padding=(8, 8))
    s.map('Treeview.Heading', background=[('active', CARD)])
    s.map('Treeview', background=[('selected', SEL)], foreground=[('selected', FG)])
    s.layout('Treeview', [('Treeview.treearea', {'sticky': 'nswe'})])
    for cb in ('TCheckbutton', 'Side.TCheckbutton'):
        bg = SIDE if cb.startswith('Side') else BG
        s.configure(cb, background=bg, foreground=FG, indicatorbackground=BTN, indicatorforeground=FG, font=(UI, 9))
        s.map(cb, background=[('active', bg)], indicatorbackground=[('selected', ACC), ('active', '#2a2a31')],
              indicatorforeground=[('selected', ACC)])
    s.configure('TSpinbox', fieldbackground=BTN, background=BTN, foreground=FG, arrowcolor=FG, bordercolor=LINE, lightcolor=BTN,
                darkcolor=BTN, insertcolor=FG, padding=(6, 2))
    s.configure('Vertical.TScrollbar', background='#24242a', troughcolor=CARD, arrowcolor=MUTED, bordercolor=CARD, lightcolor='#24242a',
                darkcolor='#24242a', arrowsize=12, gripsize=0)   # no grip lines on the thumb
    s.map('Vertical.TScrollbar', background=[('pressed', ACC_LO), ('active', ACC)], lightcolor=[('pressed', ACC_LO), ('active', ACC)],
          darkcolor=[('pressed', ACC_LO), ('active', ACC)], arrowcolor=[('active', '#ffffff')])   # all red under the pointer
    s.configure('Busy.Horizontal.TProgressbar', background=ACC, troughcolor=BG, bordercolor=BG, lightcolor=ACC, darkcolor=ACC, thickness=3)


def dark_titlebar(win):
    try:
        import ctypes; win.update()
        hwnd = ctypes.windll.user32.GetParent(win.winfo_id())
        ctypes.windll.dwmapi.DwmSetWindowAttribute(hwnd, 20, ctypes.byref(ctypes.c_int(1)), 4)
        ctypes.windll.dwmapi.DwmSetWindowAttribute(hwnd, 35, ctypes.byref(ctypes.c_int(0x000e0c0c)), 4)   # caption color (Win 11)
        win.withdraw(); win.deiconify()
    except Exception:
        pass


def ms(v, none='-'):
    return f'{v} ms' if v is not None else none


def table(parent, cols, height=None, stretch=None, mode='browse'):
    t = ttk.Treeview(parent, columns=[k for k, _ in cols], show='headings', selectmode=mode, **({'height': height} if height else {}))
    for k, w in cols:
        t.heading(k, text=HEAD.get(k, k.upper()), anchor='w'); t.column(k, width=w, anchor='w', stretch=(k == stretch))
    for state, color in ROWFG.items():
        t.tag_configure(state, foreground=color)
    t.tag_configure('odd', background=ROW2)
    t.tag_configure('broken', background='#4a1518', foreground=ROWFG['broken'])   # created last, so it wins over odd/even
    return t


TIPS = {
    # sidebar
    'Show candidates': 'Opens the nodes from the public node list that could replace yours. Ping and test them, then put them in slots.',
    'Fix: test online nodes': 'Replaces the selected node with the best node from the online list that passes a test.',
    'Fix: nodes that worked before': 'Replaces the selected node with the best node from the vault of nodes that worked before.',
    'Fix: quick swap, untested': 'Replaces the selected node at once with the next fastest node, without a test.',
    'Replace all with best 6': 'Tests the fastest nodes from the online list and your current ones, then puts the best 6 in. About a minute.',
    'Rescue from verified vault': 'For when every node is dead. Tests the vault of nodes that worked before and puts the best 6 in.',
    'Restart PC tunnel': 'Restarts the PC tunnel on the nodes in your slots. About 2 seconds.',
    'Run setup again': 'Opens the setup wizard again, to add a router or redo the VPN step or the background start. Your nodes are kept.',
    'Test on router': "The router's tunnel tests every node. If another PC changed the router, its nodes are taken first.",
    'Devices': 'Choose which devices on the router use the tunnel.',
    'Push to router': 'Sends the router config and the device list to the router and restarts its tunnel (a few seconds).',
    'Report a problem': 'Makes a redacted report of the logs to attach to a GitHub issue. Nothing is sent.',
    'Open backend folder': 'Opens the install folder with the PC, Router and logs folders.',
    'Open logs folder': 'Opens the folder with window.log, watch.log and core.log.',
    'Settings': 'Check interval, thresholds, router features, updates, the sing-box channel, uninstall.',
    # overview
    'Check now': 'Tests every node through the running PC tunnel. If another PC changed the router, its nodes are taken first.',
    'Restart tunnel': 'Restarts the PC tunnel on the nodes in your slots. About 2 seconds.',
    'Cancel': 'Stops the action that is running.',
    'Stop': "Turns everything off, including the start at boot, until you press Start. Turn the VPN app's proxy off while stopped.",
    'Start': 'Turns the PC tunnel and the background watcher on again, now and at every boot.',
    'Update': 'Installs the new Reality Hop release. The tunnel keeps running.',
    'Install update': 'Installs the new release now. The window restarts; the tunnel keeps running.',
    'Later': 'Closes this window. The Update button at the top stays until you update.',
    'Hide': 'Hides the activity log.',
    'Show': 'Shows the activity log.',
    # candidates
    'Ping again from this PC': 'Downloads the node list again and pings port 443 of each candidate from this PC.',
    'Test fastest': 'Tests the fastest candidates with a separate copy of sing-box: a real handshake and download through each. '
                    'Shows which ones work before you put them in.',
    'Replace all 6 with the best 6': 'Writes the best 6 candidates into your slots, tested working ones first. Nothing restarts yet.',
    'Put picked in slots': 'Writes the candidates you picked into the slots, in the order you clicked them. Nothing restarts yet.',
    'Test PC tunnel': 'Tests the nodes in your slots through the running PC tunnel, as Check now does. Restart first after a change.',
    # settings
    'Check for updates': 'Asks GitHub for a newer Reality Hop release now.',
    'Uninstall': 'Removes Reality Hop from this PC. You can also switch the router tunnel off.',
    'Change router': 'For a new or reset router: forgets the old one and opens the router steps of the setup.',
    'Save': 'Saves the settings.',
    'Defaults': 'Puts the default values back in the boxes. Press Save to keep them.',
    # devices
    'Add': 'Adds the address you typed to the list.',
    'Refresh list': "Reads the router's list of devices again.",
    'Save and push': 'Saves the device list and sends it to the router.',
    # report
    'Open GitHub issue': 'Opens a new GitHub issue with the summary filled in. Drag the report file into it.',
    'Open folder': 'Opens the folder with the reports.',
    'Copy': 'Copies the report to the clipboard.',
    'Close': 'Closes this window.',
    # setup
    'Next': 'Goes to the next step.',
    'Finish': 'Saves the choices on this page and closes the setup.',
    'Back': 'Goes back one step.',
    'Change...': 'Picks another install folder.',
    'Install': 'Installs Reality Hop, downloads sing-box, finds the first nodes and starts the tunnel. One UAC prompt, about a minute.',
    'Install again (repair)': 'Installs everything again over the current install, to repair it.',
    'Yes, this PC and my router': 'Sets up the tunnel on this PC and on your GL.iNet router.',
    'Router only': 'Only the router tunnels. Nothing runs on this PC\'s connection.',
    'No, only this PC': 'Sets up the tunnel on this PC only.',
    'Add key to router': "Makes an SSH key and adds it to the router. Windows' ssh asks for the router password; Reality Hop never sees it.",
    'Test connection': 'Connects to the router with the key and shows its model, firmware and sing-box version.',
    'Use my own key file...': 'Uses an SSH key file the router already accepts.',
    'Check again': 'Checks again whether sing-box is installed on the router.',
    'Install sing-box on the router': 'Installs sing-box, jq and curl on the router over SSH.',
    'Check PIA': 'Checks that PIA uses OpenVPN over TCP 443 through the SOCKS5 proxy 127.0.0.1:2080.',
}


def tip_text(text):
    m = re.fullmatch(r'vless-(\d)', text)
    if m:
        return f'Puts the selected candidate into slot vless-{m.group(1)}. Nothing restarts yet.'
    return TIPS.get('Test fastest' if re.fullmatch(r'Test \d+ fastest', text) else text)


class Tip:
    """Hover help for every button: shows after half a second, goes on leave or click."""
    win = job = owner = None

    @classmethod
    def enter(cls, w, text):
        if cls.owner is w:
            return
        cls.leave(); t = tip_text(text)
        if t:
            cls.owner, cls.job = w, w.after(500, cls.show, w, t)

    @classmethod
    def show(cls, w, t):
        cls.job = None
        if not w.winfo_exists():
            return
        cls.win = tk.Toplevel(w); cls.win.overrideredirect(True); cls.win.attributes('-topmost', True)
        tk.Label(cls.win, text=t, bg=CARD, fg=FG, font=(UI, 9), justify='left', wraplength=340, padx=10, pady=6,
                 highlightthickness=1, highlightbackground=LINE).pack()
        cls.win.update_idletasks()
        x = min(w.winfo_rootx(), w.winfo_screenwidth() - cls.win.winfo_width() - 8)
        y = w.winfo_rooty() + w.winfo_height() + 4
        if y + cls.win.winfo_height() > w.winfo_screenheight() - 48:
            y = w.winfo_rooty() - cls.win.winfo_height() - 4
        cls.win.geometry(f'+{x}+{y}')

    @classmethod
    def leave(cls, e=None):
        if e is not None and e.type == tk.EventType.Leave and cls.owner is not None:
            under = e.widget.winfo_containing(e.x_root, e.y_root)
            if under is not None and str(under).startswith(str(cls.owner)):
                return   # moved onto a part of the same button
        if cls.job and cls.owner is not None:
            try:
                cls.owner.after_cancel(cls.job)
            except tk.TclError:
                pass
        if cls.win is not None:
            try:
                cls.win.destroy()
            except tk.TclError:
                pass
        cls.win = cls.job = cls.owner = None


class NavButton(tk.Frame):
    """Sidebar entry: hover shows a red edge, works with .state([...]) and ['text'] like a ttk button."""
    def __init__(self, parent, text, command, icon='›'):
        super().__init__(parent, bg=SIDE, cursor='hand2')
        self.text, self.command, self.disabled = text, command, False
        self.bar = tk.Frame(self, bg=SIDE, width=3); self.bar.pack(side='left', fill='y')
        self.ico = tk.Label(self, text=icon, bg=SIDE, fg=DIM, font=(UIB, 11), padx=10); self.ico.pack(side='left')
        self.lab = tk.Label(self, text=text, bg=SIDE, fg=FG, font=(UI, 10), anchor='w', pady=7); self.lab.pack(side='left', fill='x', expand=True)
        for w in (self, self.ico, self.lab):
            w.bind('<Enter>', lambda e: (self.hover(True), Tip.enter(self, self.text)))
            w.bind('<Leave>', lambda e: (self.hover(False), Tip.leave(e)))
            w.bind('<Button-1>', lambda e: (Tip.leave(), None if self.disabled else self.command()))

    def hover(self, on):
        bg = HOVER if on and not self.disabled else SIDE
        for w in (self, self.ico, self.lab):
            w.configure(bg=bg)
        self.bar.configure(bg=ACC if on and not self.disabled else SIDE)
        self.ico.configure(fg=ACC if on and not self.disabled else DIM)

    def state(self, spec):
        self.disabled = 'disabled' in spec
        self.lab.configure(fg=DIM if self.disabled else FG); self.configure(cursor='arrow' if self.disabled else 'hand2')
        if self.disabled:
            self.hover(False)

    def __getitem__(self, key):
        return self.text if key == 'text' else super().__getitem__(key)


class Toggle(tk.Frame):
    """An on/off switch bound to a BooleanVar."""
    def __init__(self, parent, text, var, command=None):
        super().__init__(parent, bg=SIDE)
        self.var, self.command = var, command
        self.c = tk.Canvas(self, width=36, height=20, bg=SIDE, highlightthickness=0, cursor='hand2'); self.c.pack(side='left')
        self.lab = tk.Label(self, text=text, bg=SIDE, fg=FG, font=(UI, 9), cursor='hand2'); self.lab.pack(side='left', padx=(9, 0))
        for w in (self.c, self.lab):
            w.bind('<Button-1>', self.click)
        var.trace_add('write', lambda *a: self.draw()); self.draw()

    def draw(self):
        c, on = self.c, bool(self.var.get()); c.delete('all'); col = ACC if on else '#3a3a43'
        c.create_oval(1, 1, 19, 19, fill=col, outline=col); c.create_oval(17, 1, 35, 19, fill=col, outline=col)
        c.create_rectangle(10, 1, 26, 19, fill=col, outline=col)
        x = 26 if on else 10; c.create_oval(x - 7, 3, x + 7, 17, fill='#ffffff', outline='')

    def click(self, e=None):
        self.var.set(not self.var.get())
        if self.command:
            self.command()


def card(parent, **pack):
    """A panel with a 1 px border."""
    outer = tk.Frame(parent, bg=LINE); outer.pack(**pack)
    inner = tk.Frame(outer, bg=CARD); inner.pack(fill='both', expand=True, padx=1, pady=1)
    return inner


def pill(parent, text='', state='unk', bg=CARD):
    return tk.Label(parent, text=text, bg=PILL[state], fg=STATE[state], font=(UIB, 9), padx=10, pady=3)


def set_pill(p, text, state):
    p.configure(text=text, bg=PILL[state], fg=STATE[state])


def strip(parent, text, pad):
    f = card(parent, fill='x', pady=(12, 0), **pad)
    bar = tk.Frame(f, bg=ACC, width=4); bar.pack(side='left', fill='y')
    lab = tk.Label(f, text=text, bg=CARD, fg=FG, font=(UI, 10), anchor='w', justify='left', padx=14, pady=11, wraplength=1000)
    lab.pack(side='left', fill='x', expand=True); lab.bar = bar
    return lab


def link(parent, text, url, bg=CARD):
    """A clickable web link."""
    import webbrowser
    lab = tk.Label(parent, text=text, bg=bg, fg=ACC_HI, font=(UI, 9, 'underline'), cursor='hand2')
    lab.bind('<Button-1>', lambda e: webbrowser.open(url))
    return lab


def menu(parent):
    return tk.Menu(parent, tearoff=0, bg=CARD, fg=FG, activebackground=SEL, activeforeground=FG, relief='flat', bd=0, font=(UI, 10))


def md_blocks(text):
    """Release notes (Markdown) as [('p' or 'li', text)]: continuation lines joined, the checksum footer left out."""
    blocks = []
    for line in text.replace('\r', '').split('\n'):
        s = line.strip()
        if not s or s == '---':
            blocks.append(None); continue
        if s.startswith(('- ', '* ')):
            blocks.append(['li', s[2:]])
        elif blocks and blocks[-1] is not None:
            blocks[-1][1] += ' ' + s
        else:
            blocks.append(['p', s.lstrip('#').strip()])
    return [tuple(b) for b in blocks if b]


def md_insert(t, text, tags):
    """One line of Markdown into a Text widget: **bold**, `code`, [text](link) shown as its text."""
    text = re.sub(r'\[([^\]]+)\]\([^)]+\)', r'\1', text)
    for part in re.split(r'(\*\*[^*]+\*\*|`[^`]+`)', text):
        if part.startswith('**') and part.endswith('**') and len(part) > 4:
            t.insert('end', part[2:-2], tags + ('b',))
        elif part.startswith('`') and part.endswith('`') and len(part) > 2:
            t.insert('end', part[1:-1], tags + ('code',))
        elif part:
            t.insert('end', part, tags)


class App(tk.Tk):
    def __init__(self):
        super().__init__()
        self.title('Reality Hop'); self.geometry('1440x1040'); self.minsize(1180, 800); self.configure(bg=BG)
        self.q, self.busy, self.streak, self.sel, self.cand = queue.Queue(), False, {}, None, None
        self.lists, self.pcs, self.router = {'pc': [], 'phone': []}, None, None
        self.pc_pending, self.want_restart, self.pending, self.router_stale = False, False, None, False
        self.status_text, self.status_state = 'Reading the configs and testing from this PC...', 'unk'
        self.update_rel = self.upd_win = None
        N.say = self.say; self.flog = N.file_log('window'); self.wstate, self.started = {}, 0
        self.lock = socket.socket()
        try:
            self.lock.bind(('127.0.0.1', N.GUI_PORT))   # tells the background watcher to pause
        except OSError:
            pass
        try:
            last = json.load(open(N.LAST))
            self.lists, self.pcs = last['lists'], last['pcs']
            self.router = tuple(last['router']) if last.get('router') else None
            self.lists = {k: self.lists.get(k, []) for k in ('pc', 'phone')}
            if RONLY:   # saved before a switch to router only: no PC tunnel state any more
                self.pcs, self.lists['pc'] = None, []
            self.judge()   # the last known state at once, so the icon is not grey while the first check runs
            self.status_text = f"Last check {last['at']}: {self.status_text} Checking again now..."
        except Exception:
            pass
        try:
            import ctypes; ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID('RealityHop.App')   # own taskbar icon
        except Exception:
            pass
        self.icon_key = None
        theme(self); self.build(); self.fit(); dark_titlebar(self); self.render()
        self.after(150, self.drain); self.after(400, lambda: self.work(self.first_check)); self.after(1500, self.auto_tick); self.after(5000, self.watchdog)
        threading.Thread(target=self.find_update, daemon=True).start(); self.after(30000, self.maybe_core_update)

    def maybe_core_update(self):
        """sing-box keeps itself current on its own channel, also when Reality Hop has no new release: daily (the watcher
        does it when it runs) and at once after a channel change."""
        changed = N.this_pc().get('core_channel') != N.settings_load()['singbox']
        if N.core_update_due() and (changed or not N.port_taken(N.WATCH_PORT)):
            if self.busy:
                self.after(60000, self.maybe_core_update); return
            self.work(self.core_update)

    def core_update(self):
        N.set_this_pc(core_checked=time.time())
        try:
            ver = N.core_update()
        except Exception as e:   # offline or GitHub down: tomorrow again; the tunnel keeps running on the current build
            self.say(f'sing-box update check failed: {e}'); return
        if ver:
            self.show_core(); self.check_local('pc'); self.status_text, self.status_state = f'sing-box {ver} installed; the tunnel is running on it.', 'ok'

    def find_update(self):
        try:
            rel = install.check_update()
        except Exception:
            return   # no internet or GitHub down: the watcher tries again tomorrow
        if rel:
            self.update_rel = rel
            self.after(0, lambda: (self.update_btn.configure(text=f"Update to {rel['ver']}"), self.update_btn.pack(side='right', padx=(0, 8))))
            self.after(1500, self.do_update)   # tell at launch, not only with the button

    def do_update(self):
        if self.update_rel and not (self.upd_win and self.upd_win.winfo_exists()):
            self.upd_win = UpdateDialog(self, self.update_rel)

    def apply_update(self, rel):
        install.apply_update(rel); N.watcher_kill()
        if os.path.exists(N.STARTUP_LNK):
            install.start_watcher()
        self.lock.close(); restart_window()

    def build(self):
        self.buttons = []
        self.auto_on, self.auto_min, self.auto_fix = tk.BooleanVar(value=True), tk.IntVar(value=60), tk.BooleanVar(value=True)
        self.bg_on = tk.BooleanVar(value=os.path.exists(N.STARTUP_LNK))

        # sidebar: every action, grouped
        side = self.side = ttk.Frame(self, style='Side.TFrame', width=262); side.pack(side='left', fill='y'); side.pack_propagate(False)
        tk.Frame(self, bg=LINE, width=1).pack(side='left', fill='y')
        brand = ttk.Frame(side, style='Side.TFrame'); brand.pack(fill='x', padx=18, pady=(20, 4))
        tk.Label(brand, text='●', bg=SIDE, fg=ACC, font=(UIB, 14)).pack(side='left')
        ttk.Label(brand, text=' Reality Hop', style='Brand.TLabel').pack(side='left')
        ttk.Label(side, text=('Router only  ·  ' if RONLY else 'PC + router  ·  ' if ROUTER else 'PC only  ·  ')
                  + (f'v{N.VERSION}' if N.core_mode() or RONLY else 'rh-core MISSING'), style='SideMuted.TLabel').pack(anchor='w', padx=20)
        self.core_lbl = ttk.Label(side, text='sing-box ...', style='SideMuted.TLabel'); self.core_lbl.pack(anchor='w', padx=20, pady=(0, 14))
        threading.Thread(target=self.show_core, daemon=True).start()

        def group(title, glyph, shape):
            f = tk.Frame(side, bg='#17171c'); f.pack(fill='x', padx=10, pady=(16, 4))
            tk.Frame(f, bg=ACC, width=4).pack(side='left', fill='y')
            tk.Label(f, text=glyph if ICONS else shape, bg='#17171c', fg=ACC, font=(ICONS or UIB, 12), width=2, padx=6,
                     pady=7).pack(side='left')
            tk.Label(f, text=title, bg='#17171c', fg=FG, font=(UIB, 12)).pack(side='left')

        def sbtn(text, cmd, accent=False):
            w = NavButton(side, text, cmd); w.pack(fill='x', padx=(18, 10)); self.buttons.append(w); return w
        group('Nodes', '\ue774', '◆')   # globe
        sbtn('Show candidates', self.show_candidates)
        sbtn('Fix: test online nodes', lambda: self.replace_selected('online'))
        sbtn('Fix: nodes that worked before', lambda: self.replace_selected('vault'))
        sbtn('Fix: quick swap, untested', lambda: self.replace_selected('quick'))
        sbtn('Replace all with best 6', self.replace_all)
        sbtn('Rescue from verified vault', self.rescue)
        if not RONLY:
            group('PC tunnel', '\ue977', '■')   # computer
            sbtn('Restart PC tunnel', lambda: self.work(self.pc_restart))
            sbtn('Run setup again', self.rerun_setup)
        if ROUTER:
            group('Router', '\ueb77', '▲')   # router
            sbtn('Test on router', lambda: self.work(self.check_router))
            sbtn('Devices', lambda: DevicesDialog(self))
            sbtn('Push to router', self.push_phone)
            if RONLY:
                sbtn('Run setup again', self.rerun_setup)
        group('Files & help' if BACKEND else 'Help', '\ue897', '●')   # question mark
        sbtn('Report a problem', lambda: ReportDialog(self))
        if BACKEND:
            sbtn('Open backend folder', lambda: os.startfile(N.DATA))
            sbtn('Open logs folder', lambda: os.startfile(N.LOGS))

        foot = ttk.Frame(side, style='Side.TFrame'); foot.pack(side='bottom', fill='x', padx=18, pady=16)
        tk.Frame(foot, bg=LINE, height=1).pack(fill='x', pady=(0, 12))
        tk.Label(foot, text='Automation', bg=SIDE, fg=FG, font=(UIB, 11)).pack(anchor='w', pady=(0, 10))
        row = tk.Frame(foot, bg=SIDE); row.pack(fill='x')
        Toggle(row, 'Check every', self.auto_on).pack(side='left')
        ttk.Spinbox(row, from_=15, to=1440, increment=15, width=5, textvariable=self.auto_min).pack(side='left', padx=6)
        ttk.Label(row, text='min', style='SideMuted.TLabel').pack(side='left')
        Toggle(foot, 'Auto-repair dead nodes', self.auto_fix).pack(anchor='w', pady=(10, 0))
        Toggle(foot, 'Run in background at logon', self.bg_on, lambda: self.work(N.startup, self.bg_on.get())).pack(anchor='w', pady=(10, 0))
        self.boot_on = tk.BooleanVar(value=False)
        if not RONLY:
            Toggle(foot, 'Start tunnel at boot', self.boot_on, lambda: self.work(self.set_boot, self.boot_on.get())).pack(anchor='w', pady=(10, 0))
        threading.Thread(target=lambda: self.after(0, self.boot_on.set, N.boot_task_exists()), daemon=True).start()
        self.next_lbl = ttk.Label(foot, text='', style='SideMuted.TLabel'); self.next_lbl.pack(anchor='w', pady=(10, 0))
        ttk.Button(foot, text='Settings', style='Ghost.TButton', command=self.settings).pack(anchor='w', pady=(12, 0))
        self.auto_min.set(N.settings_load()['check_min'])

        # main area
        main = ttk.Frame(self); main.pack(side='left', fill='both', expand=True)
        slot = tk.Frame(main, bg=BG, height=3); slot.pack(fill='x'); slot.pack_propagate(False)
        self.prog = ttk.Progressbar(slot, mode='indeterminate', style='Busy.Horizontal.TProgressbar')
        pad = dict(padx=24)
        head = ttk.Frame(main); head.pack(fill='x', pady=(18, 0), **pad)
        ttk.Label(head, text='Overview', style='Title.TLabel').pack(side='left')
        self.head_pill = pill(head, 'checking', 'unk', BG); self.head_pill.pack(side='left', padx=14, pady=(6, 0))
        self.cancel_btn = ttk.Button(head, text='Cancel', command=self.cancel); self.cancel_btn.pack(side='right'); self.cancel_btn.state(['disabled'])
        self.check_btn = ttk.Button(head, text='Check now', style='Accent.TButton', command=lambda: self.work(self.first_check))
        self.check_btn.pack(side='right', padx=(0, 8)); self.buttons.append(self.check_btn)
        self.restart_btn = ttk.Button(head, text='Test on router', command=lambda: self.work(self.check_router)) if RONLY else \
            ttk.Button(head, text='Restart tunnel', command=lambda: self.work(self.pc_restart))
        self.restart_btn.pack(side='right', padx=(0, 8)); self.buttons.append(self.restart_btn)
        self.power_btn = ttk.Button(head, text='Start' if N.stopped() else 'Stop', command=self.power)
        self.power_btn.pack(side='right', padx=(0, 8)); self.buttons.append(self.power_btn)
        self.update_btn = ttk.Button(head, text='Update', style='Accent.TButton', command=self.do_update)   # shown when a release is newer

        tiles = ttk.Frame(main); tiles.pack(fill='x', pady=(16, 0), **pad)
        self.tiles = {}
        kinds = ((('pc', 'PC TUNNEL'),) if not RONLY else ()) + ((('router', 'ROUTER TUNNEL'),) if ROUTER else ()) + \
            (('nodes', 'NODES WORKING'), ('watch', 'BACKGROUND'))
        for i, (k, label) in enumerate(kinds):
            tiles.columnconfigure(i, weight=1, uniform='t')
            outer = tk.Frame(tiles, bg=LINE); outer.grid(row=0, column=i, sticky='nsew', padx=(0 if i == 0 else 6, 0 if i == len(kinds) - 1 else 6))
            t = tk.Frame(outer, bg=CARD); t.pack(fill='both', expand=True, padx=1, pady=1)
            tk.Label(t, text=label, bg=CARD, fg=DIM, font=(UIB, 8)).pack(anchor='w', padx=16, pady=(14, 0))
            v = tk.Label(t, text='-', bg=CARD, fg=FG, font=(UIB, 17)); v.pack(anchor='w', padx=16)
            sub = tk.Label(t, text='', bg=CARD, fg=MUTED, font=(UI, 9), anchor='w', justify='left'); sub.pack(anchor='w', padx=16, pady=(0, 14))
            self.tiles[k] = (v, sub)
            sub.bind('<Button-1>', lambda e, k=k: self.tile_act.get(k) and self.tile_act[k]())
        self.tile_act = {}   # a tile hint that names a button runs that button when clicked

        self.status = strip(main, self.status_text, pad)
        hs = card(main, fill='x', pady=(12, 0), **pad)
        top_ = tk.Frame(hs, bg=CARD); top_.pack(fill='x', padx=16, pady=(10, 0))
        tk.Label(top_, text='LAST 24 HOURS', bg=CARD, fg=DIM, font=(UIB, 8)).pack(side='left')
        for k in ('drop', 'replace', 'restart', 'rescue'):
            col, name = EVENTS[k]
            tk.Label(top_, text='●', bg=CARD, fg=col, font=(UI, 9)).pack(side='left', padx=(14, 2))
            tk.Label(top_, text=name, bg=CARD, fg=MUTED, font=(UI, 8)).pack(side='left')
        self.strip_info = tk.Label(top_, text='', bg=CARD, fg=MUTED, font=(UI, 8)); self.strip_info.pack(side='right')
        self.hist = tk.Canvas(hs, height=40, bg=CARD, highlightthickness=0); self.hist.pack(fill='x', padx=16, pady=(4, 10))
        self.hist.bind('<Configure>', lambda e: self.draw_history())

        self.trees, self.card_status = {}, {}
        for key, label, rel, _ in LISTS:
            c = card(main, fill='x', pady=(14, 0), **pad)
            row = tk.Frame(c, bg=CARD); row.pack(fill='x', padx=16, pady=(14, 0))
            tk.Label(row, text=label, bg=CARD, fg=FG, font=(UIB, 12)).pack(side='left')
            tk.Label(row, text=rel, bg=CARD, fg=DIM, font=(UI, 9)).pack(side='left', padx=(10, 0), pady=(3, 0))
            st = tk.Label(row, text='', bg=CARD, fg=MUTED, font=(UI, 9), anchor='e'); st.pack(side='right')
            tk.Frame(c, bg=LINE, height=1).pack(fill='x', pady=(12, 0))
            t = table(c, COLS, height=6, stretch='verdict'); t.pack(fill='x', padx=8, pady=(2, 8))
            t.bind('<<TreeviewSelect>>', lambda e, k=key: self.select(k)); t.bind('<Button-3>', lambda e, k=key: self.popup(e, k))
            t.bind('<Double-1>', lambda e, k=key: self.details(k, e))
            self.card_status[key], self.trees[key] = st, t
        self.bind('<Configure>', lambda e: self.status.configure(wraplength=max(400, e.width - 360)) if e.widget is self else None)

        lhead = ttk.Frame(main); lhead.pack(fill='x', pady=(14, 4), **pad)
        ttk.Label(lhead, text='ACTIVITY', style='Muted.TLabel', font=(UIB, 8)).pack(side='left')
        self.log_box = card(main, fill='both', expand=True, pady=(0, 18), **pad)
        self.log = tk.Text(self.log_box, height=7, state='disabled', wrap='none', font=('Cascadia Mono', 9) if 'Cascadia Mono' in
                           __import__('tkinter.font').font.families(self) else ('Consolas', 9), bg=CARD, fg=MUTED,
                           relief='flat', highlightthickness=0, padx=14, pady=10, insertbackground=FG, selectbackground=SEL)
        sb = ttk.Scrollbar(self.log_box, command=self.log.yview); self.log.configure(yscrollcommand=sb.set)
        sb.pack(side='right', fill='y'); self.log.pack(fill='both', expand=True)
        self.log.tag_configure('t', foreground=DIM); self.log.tag_configure('err', foreground=STATE['bad'])
        self.log_shown = True
        self.log_btn = ttk.Button(lhead, text='Hide', style='Ghost.TButton', command=self.toggle_log); self.log_btn.pack(side='right')
        self.menu = menu(self)
        for how, label in (('online', 'Fix this node: test online nodes and swap in the best one'),
                           ('vault', 'Fix this node: swap in a node that worked before (tested again)'),
                           ('quick', 'Fix this node: quick swap to the fastest ping (configs only, not tested)')):
            self.menu.add_command(label=label, command=lambda how=how: self.replace_selected(how))

    def fit(self):
        """Tall enough for the whole sidebar (it grows with router features and backend files), never taller than the screen."""
        self.side.pack_propagate(True); self.update_idletasks(); need = self.side.winfo_reqheight() + 24
        self.side.pack_propagate(False); self.side.configure(width=262)
        h = max(1040, min(need, self.winfo_screenheight() - 80))
        self.geometry(f'{min(1440, self.winfo_screenwidth() - 40)}x{h}'); self.minsize(1180, min(need, self.winfo_screenheight() - 80))

    def show_core(self):
        """Under the name: the sing-box build on this PC and whether it is a stable release or a test build."""
        ver = N.core_version(); m = re.search(r'alpha|beta|rc', ver or '')
        text = f"sing-box {ver} · {'test build' if m else 'stable'}" if ver else 'sing-box: not installed'   # fits the sidebar
        self.after(0, lambda: self.core_lbl.configure(text=text))

    def toggle_log(self):
        self.log_shown = not self.log_shown
        if self.log_shown:
            self.log_box.master.pack(fill='both', expand=True, pady=(0, 18), padx=24); self.log_btn.configure(text='Hide')
        else:
            self.log_box.master.pack_forget(); self.log_btn.configure(text='Show')

    # ---- plumbing
    def say(self, *a):
        line = ' '.join(str(x) for x in a); self.q.put(line); self.flog(line)

    def drain(self):
        try:
            while True:
                line = self.q.get_nowait()
                self.log.configure(state='normal'); self.log.insert('end', time.strftime('%H:%M:%S  '), 't')
                self.log.insert('end', line + '\n', 'err' if line.startswith(('error', 'FAIL')) or 'NOT ' in line else ())
                self.log.see('end'); self.log.configure(state='disabled')
        except queue.Empty:
            pass
        self.after(150, self.drain)

    def work(self, fn, *args):
        """One action at a time on a worker thread; buttons are off meanwhile; errors go to the log and the status line."""
        if self.busy:
            self.say('busy, wait for the current action to finish'); return
        self.busy, self.started = True, time.time(); N.CANCEL.clear(); self.cancel_btn.state(['!disabled']); self.prog.pack(fill='x'); self.prog.start(14)
        if self.cand and self.cand.winfo_exists():
            self.cand.prog.pack(fill='x'); self.cand.prog.start(14)
        for b in self.buttons:
            b.state(['disabled'])
        def go():
            try:
                fn(*args)
            except Exception as e:
                self.say(f'error: {e}'); self.status_text, self.status_state = f'Something failed: {e}', 'bad'
            finally:
                self.after(0, self.done)
        threading.Thread(target=go, daemon=True).start()

    def cancel(self):
        if self.busy:
            self.say('cancelling the current action...'); N.cancel()

    def watchdog(self):
        """Nothing may keep the buttons greyed out: after 6 minutes the action is cancelled and the window freed."""
        if self.busy and time.time() - self.started > 360:
            self.say('the action ran over 6 minutes; cancelled'); N.cancel()
            self.status_text, self.status_state = 'The last action took too long and was cancelled.', 'bad'; self.done()
        self.after(5000, self.watchdog)

    def done(self):
        self.busy = False; self.cancel_btn.state(['disabled']); self.prog.stop(); self.prog.pack_forget()
        if self.cand and self.cand.winfo_exists():
            self.cand.prog.stop(); self.cand.prog.pack_forget()
        for b in self.buttons:
            b.state(['!disabled'])
        self.render()
        if self.cand and self.cand.winfo_exists():
            self.cand.render()
        self.want_restart = False
        try:
            N.jsave(N.LAST, {'lists': self.lists, 'pcs': self.pcs, 'router': list(self.router) if self.router else None,
                             'at': time.strftime('%H:%M')})
        except Exception:
            pass

    def select(self, key):
        s = self.trees[key].selection()
        if s:
            self.sel = (key, s[0].split(':', 1)[1])
            for k, t in self.trees.items():
                if k != key and t.selection():
                    t.selection_remove(*t.selection())

    def popup(self, e, key):
        row = self.trees[key].identify_row(e.y)
        if row:
            self.trees[key].selection_set(row); self.sel = (key, row.split(':', 1)[1]); self.menu.tk_popup(e.x_root, e.y_root)

    # ---- checks (worker thread)
    def check_local(self, which):
        """Everything this PC can know on its own: the two config files, 443 reachability, the PC tunnel's own view."""
        where = N.known_where()
        for key, label, rel, cfg in LISTS:
            if which not in (key, 'both'):
                continue
            prev = {n['server']: n for n in self.lists[key]}
            nodes = N.nodes_of(cfg)
            for n in nodes:
                p = prev.get(n['server'], {})
                n['where'] = p.get('where') or where.get(n['server'], '?')
                for k in ('pc_ms', 'pc_why', 'phone_ms', 'phone_why'):
                    n[k] = p.get(k)
            self.say(f'{label}: {len(nodes)} nodes in {rel}; testing 443 from this PC...'); N.tcp_probe(nodes)
            self.lists[key] = nodes
        if which in ('pc', 'both') and not RONLY:
            self.say('PC: asking the PC tunnel...'); self.pcs = N.pc_status()
            if self.pcs['api']:
                r = N.check_pc(self.lists['pc'])
                if r and r['stale']:
                    self.pc_pending = True
                    self.say('PC: the tunnel still runs the previous node set; restart it to test the new one')
                elif r:
                    self.pc_pending = False
                    N.hist_add(pings={n['server']: n['pc_ms'] for n in self.lists['pc']})
            else:
                for n in self.lists['pc']:
                    n['pc_ms'] = n['pc_why'] = None
        self.pending = N.pending_deploy()
        for key in ('pc', 'phone'):
            if which in (key, 'both'):
                self.count(key)
        self.judge()

    def first_check(self):
        """Check now: first the router's nodes when another PC changed them, then everything this PC can see; in router-only
        mode also the router's own test, the only tunnel there is."""
        if ROUTER and N.router_up():
            s = N.router_sync()
            if s:
                self.say(s); N.hist_add(event=('heal', s))
        self.check_local('both')
        if RONLY:
            self.check_router()

    def check_router(self):
        """The router's own tunnel tests every node over SSH; also compares its config with router-config.json.
        First takes the router's nodes when another PC (or the self-repair) changed them."""
        s = N.router_sync()
        if s:
            self.say(s); N.hist_add(event=('heal', s)); self.check_local('both')
        nodes = self.lists['phone'] or N.nodes_of(N.ROUTER_CFG)
        self.say('router: asking the router tunnel to test every node...')
        with ThreadPoolExecutor(2) as ex:
            g = ex.submit(N.geolocate, nodes); r = N.check_phone(nodes); g.result()
        self.router = (r, time.strftime('%H:%M')); self.lists['phone'] = nodes
        self.router_stale = N.router_stale(r.get('cfg'))
        self.count('phone'); self.judge()

    def verdict(self, key, n):
        """(state, sentence) for one row. Router result > PC tunnel result > plain 443 reachability."""
        if key == 'phone' and n.get('phone_why') is not None:
            return (('ok', f"working, {n['phone_ms']} ms through the router tunnel") if n['phone_ms'] is not None
                    else ('bad', f"NOT WORKING on the router: {n['phone_why']}"))
        pc = n if key == 'pc' else next((m for m in self.lists['pc'] if m['server'] == n['server']), None)
        if pc and pc.get('pc_why') is not None:
            return (('ok', f"working, {pc['pc_ms']} ms through the PC tunnel") if pc['pc_ms'] is not None
                    else ('bad', f"NOT WORKING: the PC tunnel could not fetch through it ({pc['pc_why']})"))
        if n.get('tcp_ms') is None:
            return 'bad', 'not answering on 443 from this PC'
        if key == 'pc' and self.pc_pending:
            return 'warn', f"answers on 443 in {n['tcp_ms']} ms; the PC tunnel loads it on the next restart"
        return 'warn', f"answers on 443 in {n['tcp_ms']} ms; not tunnel-tested yet"

    def count(self, key):
        for n in self.lists[key]:
            k = (key, n['server']); self.streak[k] = self.streak.get(k, 0) + 1 if self.verdict(key, n)[0] == 'bad' else 0

    def judge(self):
        """The plain-language status line."""
        msgs, state = [], 'ok'
        for key, label, *_ in LISTS:
            nodes = self.lists[key]
            if not nodes:
                continue
            states = [self.verdict(key, n)[0] for n in nodes]
            bad = [n for n, s in zip(nodes, states) if s == 'bad']
            if bad:
                names = ', '.join(f"{n['tag']} ({n['server']})" for n in bad)
                msgs.append(f"{label}: {names} {'is' if len(bad) == 1 else 'are'} not working."
                            + (' ALL nodes are down: press Rescue.' if len(bad) == len(nodes) else '')); state = 'bad'
            elif all(s == 'ok' for s in states):
                msgs.append(f'{label}: all {len(nodes)} nodes are working.')
            else:
                msgs.append(f'{label}: all {len(nodes)} nodes answer on 443; no tunnel test yet.'); state = 'warn' if state == 'ok' else state
        if ROUTER and self.lists['pc'] and self.lists['phone'] and [n['server'] for n in self.lists['pc']] != [n['server'] for n in self.lists['phone']]:
            msgs.append('The PC and router lists differ; a replace makes them the same.')
        if self.pending and self.router_stale:
            msgs.append('Both configs are newer than what is running. ' + PUSH_HINT)
        elif self.pending:
            msgs.append('The PC tunnel runs an older node set than pc-config.json: press Restart tunnel.')
        elif self.router_stale:
            msgs.append('The router runs an older config than router-config.json: press Push to router.')
        elif self.pc_pending:
            msgs.append('New nodes are waiting for a PC tunnel restart.')
        self.status_text, self.status_state = ' '.join(msgs) or 'Nothing checked yet.', state

    # ---- history strip, node details, settings
    def draw_history(self):
        c = self.hist; c.delete('all'); w = max(c.winfo_width(), 200); h = 40; now = time.time(); span = 24 * 3600
        c.create_line(0, h - 12, w, h - 12, fill=LINE)
        L, R = 24, w - 24
        for hr in range(0, 25, 3):
            x = R - hr / 24 * (R - L)
            c.create_line(x, h - 15, x, h - 9, fill=LINE)
            c.create_text(x, h - 2, text='now' if hr == 0 else f'-{hr}h', fill=DIM, font=(UI, 7), anchor='s')
        ev = [e for e in N.hist_load()['events'] if e[0] > now - span]
        counts = {}
        for t, kind, *_ in ev:
            col = EVENTS.get(kind, (MUTED, ''))[0]; x = R - (now - t) / span * (R - L)
            y = 14 if kind in ('drop', 'back') else 6
            if kind == 'back':
                c.create_line(x, y - 3, x, y + 3, fill=col)
            else:
                c.create_oval(x - 3, y - 3, x + 3, y + 3, fill=col, outline='')
            counts[kind] = counts.get(kind, 0) + 1
        self.strip_info.configure(text=f"PIA Drops {counts.get('drop', 0)}   ·   Replacements {counts.get('replace', 0)}   ·   "
                                       f"Restarts {counts.get('restart', 0)}   ·   Rescues {counts.get('rescue', 0)}")

    def details(self, key, e):
        row = self.trees[key].identify_row(e.y)
        if not row:
            return
        n = next((x for x in self.lists[key] if x['tag'] == row.split(':', 1)[1]), None)
        if n:
            NodeDetails(self, key, n)

    def settings(self):
        SettingsDialog(self)

    # ---- drawing (main thread)
    def short(self, key, n):
        """Short status for the table; the full sentence stays in the status banner and the log."""
        state, text = self.verdict(key, n)
        if state == 'ok':
            label = 'Working'
        elif state == 'bad':
            why = text.split('(')[-1].rstrip(')') if '(' in text else text.split(': ', 1)[1] if ': ' in text else ''
            label = 'Not answering on 443' if 'not answering' in text else 'Dead' + (f'  ·  {why}' if why else '')
        else:
            label = 'Waiting for PC restart' if 'next restart' in text else 'Reachable  ·  not tunnel-tested'
        return state, label

    def render(self):
        for key, label, *_ in LISTS:
            t = self.trees[key]; t.delete(*t.get_children())
            for i, n in enumerate(self.lists[key]):
                state, text = self.short(key, n)
                tun = n.get('phone_ms') if key == 'phone' and n.get('phone_why') is not None else None
                if tun is None:
                    pc = n if key == 'pc' else next((m for m in self.lists['pc'] if m['server'] == n['server']), None)
                    tun = pc.get('pc_ms') if pc and pc.get('pc_why') is not None else None
                active = []
                if self.pcs and self.pcs.get('now') == n['tag']:
                    active.append('PC')
                if self.router and self.router[0].get('now') == n['tag']:
                    active.append('router')
                text = {'ok': '●  ', 'warn': '◐  ', 'bad': '✕  '}.get(state, '○  ') + text + (f"   ·   in use by {' + '.join(active)}" if active else '')
                tag = 'broken' if state == 'bad' and n.get('tcp_ms') is None else state
                t.insert('', 'end', iid=f"{key}:{n['tag']}", tags=('odd' if i % 2 else 'even', tag),
                         values=(n['tag'], n['server'], n.get('where', '?'), ms(n.get('tcp_ms'), 'none'), ms(tun, '-'), text))
            if self.sel and self.sel[0] == key and t.exists(f'{key}:{self.sel[1]}'):
                t.selection_set(f'{key}:{self.sel[1]}')

        v, sub = self.tiles.get('pc', (None, None)); p = self.pcs
        if v is None:
            pass   # router-only mode: no PC tunnel tile
        elif p is None:
            v.configure(text='-', fg=MUTED); sub.configure(text='Not checked yet')
        elif p['api'] or p['listening']:
            old = not p['api'] or self.pc_pending
            v.configure(text='Needs restart' if old else 'Running', fg=STATE['warn' if old else 'ok'])
            sub.configure(text=f"{p['now'] or '?'}  ·  exit {p['exit'] or '?'}")
        else:
            v.configure(text='Down', fg=STATE['bad']); sub.configure(text='Nothing on 127.0.0.1:2080')
        if ROUTER:
            v, sub = self.tiles['router']; r = self.router
            if r is None:
                v.configure(text='-', fg=MUTED); self.tile_link('router', 'Press Test on router', lambda: self.work(self.check_router))
            elif r[0]['service']:
                v.configure(text='Older config' if self.router_stale else 'Running', fg=STATE['warn' if self.router_stale else 'ok'])
                self.tile_link('router', f"{r[0]['now'] or '?'}  ·  checked {r[1]}")
            else:
                v.configure(text='Stopped', fg=STATE['bad'])
                self.tile_link('router', f'Checked {r[1]}; press Push to router', self.push_phone)
        v, sub = self.tiles['nodes']
        key = 'pc' if not RONLY and (any(n.get('pc_why') is not None for n in self.lists['pc']) or not self.router) else 'phone'
        nodes = self.lists[key]
        states = [self.verdict(key, n)[0] for n in nodes]
        good, bad = states.count('ok'), states.count('bad')
        if nodes:
            v.configure(text=f'{good} / {len(nodes)}' if good or bad else f'{len(nodes)} reachable',
                        fg=STATE['bad' if bad == len(nodes) else 'warn' if bad or not good else 'ok'])
            sub.configure(text=f"{'router' if key == 'phone' else 'PC'} tunnel test" if good or bad else '443 ping only')
        v, sub = self.tiles['watch']
        on = N.port_taken(N.WATCH_PORT)
        v.configure(text='On' if on else 'Off', fg=STATE['ok' if on else 'unk'])
        sub.configure(text='Hourly checks wait while this window is open' if on else 'Tick Run in background at logon')

        for key, st in (('pc', self.pending), ('phone', self.router_stale)):
            if key in self.card_status:
                self.card_status[key].configure(text=('config newer than what runs' if st else ''), fg=STATE['warn'])
        self.draw_history()
        set_pill(self.head_pill, {'ok': 'All good', 'warn': 'Attention', 'bad': 'Problem', 'unk': 'Checking'}[self.status_state],
                 self.status_state)
        self.status.configure(text=self.status_text, fg=FG if self.status_state in ('ok', 'unk') else STATE[self.status_state])
        self.status.bar.configure(bg=STATE[self.status_state] if self.status_state != 'unk' else ACC)
        self.set_icon()

    def set_icon(self):
        """Window and taskbar icon: the same ring as the tray, one segment per node (PC tunnel, or the router's in
        router-only mode), center = overall state."""
        segs, key = [], 'phone' if RONLY else 'pc'
        for n in self.lists[key]:
            st = self.verdict(key, n)[0]
            segs.append('warn' if st == 'ok' and (n.get(key + '_ms') or 0) > N.SLOW else st if st in ('ok', 'bad') else 'unk')
        key = (self.status_state, tuple(segs))
        if key != self.icon_key:
            try:
                self.iconbitmap(default=N.tray_icon_file(self.status_state, segs)); self.icon_key = key
            except Exception:
                pass

    # ---- actions
    def show_candidates(self):
        if self.cand and self.cand.winfo_exists():
            self.cand.lift(); self.cand.focus_force()
        else:
            self.cand = Candidates(self)

    def replace_selected(self, how):
        """Fix one node: 'online' = test the online node list and swap in the best, 'vault' = the same with the nodes that
        worked before, 'quick' = the fastest 443 ping into the configs only, untested."""
        if not self.sel or not self.lists[MAIN]:
            messagebox.showinfo('Reality Hop', 'Select a node in one of the tables first.'); return
        tags = [n['tag'] for n in self.lists[MAIN]]
        if self.sel[1] not in tags:
            messagebox.showinfo('Reality Hop', 'Select a node in the node table first.'); return
        tag = self.sel[1]; i = tags.index(tag); cur = self.lists[MAIN][i]['server']
        where = 'the router tunnel is' if RONLY else 'the router and the PC tunnel are' if ROUTER else 'the PC tunnel is'
        text = {'online': f'Test the {N.TOP} fastest nodes from the online node list (under a minute) and swap the best one '
                          f'in for {tag} ({cur})? Then {where} updated.',
                'vault': f'Test the nodes that worked before (the vault) again and swap the best one in for {tag} ({cur})? '
                         f'Then {where} updated. Useful when the online node list cannot be reached.',
                'quick': f'Swap {tag} ({cur}) for the node that answers fastest on port 443 from this PC? Only the configs '
                         'change: the node is not tested and nothing restarts until you say so.'}[how]
        if messagebox.askyesno('Fix node', text):
            self.work(self.do_replace, [i], True, how)

    def replace_all(self):
        if messagebox.askyesno('Replace all', f'Scan the node list, test the {N.TOP} fastest candidates plus the current nodes on this PC, '
                               'put the 6 best into the configs, and update the tunnel(s)?\n\nAbout a minute.'):
            self.work(self.do_replace_all)

    def do_replace(self, slots, ask=True, how='online'):
        if how == 'quick':
            swaps = N.replace_local(slots)
            self.after_files(' '.join(f"{tag} was replaced in the configs: {old} -> {new['server']} (443 ping {new['tcp_ms']} ms, not tunnel-tested)."
                                      for tag, old, new in swaps))
        else:
            swaps = N.replace(slots, vault=how == 'vault')
            self.after_deploy(' '.join(f"{tag} was replaced: {old} -> {new['server']} ({new['ms']} ms, {new.get('where', '?')})." for tag, old, new in swaps), ask)

    def tile_link(self, k, text, act=None):
        """A tile's hint text; with act it looks like a link and clicking it does the same as the button it names."""
        sub = self.tiles[k][1]; self.tile_act[k] = act
        sub.configure(text=text, fg=ACC_HI if act else MUTED, cursor='hand2' if act else '',
                      font=(UI, 9, 'underline') if act else (UI, 9))

    def rescue(self):
        if messagebox.askyesno('Rescue', 'For when every node is dead and nothing else works.\n\nThis PC tests the nodes in '
                               'the vault (every node that ever passed a test), puts the best 6 into the configs, and '
                               'updates the tunnel(s). If too few work, it scans the node list for more.'):
            self.work(self.do_rescue)

    def do_rescue(self):
        sentence = N.rescue(); ok = sentence.startswith('Rescue done')
        self.router = None; self.router_stale = not ok; self.pc_pending = self.want_restart = True
        for n in self.lists['phone']:
            n['phone_ms'] = n['phone_why'] = None
        self.check_local('both')
        self.status_text, self.status_state = sentence, 'ok' if ok and 'FAILED' not in sentence else 'warn'

    def do_replace_all(self):
        N.apply(N.scan(), 6)
        self.after_deploy('All slots were rewritten with the 6 best nodes from the scan.', True)

    def after_files(self, sentence):
        """Configs changed, nothing deployed."""
        self.router = None
        for n in self.lists['phone']:
            n['phone_ms'] = n['phone_why'] = None
        self.check_local('both')
        self.status_text, self.status_state = sentence + ' Nothing deployed yet. ' + PUSH_HINT, 'warn'

    def after_deploy(self, sentence, ask):
        self.router = None; self.router_stale = False; self.pc_pending, self.want_restart = True, ask
        for n in self.lists['phone']:
            n['phone_ms'] = n['phone_why'] = None
        self.check_local('both')
        self.status_text, self.status_state = sentence + (' PC tunnel updated.' if N.pc_only() else ' Router updated.' if RONLY else
                                                          ' Router and PC tunnel updated.'), 'ok'

    def push_phone(self):
        if messagebox.askyesno('Push to router', 'Send router-config.json and the device list to the router over SSH and '
                               'restart its sing-box? The router tunnel drops for a few seconds.'):
            self.work(self.do_push_phone)

    def do_push_phone(self):
        out = N.push(); self.say(out); ok = 'sing-box service: running' in out
        self.router = None; self.router_stale = not ok
        self.check_local('phone')
        self.status_text = 'Router updated and running.' if ok else 'Router NOT updated (see the activity log). router-config.json stays ready for the next try.'
        self.status_state = 'ok' if ok else 'bad'

    def do_push_pc(self):
        live = N.pc_push(); self.pc_pending = not live
        self.check_local('pc')
        self.status_text, self.status_state = ('PC tunnel restarted.', 'ok') if live else \
            ('PC tunnel did not come back; see the activity log and logs/core.log.', 'bad')

    def set_boot(self, on):
        if not N.boot_task(on):
            self.after(0, self.boot_on.set, not on)

    def rerun_setup(self, router=False):
        if messagebox.askyesno('Run setup again', 'Open the setup wizard? This window closes; the tunnel keeps running.\n\n'
                               'Nodes already in use are kept. Use it to add a GL.iNet router, redo the VPN step or the background start.'):
            restart_window(['--setup'] + (['--router'] if router else []))

    def power(self):
        if N.stopped():
            if messagebox.askyesno('Start Reality Hop', 'Start everything again on this PC?\n\n'
                                   '- the PC tunnel (rh-core) starts now and at every boot (one UAC prompt)\n'
                                   '- the background watcher and tray icon start now and at every logon\n\n'
                                   'Afterwards turn the VPN app\'s proxy back on (PIA: Settings → Multi-Hop, tick '
                                   '"Multi-hop and Obfuscation", SOCKS5 127.0.0.1:2080).'):
                self.work(self.do_power, False)
        elif messagebox.askyesno('Stop Reality Hop', 'Stop everything on this PC, for when you are somewhere else?\n\n'
                                 '- the PC tunnel (rh-core) stops and no longer starts at boot (one UAC prompt)\n'
                                 '- the background watcher and tray icon stop and no longer start at logon\n'
                                 '- the router and your configs are not touched\n\n'
                                 'Afterwards turn the VPN app\'s proxy off (PIA: Settings → Multi-Hop, untick "Multi-hop and '
                                 'Obfuscation"), or it cannot connect. Press Start to undo.'):
            self.work(self.do_power, True)

    def do_power(self, stop):
        res = N.manager_stop() if stop else N.manager_start()
        self.after(0, lambda: (self.power_btn.configure(text='Start' if N.stopped() else 'Stop'), self.bg_on.set(not N.stopped()),
                               self.boot_on.set(N.task_state() not in (None, 'Disabled'))))
        if not stop:
            self.check_local('pc')
        bad = [l for l in res if 'NOT' in l or 'not' in l.split(':', 1)[-1][:12] or 'still' in l]
        self.status_text = ('Reality Hop is stopped. ' if stop else 'Reality Hop is running. ') + ' '.join(res)
        self.status_state = 'warn' if stop or bad else 'ok'

    def pc_restart(self):
        self.do_push_pc(); N.hist_add(event=('restart', 'PC tunnel restarted from the window'))

    def auto_tick(self):
        if N.stopped():
            self.after(60000, self.auto_tick); self.next_lbl['text'] = 'stopped'; return
        try:
            mins = max(15, int(self.auto_min.get() or 60))
        except (tk.TclError, ValueError):
            mins = 60   # someone typed letters into the box
        if self.auto_on.get() and not self.busy:
            self.work(self.auto_run)
        self.next_lbl['text'] = f"next auto-check {time.strftime('%H:%M', time.localtime(time.time() + mins * 60))}" if self.auto_on.get() else ''
        self.after(mins * 60000, self.auto_tick)

    def auto_run(self):
        if self.auto_fix.get():
            out = N.watch_cycle(self.wstate, background=False)
            self.router = None; self.check_local('both')
            if out:
                self.status_text, self.status_state = 'Auto-repair: ' + out, 'ok'
            return
        self.check_local('both')
        due = [n['tag'] for n in self.lists[MAIN]
               if max(self.streak.get(('phone', n['server']), 0), self.streak.get(('pc', n['server']), 0)) >= 2]
        if due:
            self.status_text = f"{', '.join(due)} failed two checks in a row. Replace with the button, or tick auto-repair."; self.status_state = 'bad'


class NodeDetails(tk.Toplevel):
    """One node: where it is, how it tests, how often it passed, and its tunnel ping over the last 24 hours."""
    def __init__(self, app, key, n):
        super().__init__(app); self.configure(bg=BG); self.title(f"{n['tag']}  ·  {n['server']}"); self.geometry('760x520')
        v = N.vault_load().get(n['server'], {}); state, sentence = app.verdict(key, n)
        tk.Label(self, text=f"{n['tag']}  ·  {n['server']}", bg=BG, fg=FG, font=(UIB, 16)).pack(anchor='w', padx=22, pady=(18, 2))
        tk.Label(self, text=sentence, bg=BG, fg=STATE[state], font=(UI, 10)).pack(anchor='w', padx=22)
        c = card(self, fill='x', padx=22, pady=(14, 0))
        rows = (('Location', n.get('where') or v.get('where') or '?'), ('Host', v.get('isp') or n.get('isp') or '?'),
                ('Front site (SNI)', n.get('sni', '?')), ('443 ping from this PC', ms(n.get('tcp_ms'), 'no answer')),
                ('Through the PC tunnel', ms(n.get('pc_ms'), n.get('pc_why') or '-')),
                *((('Through the router tunnel', ms(n.get('phone_ms'), n.get('phone_why') or '-')),) if ROUTER else ()),
                ('Passed tests', f"{v.get('passes', 0)}x, last {v.get('last_ok', 'never')[:16]}"), ('Best tested', ms(v.get('ms'), '-')))
        for i, (k, val) in enumerate(rows):
            tk.Label(c, text=k, bg=CARD, fg=MUTED, font=(UI, 9)).grid(row=i, column=0, sticky='w', padx=(16, 24), pady=3)
            tk.Label(c, text=val, bg=CARD, fg=FG, font=(UI, 10)).grid(row=i, column=1, sticky='w', pady=3)
        tk.Label(self, text='TUNNEL PING, LAST 24 HOURS', bg=BG, fg=DIM, font=(UIB, 8)).pack(anchor='w', padx=22, pady=(16, 4))
        g = card(self, fill='both', expand=True, padx=22, pady=(0, 18))
        self.cv = tk.Canvas(g, bg=CARD, highlightthickness=0); self.cv.pack(fill='both', expand=True, padx=10, pady=10)
        self.pts = [x for x in N.hist_load()['pings'].get(n['server'], []) if x[0] > time.time() - 24 * 3600]
        self.cv.bind('<Configure>', lambda e: self.draw())
        dark_titlebar(self)

    def draw(self):
        c = self.cv; c.delete('all'); w, h = c.winfo_width(), c.winfo_height(); now = time.time()
        if not self.pts:
            c.create_text(w / 2, h / 2, text='No history yet. It fills in with every hourly check.', fill=MUTED, font=(UI, 10)); return
        top = max([p[1] for p in self.pts if p[1]] + [N.SLOW]) * 1.15
        y = lambda v: h - 18 - v / top * (h - 30)
        x = lambda t: 36 + (1 - (now - t) / (24 * 3600)) * (w - 46)
        for val in (0, N.SLOW):
            c.create_line(36, y(val), w - 10, y(val), fill=LINE, dash=(2, 3) if val else ())
            c.create_text(32, y(val), text=f'{val}', fill=DIM, font=(UI, 7), anchor='e')
        good = [(x(t), y(v)) for t, v in self.pts if v is not None]
        if len(good) > 1:
            c.create_line(*[k for pt in good for k in pt], fill=STATE['ok'], width=2, smooth=True)
        for t, v in self.pts:
            if v is None:
                c.create_text(x(t), h - 10, text='✕', fill=STATE['bad'], font=(UI, 9))
            else:
                c.create_oval(x(t) - 3, y(v) - 3, x(t) + 3, y(v) + 3, fill=STATE['warn' if v > N.SLOW else 'ok'], outline='')


class SettingsDialog(tk.Toplevel):
    """The numbers that used to be fixed in the code."""
    FIELDS = (('check_min', 'Check the nodes every (minutes)', 15, 1440, 15),
              ('slow_ms', 'A working node counts as slow above (ms)', 50, 5000, 25),
              ('tolerance_ms', 'Switch to a faster node when it is faster by (ms)', 0, 1000, 5),
              ('top', 'Candidates tested per scan', 10, 120, 10))

    def __init__(self, app):
        super().__init__(app); self.app = app; self.configure(bg=BG); self.title('Settings'); self.resizable(False, False)
        cur = N.settings_load(); self.vars = {}
        tk.Label(self, text='Settings', bg=BG, fg=FG, font=(UIB, 16)).pack(anchor='w', padx=22, pady=(18, 10))
        c = card(self, fill='x', padx=22)
        for i, (k, label, lo, hi, step) in enumerate(self.FIELDS):
            tk.Label(c, text=label, bg=CARD, fg=FG, font=(UI, 10)).grid(row=i, column=0, sticky='w', padx=(16, 20), pady=8)
            v = tk.IntVar(value=cur[k]); self.vars[k] = v
            ttk.Spinbox(c, from_=lo, to=hi, increment=step, width=7, textvariable=v).grid(row=i, column=1, padx=(0, 16))
        tk.Label(self, text='The switching margin takes effect on the next tunnel restart or router push.', bg=BG, fg=MUTED,
                 font=(UI, 9)).pack(anchor='w', padx=22, pady=(10, 0))
        c2 = card(self, fill='x', padx=22, pady=(14, 0)); c2.configure(bg=CARD)
        self.router_var, self.upd_var = tk.BooleanVar(value=bool(cur['router'])), tk.BooleanVar(value=bool(cur['auto_update']))
        self.beta_var, self.files_var = tk.BooleanVar(value=cur['singbox'] == 'testing'), tk.BooleanVar(value=bool(cur['backend_files']))
        self.menu_var, self.desk_var = tk.BooleanVar(value=bool(cur['start_menu'])), tk.BooleanVar(value=bool(cur['desktop_icon']))
        self.channel, self.icons = cur['singbox'], (cur['start_menu'], cur['desktop_icon'])
        self.ronly_var = tk.BooleanVar(value=RONLY)
        for text, var in (('Router features (GL.iNet): show the router section', self.router_var),
                          ('Router only: no tunnel on this PC; the router tunnels your devices', self.ronly_var),
                          ('Install Reality Hop updates automatically', self.upd_var),
                          ('sing-box test builds (beta, alpha) instead of stable; updated daily either way', self.beta_var),
                          ('Show backend files: buttons for the PC, Router and logs folders', self.files_var),
                          ('Show Reality Hop in the Start menu and Windows search', self.menu_var),
                          ('Desktop icon', self.desk_var)):
            t = Toggle(c2, text, var); t.configure(bg=CARD); t.c.configure(bg=CARD); t.lab.configure(bg=CARD, font=(UI, 10))
            t.pack(anchor='w', padx=16, pady=(10, 0))
        tk.Label(c2, text=f'Reality Hop {N.VERSION}', bg=CARD, fg=MUTED, font=(UI, 9)).pack(anchor='w', padx=16, pady=(12, 2))
        for text, url in ((f'github.com/{N.REPO}', f'https://github.com/{N.REPO}'),
                          ("What's new (changelog)", f'https://github.com/{N.REPO}/blob/main/CHANGELOG.md'),
                          ('Nodes come from github.com/rtwo2/FastNodes', 'https://github.com/rtwo2/FastNodes'),
                          ('Tunnel engine: github.com/SagerNet/sing-box', 'https://github.com/SagerNet/sing-box')):
            link(c2, text, url).pack(anchor='w', padx=16)
        row2 = tk.Frame(c2, bg=CARD); row2.pack(fill='x', padx=16, pady=(4, 14))
        for text, cmd in (('Check for updates', self.check_updates), ('Run setup again', lambda: (self.destroy(), app.rerun_setup())),
                          *((('Change router', self.change_router),) if ROUTER else ()), ('Uninstall', self.uninstall)):
            ttk.Button(row2, text=text, style='Ghost.TButton', command=cmd).pack(side='left', padx=(0, 8))
        row = tk.Frame(self, bg=BG); row.pack(fill='x', padx=22, pady=16)
        ttk.Button(row, text='Save', style='Accent.TButton', command=self.save).pack(side='right')
        ttk.Button(row, text='Defaults', command=lambda: [v.set(N.DEFAULTS[k]) for k, v in self.vars.items()]).pack(side='right', padx=8)
        dark_titlebar(self)

    def change_router(self):
        if messagebox.askyesno('Change router', 'For a new router, a reset router or a new router address.\n\nReality Hop '
                               'forgets the old router and opens the router steps of the setup, where you connect the new one. '
                               'This window closes; the PC tunnel keeps running.', parent=self):
            N.router_forget(); restart_window(['--setup', '--router'])

    def check_updates(self):
        try:
            rel = install.check_update() if N.FROZEN else None
        except Exception as e:
            messagebox.showerror('Updates', f'Could not reach GitHub: {e}', parent=self); return
        if rel:
            self.app.update_rel = rel; self.destroy(); self.app.do_update()
        else:
            messagebox.showinfo('Updates', f'Reality Hop {N.VERSION} is the newest version.' if N.FROZEN else
                                'Running from source: update with git pull.', parent=self)

    def uninstall(self):
        if not messagebox.askyesno('Uninstall Reality Hop', 'Remove Reality Hop from this PC?\n\nThe tunnel, the background watcher, the '
                                   'boot start (one UAC prompt), the shortcuts and the program are removed.', parent=self):
            return
        keep = messagebox.askyesno('Uninstall Reality Hop', 'Keep your settings and node vault for a later install?', parent=self)
        router = (not N.pc_only()) and messagebox.askyesno('Uninstall Reality Hop', 'Also switch the tunnel on the router off?', parent=self)
        out = install.uninstall(keep_data=keep, router_too=router)
        messagebox.showinfo('Uninstall Reality Hop', 'Reality Hop is removed.\n\n' + '\n'.join(out), parent=self)
        os._exit(0)

    def save(self):
        try:
            d = N.settings_save(dict({k: v.get() for k, v in self.vars.items()}, router=int(self.router_var.get()),
                                     auto_update=int(self.upd_var.get()), singbox='testing' if self.beta_var.get() else 'stable',
                                     backend_files=int(self.files_var.get()), start_menu=int(self.menu_var.get()),
                                     desktop_icon=int(self.desk_var.get()), pc_tunnel=int(not self.ronly_var.get())))
        except (tk.TclError, ValueError):
            messagebox.showerror('Settings', 'Numbers only, please.', parent=self); return
        if self.ronly_var.get() and not d['router']:
            N.settings_save({'pc_tunnel': 1}); d = N.settings_load()
            messagebox.showinfo('Settings', 'Router only needs router features on; the tunnel stays on this PC.', parent=self)
        if N.router_only() != RONLY:   # the PC tunnel goes off or on, then the window opens again in the other layout
            if N.router_only():
                N.core_stop()
                if N.boot_task_exists():   # nothing to start at boot any more (one admin prompt)
                    N.boot_task(False)
                messagebox.showinfo('Router only', 'The tunnel on this PC is off now. If your VPN app uses the SOCKS5 proxy '
                                    '127.0.0.1:2080, turn that off (PIA: Settings → Multi-Hop, untick "Multi-hop and '
                                    'Obfuscation"). To tunnel this PC through the router, tick it in Router > Devices, or '
                                    'choose every device.', parent=self)
            else:
                N.core_restart()
            restart_window()
        self.app.auto_min.set(d['check_min']); self.app.say(f'settings saved: {d}'); self.destroy()
        channel = d['singbox'] != self.channel   # a new window picks the channel change up by itself
        if (d['start_menu'], d['desktop_icon']) != self.icons and N.FROZEN:
            install.shortcuts()
        if bool(d['router']) != ROUTER:
            if d['router'] and not os.path.exists(d['router_key'] or N.ROUTER_KEY):
                if messagebox.askyesno('Router features', 'The router is not set up yet. Open the router steps of the setup wizard now?'):
                    restart_window(['--setup', '--router'])
                N.settings_save({'router': 0})
            else:
                restart_window()
        if bool(d['backend_files']) != BACKEND:
            restart_window()
        if channel:
            self.app.say(f"sing-box channel: {d['singbox']}; installing that build now...")
            self.app.work(self.app.core_update)


class Candidates(tk.Toplevel):
    """Node-list candidates that could replace yours. Click one, Ctrl+click more: the click order numbers them vless-1,
    vless-2, ... Put in slots writes them into the configs. Restarting the tunnel(s) is separate."""
    COLS = (('num', 44), ('server', 130), ('front', 150), ('where', 190), ('tcp', 120), ('tunnel', 140), ('result', 340))

    def __init__(self, app):
        super().__init__(app); self.app, self.nodes, self.order, self.ctx = app, [], [], None
        self.msg = 'Downloading the node list and pinging from this PC...'
        self.title('Candidates'); self.geometry('1180x800'); self.minsize(940, 560); self.configure(bg=BG)
        pad = dict(padx=24)
        slot = tk.Frame(self, bg=BG, height=3); slot.pack(fill='x'); slot.pack_propagate(False)
        self.prog = ttk.Progressbar(slot, mode='indeterminate', style='Busy.Horizontal.TProgressbar')
        ttk.Label(self, text='Candidates', style='Title.TLabel').pack(anchor='w', pady=(18, 0), **pad)
        ttk.Label(self, text='Nodes from the public node list that could replace yours, fastest 443 first. Click one, Ctrl+click more: '
                  'the click order numbers them vless-1, vless-2, ... For one specific slot, click a node and then a vless button below. A 443 answer proves '
                  'nothing; only a test shows which ones actually work.', style='Muted.TLabel', wraplength=1100, justify='left'
                  ).pack(anchor='w', pady=(8, 0), **pad)
        c = card(self, fill='both', expand=True, pady=(14, 0), **pad)
        self.tree = table(c, self.COLS, stretch='result', mode='extended')
        sb = ttk.Scrollbar(c, command=self.tree.yview); self.tree.configure(yscrollcommand=sb.set)
        sb.pack(side='right', fill='y'); self.tree.pack(fill='both', expand=True, padx=8, pady=(2, 8))
        self.tree.bind('<<TreeviewSelect>>', self.on_select); self.tree.bind('<Button-3>', self.popup)
        self.menu = menu(self)
        for i in range(6):
            self.menu.add_command(label=f'Put this one into vless-{i + 1}  (configs only)', command=lambda i=i: self.put_one(i))
        self.buttons = []
        def btn(parent, text, cmd, accent=False):
            w = ttk.Button(parent, text=text, command=cmd, style='Accent.TButton' if accent else 'TButton'); w.pack(side='left', padx=(0, 8)); self.buttons.append(w)
        bar1 = ttk.Frame(self); bar1.pack(fill='x', pady=(12, 0), **pad)
        btn(bar1, 'Ping again from this PC', lambda: self.app.work(self.ping))
        btn(bar1, 'Test 40 fastest', lambda: self.app.work(self.test))
        self.pick_lbl = ttk.Label(bar1, text='', style='Muted.TLabel'); self.pick_lbl.pack(side='right')
        bar2 = ttk.Frame(self); bar2.pack(fill='x', pady=(8, 0), **pad)
        btn(bar2, 'Replace all 6 with the best 6', self.put_best, True)
        btn(bar2, 'Put picked in slots', self.put_all, True)
        if not RONLY:
            btn(bar2, 'Restart PC tunnel', lambda: self.app.work(self.app.pc_restart))
            btn(bar2, 'Test PC tunnel', lambda: self.app.work(self.test_tunnel))
        if ROUTER:
            btn(bar2, 'Push to router', self.app.push_phone)
        bar3 = ttk.Frame(self); bar3.pack(fill='x', pady=(8, 0), **pad)
        ttk.Label(bar3, text='Put the selected node into:', style='Muted.TLabel').pack(side='left', padx=(0, 10))
        for i in range(6):
            btn(bar3, f'vless-{i + 1}', lambda i=i: self.put_slot(i))
        self.status = strip(self, self.msg, pad)
        self.status.master.master.pack_configure(pady=(12, 18))
        self.app.buttons += self.buttons; self.protocol('WM_DELETE_WINDOW', self.close)
        dark_titlebar(self); self.app.work(self.ping)

    def close(self):
        self.app.buttons = [b for b in self.app.buttons if b not in self.buttons]; self.destroy()

    def slots(self):
        return len(self.app.lists[MAIN]) or 6

    def on_select(self, e=None):
        sel = self.tree.selection()
        self.order = [s for s in self.order if s in sel] + [s for s in sel if s not in self.order]
        if len(self.order) > self.slots():
            extra = self.order[self.slots():]; self.order = self.order[:self.slots()]
            self.tree.selection_remove(*extra); return   # fires this handler again with the trimmed selection
        for iid in self.tree.get_children():
            self.tree.set(iid, 'num', str(self.order.index(iid) + 1) if iid in self.order else '')
        self.pick_lbl['text'] = ('  '.join(f'vless-{i + 1} <- {s}' for i, s in enumerate(self.order))
                                 if self.order else 'no candidates picked')

    def popup(self, e):
        row = self.tree.identify_row(e.y)
        if row:
            self.ctx = row; self.menu.tk_popup(e.x_root, e.y_root)

    def ping(self):
        self.app.say('candidates: downloading the node list...')
        used = {n['server'] for n in self.app.lists['phone']} | {n['server'] for n in self.app.lists['pc']}
        c = [n for n in N.feed() if n['server'] not in used]
        self.app.say(f'candidates: {len(c)} with plausible fronts; pinging 443 from this PC...')
        where, failed = N.known_where(), N.known_failed()
        for n in c:
            n['where'] = where.get(n['server'], '?')
            if n['server'] in failed:
                n['ok'], n['why'], n['ms'] = False, 'failed the last scan', None
        self.nodes = sorted(N.probe(c), key=lambda n: (n.get('ok') is False, n['tcp_ms'])); self.order = []   # known-failed last
        self.msg = f'{len(self.nodes)} of {len(c)} candidates answer on 443 from this PC. None is tunnel-tested yet.'

    def test(self):
        if not self.nodes:
            self.msg = 'Nothing to test; ping first.'; return
        top = [n for n in self.nodes if n.get('ok') is not False][:N.TOP]
        self.app.say(f'candidates: this PC is testing the {len(top)} fastest, 5 at a time (about 20 seconds)...')
        N.router_test(top)
        ok = sorted([n for n in top if n.get('ok')], key=lambda n: n['ms'])
        self.nodes.sort(key=lambda n: (n.get('ok') is not True, n['ms'] if n.get('ok') else 9e9, n['tcp_ms']))
        self.msg = (f'{len(ok)} of {len(top)} tested candidates work.'
                    + (f" Best: {ok[0]['server']} at {ok[0]['ms']} ms ({ok[0].get('where', '?')})." if ok else ' Ping again later or widen the test.'))

    def test_tunnel(self):
        """The running PC tunnel fetches through each node in its slots, as Check now does."""
        self.app.check_local('pc'); nodes = self.app.lists['pc']
        dead = [n['tag'] for n in nodes if n.get('pc_ms') is None]
        if not (self.app.pcs or {}).get('api'):
            self.msg = 'The PC tunnel is not running. Press Restart PC tunnel, then test again.'
        elif self.app.pc_pending:
            self.msg = 'The PC tunnel still runs the previous nodes. Press Restart PC tunnel, then test again.'
        else:
            self.msg = (f'PC tunnel: {len(nodes) - len(dead)} of {len(nodes)} nodes work'
                        + (f"; not working: {', '.join(dead)}." if dead else '.'))

    def render(self):
        if not self.winfo_exists():
            return
        self.tree.delete(*self.tree.get_children())
        for i, n in enumerate(self.nodes):
            if n.get('ok') is True:
                state, res = 'ok', f"works, {n['ms']} ms in the test, exit {n['exit']}"
            elif n.get('ok') is False:
                state, res = 'bad', f"failed the test: {n['why']}"
            else:
                state, res = 'warn', 'answers on 443; not tunnel-tested'
            self.tree.insert('', 'end', iid=n['server'], tags=(state, 'odd' if i % 2 else 'even'),
                             values=('', n['server'], n['sni'], n.get('where', '?'), ms(n['tcp_ms'], 'no answer'), ms(n.get('ms'), ''), res))
        self.order = [s for s in self.order if self.tree.exists(s)]
        if self.order:
            self.tree.selection_set(*self.order)
        else:
            self.on_select()
        self.status.configure(text=self.msg)

    def picks(self, pairs):
        return '\n'.join(f"vless-{i + 1}  <-  {n['server']}  ({'tested, works' if n.get('ok') else 'NOT tunnel-tested'})" for i, n in pairs)

    def put_all(self):
        if not self.order:
            messagebox.showinfo('Candidates', 'Pick candidates first: click one, Ctrl+click more. The order gives the slot numbers.', parent=self); return
        by = {n['server']: n for n in self.nodes}
        pairs = [(i, by[s]) for i, s in enumerate(self.order)]
        if messagebox.askyesno('Put in slots', 'Write these into the configs? Nothing is restarted until you say so.\n\n' + self.picks(pairs), parent=self):
            self.app.work(self.do_put, pairs)

    def put_best(self):
        """The best nodes into every slot: tested working ones first (fastest tunnel), then fastest 443 ping."""
        k = self.slots()
        ok = sorted([n for n in self.nodes if n.get('ok')], key=lambda n: n['ms'])
        rest = [n for n in self.nodes if n.get('ok') is None]
        best = (ok + rest)[:k]
        if not best:
            messagebox.showinfo('Candidates', 'No candidates yet; ping first.', parent=self); return
        note = '' if len(ok) >= k else (f'\n\nOnly {len(ok)} of these passed the test. Run "Test {N.TOP} fastest" first '
                                        'to fill every slot with a node that is known to work.')
        if messagebox.askyesno('Replace all', 'Write these into the configs? Nothing is restarted until you say so.\n\n'
                               + self.picks(list(enumerate(best))) + note, parent=self):
            self.app.work(self.do_put, list(enumerate(best)))

    def put_slot(self, slot):
        sel = self.tree.selection()
        if len(sel) != 1:
            messagebox.showinfo('Candidates', 'Select one node in the list (a single click), then click the vless slot it goes into.', parent=self); return
        self.ctx = sel[0]; self.put_one(slot)

    def put_one(self, slot):
        if not self.ctx:
            return
        n = next(x for x in self.nodes if x['server'] == self.ctx)
        if messagebox.askyesno('Put in slot', 'Write this into the configs? Nothing is restarted until you say so.\n\n' + self.picks([(slot, n)]), parent=self):
            self.app.work(self.do_put, [(slot, n)])

    def do_put(self, pairs):
        swaps = N.swap_files(pairs)
        used = {n['server'] for _, n in pairs}
        self.nodes = [x for x in self.nodes if x['server'] not in used]; self.order = []
        sentence = ' '.join(f"{tag}: {old} -> {new['server']}" + (f" ({new['ms']} ms, tested)." if new.get('ok') else ' (not tunnel-tested).')
                            for tag, old, new in swaps)
        self.app.after_files('Configs updated. ' + sentence); self.msg = self.app.status_text


class DevicesDialog(tk.Toplevel):
    """The devices the router puts through the tunnel: tick to add, untick to remove, then push. Also how to keep a
    device's address from changing, which is what the router goes by."""
    def __init__(self, app):
        super().__init__(app); self.app = app; self.configure(bg=BG); self.title('Router devices'); self.geometry('900x760')
        self.picked, self.shown = set(N.devices()), set()
        tk.Label(self, text='Router devices', bg=BG, fg=FG, font=(UIB, 16)).pack(anchor='w', padx=22, pady=(18, 4))
        self.all_var = tk.BooleanVar(value=N.all_devices())
        t = Toggle(self, 'Every device on the main network (this PC too), not just the ticked ones', self.all_var,
                   lambda: self.fill([(i, self.tree.set(i, 'name')) for i in self.tree.get_children() if i in self.shown]))
        t.configure(bg=BG); t.c.configure(bg=BG); t.lab.configure(bg=BG, font=(UI, 10)); t.pack(anchor='w', padx=22, pady=(8, 4))
        tk.Label(self, text='Ticked devices go through the tunnel on the router; everything else on your network stays as it is. '
                 'Click a row to tick or untick it, then press Save and push.', bg=BG, fg=MUTED, font=(UI, 10), wraplength=850,
                 justify='left').pack(anchor='w', padx=22)
        c = card(self, fill='x', padx=22, pady=(12, 0))
        self.tree = table(c, (('num', 40), ('ip', 150), ('name', 300), ('state', 300)), height=9, stretch='state')
        for k, t in (('num', ''), ('ip', 'ADDRESS'), ('name', 'DEVICE'), ('state', 'STATUS')):
            self.tree.heading(k, text=t)
        self.tree.pack(fill='x', padx=8, pady=6); self.tree.bind('<Button-1>', self.toggle)
        row = tk.Frame(self, bg=BG); row.pack(fill='x', padx=22, pady=(10, 0))
        tk.Label(row, text='Add an address', bg=BG, fg=FG, font=(UI, 10)).pack(side='left')
        self.extra = tk.StringVar()
        tk.Entry(row, textvariable=self.extra, width=20, bg=BTN, fg=FG, insertbackground=FG, relief='flat', font=(UI, 10)).pack(side='left', padx=10, ipady=4)
        ttk.Button(row, text='Add', command=self.add).pack(side='left')
        ttk.Button(row, text='Refresh list', command=lambda: threading.Thread(target=self.load, daemon=True).start()).pack(side='left', padx=8)
        info = card(self, fill='x', padx=22, pady=(14, 0))
        tk.Label(info, text='KEEP EACH DEVICE ON THE SAME ADDRESS', bg=CARD, fg=DIM, font=(UIB, 8)).pack(anchor='w', padx=16, pady=(12, 4))
        for i, t in enumerate(('On the phone, turn off the changing ("private" or "random") Wi-Fi address for this network, so it '
                               'keeps one MAC address. iPhone: Settings → Wi-Fi → (i) next to the network → Private Wi-Fi Address: '
                               'Off (or Fixed). Android: Wi-Fi → the network → Privacy → Use device MAC. Names vary by phone.',
                               'Then reserve its address on the router: GL.iNet web panel > CLIENTS, then turn on the Reserved IP '
                               'slider next to the device.'), 1):
            tk.Label(info, text=f'{i}.  {t}', bg=CARD, fg=FG, font=(UI, 10), wraplength=820, justify='left').pack(anchor='w', padx=16, pady=(0, 6))
        host = N.settings_load()['router_host']
        link(info, f'Open the GL.iNet web panel (http://{host})', f'http://{host}').pack(anchor='w', padx=16, pady=(0, 12))
        bar = tk.Frame(self, bg=BG); bar.pack(fill='x', padx=22, pady=16)
        self.msg = tk.Label(bar, text='Reading the router\'s device list...', bg=BG, fg=MUTED, font=(UI, 10), anchor='w')
        self.msg.pack(side='left', fill='x', expand=True)
        ttk.Button(bar, text='Close', command=self.destroy).pack(side='right')
        self.save_btn = ttk.Button(bar, text='Save and push', style='Accent.TButton', command=self.save)
        self.save_btn.pack(side='right', padx=8)
        dark_titlebar(self); threading.Thread(target=self.load, daemon=True).start()

    def load(self):
        try:
            rows, err = N.router_clients(), ''
        except Exception as e:
            rows, err = [], f'Could not read the router ({e}); you can still add addresses by hand.'
        self.after(0, self.fill, rows, err)

    def fill(self, rows, err=''):
        if not self.winfo_exists():
            return
        names = dict(rows); self.shown = set(names)
        self.tree.delete(*self.tree.get_children())
        for i, ip in enumerate(sorted(self.shown | self.picked, key=lambda x: tuple(int(k) for k in x.split('.')))):
            on = ip in self.picked
            state = ('tunnelled' if ip in N.devices() else 'added, press Save and push') if on else 'not tunnelled'
            if on and ip not in self.shown:
                state += '  ·  not connected now'
            self.tree.insert('', 'end', iid=ip, values=('✓' if on else '', ip, names.get(ip, '(not in the router list)'), state),
                             tags=('odd' if i % 2 else 'even', 'ok' if on else 'unk'))
        self.msg.configure(text=err or ('Every device on the main network goes through the tunnel.' if self.all_var.get() else
                                        f'{len(self.picked)} device(s) ticked.'), fg=STATE['bad'] if err else MUTED)

    def toggle(self, e):
        ip = self.tree.identify_row(e.y)
        if ip:
            self.picked ^= {ip}; self.fill([(i, self.tree.set(i, 'name')) for i in self.tree.get_children() if i in self.shown])
        return 'break'

    def add(self):
        ip = self.extra.get().strip()
        if not N.lan_ip(ip):
            self.msg.configure(text='That is not a local network address like 192.168.8.20.', fg=STATE['bad']); return
        self.picked.add(ip); self.extra.set('')
        self.fill([(i, self.tree.set(i, 'name')) for i in self.tree.get_children() if i in self.shown])

    def save(self):
        if self.app.busy:
            self.msg.configure(text='Wait for the current action to finish, then press Save and push again.', fg=STATE['warn']); return
        ips = ['all'] if self.all_var.get() else sorted(self.picked, key=lambda x: tuple(int(k) for k in x.split('.')))
        what = ('every device on the main network, this PC included' if ips == ['all'] else '\n'.join(ips) or
                '(none: the router tunnel then carries no device)')
        if not messagebox.askyesno('Router devices', f'Push this to the router?\n\n{what}\n\nThe router tunnel restarts; it drops for '
                                   'a few seconds.', parent=self):
            return
        N.settings_save({'devices': ', '.join(ips)}); self.msg.configure(text='Pushing to the router...', fg=MUTED)
        self.app.work(self.push)

    def push(self):
        out = N.push(); self.app.say(out); ok = 'sing-box service: running' in out
        who = 'every device on the main network goes' if N.all_devices() else (', '.join(N.devices()) or 'no devices') + ' go'
        self.app.status_text, self.app.status_state = ((f'Router updated: {who} through the tunnel.', 'ok')
                                                       if ok else ('Router NOT updated; see the activity log.', 'bad'))
        self.after(0, lambda: self.winfo_exists() and (self.msg.configure(text=self.app.status_text, fg=STATE['ok' if ok else 'bad']),
                                                       self.load()))


class UpdateDialog(tk.Toplevel):
    """A new release: what changed in every version since this one, then install now or later."""
    def __init__(self, app, rel):
        super().__init__(app); self.app, self.rel = app, rel
        self.configure(bg=BG); self.title('Update available'); self.geometry('760x620'); self.minsize(600, 440); self.transient(app)
        tk.Label(self, text=f"Reality Hop {rel['ver']} is ready", bg=BG, fg=FG, font=(UIB, 17)).pack(anchor='w', padx=26, pady=(22, 2))
        tk.Label(self, text=f"You have {N.VERSION}. The update takes a few seconds, and the tunnel keeps running.",
                 bg=BG, fg=MUTED, font=(UI, 10)).pack(anchor='w', padx=26)
        row = tk.Frame(self, bg=BG); row.pack(side='bottom', fill='x', padx=26, pady=18)   # first, so it always shows
        c = card(self, fill='both', expand=True, padx=26, pady=(16, 0))
        t = tk.Text(c, wrap='word', height=1, bg=CARD, fg=FG, font=(UI, 10), relief='flat', highlightthickness=0, padx=20, pady=16,
                    spacing1=2, spacing3=2, cursor='arrow')
        sb = ttk.Scrollbar(c, command=t.yview); t.configure(yscrollcommand=sb.set)
        sb.pack(side='right', fill='y'); t.pack(fill='both', expand=True)
        t.tag_configure('ver', font=(UIB, 13), foreground=FG, spacing1=10)
        t.tag_configure('date', font=(UI, 9), foreground=MUTED)
        t.tag_configure('p', spacing3=8)
        t.tag_configure('li', lmargin1=6, lmargin2=19, spacing3=6)
        t.tag_configure('dot', foreground=ACC, font=(UIB, 10))
        t.tag_configure('b', font=(UIB, 10))
        t.tag_configure('code', font=('Consolas', 9), background=ROW2)
        for i, (ver, date, notes) in enumerate(rel.get('changes') or [(rel['ver'], '', rel['notes'])]):
            if i:
                t.insert('end', '\n')
            t.insert('end', f'Version {ver}', ('ver',)); t.insert('end', f'   {date}' if date else '', ('ver', 'date')); t.insert('end', '\n')
            for kind, text in md_blocks(notes):
                if kind == 'li':
                    t.insert('end', '•  ', ('li', 'dot')); md_insert(t, text, ('li',))
                else:
                    md_insert(t, text, ('p',))
                t.insert('end', '\n', (kind,))
        t.configure(state='disabled')
        ttk.Button(row, text='Install update', style='Accent.TButton', command=self.install).pack(side='left')
        ttk.Button(row, text='Later', command=self.destroy).pack(side='left', padx=8)
        link(row, 'All changes on GitHub', f'https://github.com/{N.REPO}/blob/main/CHANGELOG.md', bg=BG).pack(side='right')
        dark_titlebar(self); self.lift(); self.focus_force()

    def install(self):
        self.destroy(); self.app.work(self.app.apply_update, self.rel)


class ReportDialog(tk.Toplevel):
    """Report a problem: an anonymous copy of the three logs plus a short summary, shown before anything leaves this PC."""
    def __init__(self, app):
        super().__init__(app); self.configure(bg=BG); self.title('Report a problem'); self.geometry('1000x740')
        self.path = self.text = None
        tk.Label(self, text='Report a problem', bg=BG, fg=FG, font=(UIB, 16)).pack(anchor='w', padx=22, pady=(18, 4))
        tk.Label(self, text='This makes a redacted report from the three logs (window, watch, core) and a short summary of your '
                 'setup. Addresses, MAC addresses, your user and computer name, folder paths, e-mail addresses, keys and secrets '
                 'are redacted. Nothing is sent: read it, then attach the file to a GitHub issue.',
                 bg=BG, fg=MUTED, font=(UI, 10), wraplength=940, justify='left').pack(anchor='w', padx=22)
        c = card(self, fill='both', expand=True, padx=22, pady=(12, 0))
        self.txt = tk.Text(c, wrap='none', font=('Consolas', 9), bg=CARD, fg=FG, relief='flat', highlightthickness=0, padx=12, pady=8)
        sb = ttk.Scrollbar(c, command=self.txt.yview); self.txt.configure(yscrollcommand=sb.set)
        sb.pack(side='right', fill='y'); self.txt.pack(fill='both', expand=True)
        self.txt.insert('end', 'Making the report...'); self.txt.configure(state='disabled')
        row = tk.Frame(self, bg=BG); row.pack(fill='x', padx=22, pady=16); self.btns = []
        for text, cmd, accent in (('Open GitHub issue', self.issue, True), ('Open folder', lambda: os.startfile(N.REPORTS), False),
                                  ('Copy', self.copy, False)):
            b = ttk.Button(row, text=text, command=cmd, style='Accent.TButton' if accent else 'TButton')
            b.pack(side='left', padx=(0, 8)); b.state(['disabled']); self.btns.append(b)
        ttk.Button(row, text='Close', command=self.destroy).pack(side='right')
        dark_titlebar(self); threading.Thread(target=self.make, daemon=True).start()

    def make(self):
        try:
            self.path, self.text = report.make(); shown = self.text
        except Exception as e:
            shown = f'Could not make the report: {e}'
        self.after(0, self.show, shown)

    def show(self, shown):
        if not self.winfo_exists():
            return
        self.txt.configure(state='normal'); self.txt.delete('1.0', 'end'); self.txt.insert('end', shown); self.txt.configure(state='disabled')
        if self.path:
            self.title(f'Report a problem  ·  {os.path.basename(self.path)}')
            for b in self.btns:
                b.state(['!disabled'])

    def issue(self):
        import webbrowser
        webbrowser.open(report.issue_url(self.path, self.text)); os.startfile(N.REPORTS)

    def copy(self):
        self.clipboard_clear(); self.clipboard_append(self.text)


def restart_window(args=()):
    """Close this window and open Reality Hop again (with args, e.g. --setup for the wizard)."""
    cmd = ([N.exe()] if N.FROZEN else [N.pythonw(), os.path.abspath(__file__)]) + list(args)
    subprocess.Popen(cmd, cwd=N.DATA if os.path.isdir(N.DATA) else None, env=N.own_env(),
                     creationflags=subprocess.DETACHED_PROCESS); os._exit(0)


class Wizard(tk.Tk):
    """The setup, one step at a time: install and first nodes, an optional GL.iNet router, the VPN app, background start."""
    STEPS = (('welcome', 'Welcome'), ('install', 'Install'), ('router', 'Router?'), ('ssh', 'Router: connect'),
             ('box', 'Router: sing-box'), ('devices', 'Router: devices'), ('vpn', 'VPN app'), ('finish', 'Finish'))
    GATES = {'install', 'ssh', 'box', 'devices'}   # pages whose Next waits for their action to succeed

    def __init__(self, router_only=False):
        super().__init__()
        self.title('Reality Hop setup'); self.geometry('1120x800'); self.minsize(1000, 700); self.configure(bg=BG)
        self.q, self.busy, self.ok, self.logw, self.note_lbl = queue.Queue(), False, set(), None, None
        self.use_router, self.picked, self.img = router_only or not N.pc_only(), set(), None
        self.lock = socket.socket()
        try:
            self.lock.bind(('127.0.0.1', N.GUI_PORT))   # the background watcher pauses while the setup runs
        except OSError:
            pass
        os.makedirs(N.LOGS, exist_ok=True); N.say = self.say; self.flog = N.file_log('window')
        if N.is_setup() and os.path.exists(N.PC_CFG) and N.nodes_of(N.PC_CFG):
            self.ok.add('install')   # running the wizard again: the PC part is already done
        try:
            import ctypes; ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID('RealityHop.App')
            self.iconbitmap(default=os.path.join(N.RES, 'app.ico'))
        except Exception:
            pass
        theme(self)
        side = tk.Frame(self, bg=SIDE, width=230); side.pack(side='left', fill='y'); side.pack_propagate(False)
        tk.Frame(self, bg=LINE, width=1).pack(side='left', fill='y')
        brand = tk.Frame(side, bg=SIDE); brand.pack(fill='x', padx=18, pady=(20, 4))
        tk.Label(brand, text='●', bg=SIDE, fg=ACC, font=(UIB, 14)).pack(side='left')
        tk.Label(brand, text=' Reality Hop', bg=SIDE, fg=FG, font=(UIB, 15)).pack(side='left')
        tk.Label(side, text=f'setup  ·  v{N.VERSION}', bg=SIDE, fg=MUTED, font=(UI, 9)).pack(anchor='w', padx=20, pady=(0, 22))
        self.step_labels = {}
        for key, name in self.STEPS:
            lab = tk.Label(side, text=name, bg=SIDE, fg=DIM, font=(UI, 10), anchor='w', padx=22, pady=6)
            lab.pack(fill='x'); self.step_labels[key] = lab
        main = tk.Frame(self, bg=BG); main.pack(side='left', fill='both', expand=True)
        slot = tk.Frame(main, bg=BG, height=3); slot.pack(fill='x'); slot.pack_propagate(False)
        self.prog = ttk.Progressbar(slot, mode='indeterminate', style='Busy.Horizontal.TProgressbar')
        nav = tk.Frame(main, bg=BG); nav.pack(side='bottom', fill='x', padx=34, pady=(0, 20))
        tk.Frame(main, bg=LINE, height=1).pack(side='bottom', fill='x', padx=34, pady=(0, 16))
        self.next_btn = ttk.Button(nav, text='Next', style='Accent.TButton', command=self.next); self.next_btn.pack(side='right')
        self.back_btn = ttk.Button(nav, text='Back', command=self.back); self.back_btn.pack(side='right', padx=8)
        self.body = tk.Frame(main, bg=BG); self.body.pack(fill='both', expand=True, padx=34, pady=(24, 10))
        dark_titlebar(self); self.show('router' if router_only else 'welcome'); self.after(150, self.drain)

    # ---- plumbing
    def say(self, *a):
        line = ' '.join(str(x) for x in a); self.q.put(line); self.flog(line)

    def drain(self):
        try:
            while True:
                line = self.q.get_nowait()
                if self.logw and self.logw.winfo_exists():
                    self.logw.configure(state='normal'); self.logw.insert('end', time.strftime('%H:%M:%S  '), 't')
                    self.logw.insert('end', line + '\n', 'err' if line.startswith(('error', 'FAIL')) or 'NOT ' in line else ())
                    self.logw.see('end'); self.logw.configure(state='disabled')
        except queue.Empty:
            pass
        self.after(150, self.drain)

    def work(self, fn, *args):
        if self.busy:
            return
        self.busy = True; self.prog.pack(fill='x'); self.prog.start(14); self.nav()
        def go():
            try:
                fn(*args)
            except Exception as e:
                self.say(f'error: {e}'); self.after(0, self.note, f'Something failed: {e}', 'bad')
            finally:
                self.after(0, self.idle)
        threading.Thread(target=go, daemon=True).start()

    def idle(self):
        self.busy = False; self.prog.stop(); self.prog.pack_forget(); self.nav()

    def seq(self):
        return [k for k, _ in self.STEPS if self.use_router or k not in ('ssh', 'box', 'devices')]

    def nav(self):
        ok = not self.busy and (self.page not in self.GATES or self.page in self.ok) and self.page != 'router'
        self.next_btn.state(['!disabled'] if ok else ['disabled'])
        self.back_btn.state(['disabled'] if self.busy or self.page == self.seq()[0] else ['!disabled'])
        self.next_btn.configure(text='Finish' if self.page == 'finish' else 'Next')
        seq, here = self.seq(), self.seq().index(self.page)
        for k, lab in self.step_labels.items():
            if k not in seq:
                lab.configure(fg='#3a3a43', font=(UI, 10), bg=SIDE)
            elif k == self.page:
                lab.configure(fg=FG, font=(UIB, 10), bg=HOVER)
            else:
                lab.configure(fg=STATE['ok'] if seq.index(k) < here else MUTED, font=(UI, 10), bg=SIDE)

    def show(self, page):
        self.page = page
        for w in self.body.winfo_children():
            w.destroy()
        self.logw = self.note_lbl = None
        getattr(self, 'page_' + page)(); self.nav()

    def next(self):
        if self.page == 'finish':
            self.work(self.finish, self.bg.get(), self.boot.get(), self.upd.get(), self.files.get()); return
        seq = self.seq(); self.show(seq[seq.index(self.page) + 1])

    def back(self):
        seq = self.seq(); self.show(seq[max(0, seq.index(self.page) - 1)])

    # ---- building blocks
    def h(self, text):
        tk.Label(self.body, text=text, bg=BG, fg=FG, font=(UIB, 20), anchor='w').pack(fill='x', pady=(0, 12))

    def p(self, text, muted=False, parent=None, bold=False, wrap=780):
        parent = parent or self.body
        tk.Label(parent, text=text, bg=parent['bg'], fg=MUTED if muted else FG, font=(UIB if bold else UI, 10), justify='left',
                 anchor='w', wraplength=wrap).pack(fill='x', pady=(0, 8))

    def items(self, lines, parent=None, wrap=740, numbered=False):
        parent = parent or self.body
        for i, l in enumerate(lines, 1):
            row = tk.Frame(parent, bg=parent['bg']); row.pack(fill='x', pady=(0, 6))
            tk.Label(row, text=f'{i}.' if numbered else '•', bg=parent['bg'], fg=ACC, font=(UIB, 10), width=3, anchor='ne').pack(side='left', anchor='n')
            tk.Label(row, text=l, bg=parent['bg'], fg=FG, font=(UI, 10), justify='left', anchor='w', wraplength=wrap).pack(side='left', fill='x')

    def buttons(self, *specs, parent=None):
        row = tk.Frame(parent or self.body, bg=BG); row.pack(fill='x', pady=(6, 10))
        for text, cmd, accent in specs:
            ttk.Button(row, text=text, style='Accent.TButton' if accent else 'TButton', command=cmd).pack(side='left', padx=(0, 8))
        return row

    def note(self, text='', state='unk'):
        if self.note_lbl is None or not self.note_lbl.winfo_exists():
            self.note_lbl = tk.Label(self.body, text='', bg=BG, font=(UIB, 10), justify='left', anchor='w', wraplength=780)
            self.note_lbl.pack(fill='x', pady=(4, 8))
        self.note_lbl.configure(text=text, fg=STATE[state] if state != 'unk' else MUTED)

    def log_box(self, height=9):
        c = card(self.body, fill='both', expand=True, pady=(6, 0))
        self.logw = tk.Text(c, height=height, state='disabled', wrap='word', font=('Consolas', 9), bg=CARD, fg=MUTED, relief='flat',
                            highlightthickness=0, padx=12, pady=8)
        self.logw.pack(fill='both', expand=True); self.logw.tag_configure('t', foreground=DIM); self.logw.tag_configure('err', foreground=STATE['bad'])

    def done_ok(self, gate, text):
        self.ok.add(gate); self.after(0, self.note, text, 'ok'); self.after(0, self.nav)

    # ---- pages
    def page_welcome(self):
        self.h('Welcome to Reality Hop')
        self.p('Reality Hop hides your VPN inside a disguised connection. Networks that block VPNs (school, work, hotel or dorm '
               'Wi-Fi, a router with "VPN blocking") see only ordinary HTTPS to a big website such as apple.com, so your VPN '
               'connects anyway.')
        self.p('It runs sing-box with VLESS + REALITY, takes free public nodes from the FastNodes list, tests them from this PC, '
               'keeps the 6 fastest that work and replaces dead ones on its own.')
        row = tk.Frame(self.body, bg=BG); row.pack(fill='x', pady=(0, 10))
        for label, text, url in (('Nodes:', 'github.com/rtwo2/FastNodes', 'https://github.com/rtwo2/FastNodes'),
                                 ('Tunnel engine:', 'github.com/SagerNet/sing-box', 'https://github.com/SagerNet/sing-box')):
            tk.Label(row, text=label, bg=BG, fg=MUTED, font=(UI, 9)).pack(side='left')
            link(row, text, url, BG).pack(side='left', padx=(4, 18))
        self.p('What you need', bold=True)
        self.items(['Windows 10 or 11, 64-bit.',
                    'A VPN app that can use a SOCKS5 proxy. Private Internet Access (PIA) works out of the box; the VPN step shows how.',
                    'Optional: a GL.iNet router (firmware 4.x) to put other devices, such as a phone or a console, through the tunnel too.'])
        self.p('Public nodes are run by strangers. Your VPN still encrypts everything end to end, so a node only sees encrypted VPN '
               'traffic. Always keep the VPN on top. Make sure you are allowed to use a VPN on the network you are on.', muted=True)

    def page_install(self):
        self.h('Install')
        self.p('This step:')
        self.items((['copies Reality Hop into its install folder and adds it to the Start menu (one admin prompt). Everything '
                     'it keeps stays in that folder (settings, nodes, logs, the tunnel engine); only your Windows account can '
                     'open it, and Reality Hop updates itself there without asking again'] if N.FROZEN else []) +
                   ['downloads sing-box from its official releases (github.com/SagerNet/sing-box, about 35 MB) and checks its SHA-256 checksum',
                    'downloads the public node list (github.com/rtwo2/FastNodes), tests the 40 fastest candidates from this PC and keeps '
                    'the 6 fastest that work (about a minute)',
                    'starts the tunnel on 127.0.0.1:2080'])
        self.folder = None
        if N.FROZEN and not os.path.isfile(N.INSTALLED_EXE):   # first install: the folder can be chosen
            row = tk.Frame(self.body, bg=BG); row.pack(fill='x', pady=(4, 6))
            tk.Label(row, text='Install folder', bg=BG, fg=FG, font=(UI, 10)).pack(side='left')
            self.folder = tk.StringVar(value=N.INSTALL_DIR)
            tk.Entry(row, textvariable=self.folder, width=52, bg=BTN, fg=FG, insertbackground=FG, relief='flat',
                     font=(UI, 10)).pack(side='left', padx=10, ipady=4)
            ttk.Button(row, text='Change...', command=self.pick_folder).pack(side='left')
        elif N.FROZEN:
            self.p(f'Installed in {N.INSTALL_DIR}.', muted=True)
        self.beta = tk.BooleanVar(value=N.settings_load()['singbox'] == 'testing')
        t = Toggle(self.body, 'Use sing-box test builds (beta, alpha) instead of stable releases. Either way it updates itself daily.',
                   self.beta)
        t.configure(bg=BG); t.c.configure(bg=BG); t.lab.configure(bg=BG, font=(UI, 10)); t.pack(anchor='w', pady=(6, 4))
        s = N.settings_load()
        self.menu, self.desk = tk.BooleanVar(value=bool(s['start_menu'])), tk.BooleanVar(value=bool(s['desktop_icon']))
        if N.FROZEN:
            for text, var in (('Show Reality Hop in the Start menu and Windows search (press the Windows key, type Reality Hop)', self.menu),
                              ('Desktop icon', self.desk)):
                t = Toggle(self.body, text, var)
                t.configure(bg=BG); t.c.configure(bg=BG); t.lab.configure(bg=BG, font=(UI, 10)); t.pack(anchor='w', pady=(2, 4))
        self.buttons(('Install' if 'install' not in self.ok else 'Install again (repair)',
                      lambda: self.work(self.do_install, self.beta.get(), self.menu.get(), self.desk.get(),
                                        self.folder.get().strip() if self.folder else None), True))
        if 'install' in self.ok:
            self.note('Already installed. Press Next, or Install again to repair.', 'ok')
        self.log_box()

    def pick_folder(self):
        f = filedialog.askdirectory(title='Install Reality Hop in this folder', parent=self,
                                    initialdir=os.path.dirname(self.folder.get()) or None)
        if f:
            f = os.path.normpath(f)
            self.folder.set(f if os.path.basename(f).lower() == N.APP.lower() else os.path.join(f, N.APP))

    def do_install(self, testing, start_menu=True, desktop=False, folder=None):
        channel = 'testing' if testing else 'stable'
        if folder is not None and not os.path.isabs(folder):
            raise RuntimeError('the install folder must be a full path, like C:\\Program Files\\RealityHop')
        N.settings_save({'singbox': channel, 'start_menu': int(start_menu), 'desktop_icon': int(desktop)})
        if N.FROZEN:
            install.install(folder)
        if os.path.exists(N.CORE) and N.this_pc().get('core_channel') != channel:
            N.core_update()   # running the setup again with the other channel
        res = N.setup_pc()
        bad = [l for l in res if 'NOT' in l or 'did not come up' in l]
        if bad:
            self.after(0, self.note, 'Installed with a problem: ' + ' '.join(bad), 'warn')
            if 'did not come up' not in ' '.join(bad):
                self.ok.add('install'); self.after(0, self.nav)
        else:
            self.done_ok('install', 'Installed. The tunnel is running. Press Next.')

    def page_router(self):
        self.h('Do you have a GL.iNet router?')
        self.p('With the router set up, devices you choose (a phone, a games console, a TV) also go through the tunnel, with '
               'nothing to install on them. Only their traffic is diverted; everything else on your network stays as it is.')
        self.p('Tested on a GL-AXT1800 with firmware 4.x and the router\'s sing-box 1.9. Other GL.iNet routers on firmware 4.x use '
               'the same system and should work the same way. You can turn this on or off later in Settings.', muted=True)
        self.p('Router only: nothing runs on this PC\'s connection. The router tunnels the devices you choose, or every device '
               'on its main network, this PC included. For people who want all their traffic to go through the tunnel.',
               muted=True)

        def pick(mode):   # 'both', 'router' (router only) or 'pc'
            self.use_router = mode != 'pc'
            N.settings_save({'pc_tunnel': int(mode != 'router')} if self.use_router else {'router': 0, 'pc_tunnel': 1})
            if mode == 'router':   # the install step started the PC tunnel; the router does the work from now on
                threading.Thread(target=N.core_stop, daemon=True).start()
            self.next_btn.state(['!disabled']); self.page = 'router'
            seq = self.seq(); self.show(seq[seq.index('router') + 1])
        self.buttons(('Yes, this PC and my router', lambda: pick('both'), True), ('Router only', lambda: pick('router'), False),
                     ('No, only this PC', lambda: pick('pc'), False))

    def page_ssh(self):
        self.h('Router step 1 of 3: connect')
        self.p('Reality Hop talks to the router over SSH with its own key, so it never needs or keeps your router password. '
               'SSH is on by default on GL.iNet routers.')
        row = tk.Frame(self.body, bg=BG); row.pack(fill='x', pady=(4, 10))
        tk.Label(row, text='Router address', bg=BG, fg=FG, font=(UI, 10)).pack(side='left')
        self.host = tk.StringVar(value=N.settings_load()['router_host'])
        tk.Entry(row, textvariable=self.host, width=18, bg=BTN, fg=FG, insertbackground=FG, relief='flat',
                 font=(UI, 10)).pack(side='left', padx=10, ipady=4)
        tk.Label(row, text='(GL.iNet default: 192.168.8.1)', bg=BG, fg=MUTED, font=(UI, 9)).pack(side='left')
        self.items(['Click "Add key to router". A black window opens and asks for the router password: type the admin password '
                    'of the GL.iNet web panel and press Enter (nothing shows while you type). When it says "Key added", close it.',
                    'Click "Test connection".'], numbered=True)
        self.buttons(('Add key to router', self.add_key, False), ('Test connection', lambda: self.work(self.test_ssh, self.host.get()), True),
                     ('Use my own key file...', self.own_key, False))
        if 'ssh' in self.ok:
            self.note('Connected.', 'ok')

    def save_host(self, h=None):
        h = (h if h is not None else self.host.get()).strip()
        if not re.fullmatch(r'[A-Za-z0-9.-]+', h):
            raise RuntimeError('the router address looks wrong; use something like 192.168.8.1')
        N.settings_save({'router_host': h})
        return h

    def add_key(self):
        try:
            host = self.save_host(); N.router_forget()   # this is the router now, also when it is a new one
            N.add_key_console(host); self.note('Type the router password in the black window, then click Test connection.')
        except Exception as e:
            self.note(f'Something failed: {e}', 'bad')

    def own_key(self):
        f = filedialog.askopenfilename(title='Private key that the router already accepts', parent=self)
        if f:
            N.settings_save({'router_key': f}); self.note(f'Using {os.path.basename(f)}. Click Test connection.')

    def test_ssh(self, host):
        self.save_host(host); N.router_forget()
        if N.ssh('echo up', timeout=30).strip() != 'up':
            raise RuntimeError('the router did not answer over SSH; add the key first, and check the address')
        i = N.router_info()
        self.done_ok('ssh', f"Connected: {i['model']}, firmware {i['firmware']}, sing-box {i['singbox'] or 'not installed'}. Press Next.")

    def page_box(self):
        self.h('Router step 2 of 3: sing-box on the router')
        self.p('The router runs its own copy of sing-box for the devices you choose. Reality Hop can install it from the router\'s '
               'package list, or you can do it yourself: GL.iNet web panel → APPLICATIONS → Plug-ins → Update, then search for '
               '"sing-box" and install it, and also "jq" and "curl" (used by the router\'s self-repair).')
        self.buttons(('Check again', lambda: self.work(self.check_box), False),
                     ('Install sing-box on the router', lambda: self.work(self.box_install), True))
        self.log_box(6); self.work(self.check_box)

    def check_box(self):
        i = N.router_info(); miss = [t for t in ('jq', 'curl') if t not in i['tools']]
        if not i['singbox'] or miss:
            self.after(0, self.note, f"{i['model']}: " + ('sing-box is not installed' if not i['singbox'] else f"sing-box {i['singbox']}")
                       + (f"; missing {', '.join(miss)}" if miss else '') + '. Click Install sing-box on the router.', 'warn')
        elif i['too_new']:
            self.ok.add('box'); self.after(0, self.nav)
            self.after(0, self.note, f"sing-box {i['singbox']} is installed. Reality Hop's router config is written for sing-box 1.9 to "
                       '1.12; this newer version may reject it. If Push to router fails, please open an issue on GitHub.', 'warn')
        else:
            self.done_ok('box', f"{i['model']}: sing-box {i['singbox']}, jq and curl are installed. Press Next.")

    def box_install(self):
        self.say('router: installing sing-box, jq and curl (a few minutes on a slow connection)...')
        self.say(N.router_install()); self.check_box()

    def page_devices(self):
        self.h('Router step 3 of 3: choose devices')
        self.p('Click the devices whose traffic should go through the tunnel. The list is the router\'s DHCP clients (devices seen '
               'recently).')
        self.all_dev = tk.BooleanVar(value=N.all_devices())
        t = Toggle(self.body, 'Every device on the main network (this PC too): all traffic goes through the tunnel', self.all_dev)
        t.configure(bg=BG); t.c.configure(bg=BG); t.lab.configure(bg=BG, font=(UI, 10)); t.pack(anchor='w', pady=(0, 8))
        c = card(self.body, fill='x', pady=(0, 8))
        self.dev = table(c, (('num', 40), ('ip', 150), ('name', 400)), height=7, stretch='name'); self.dev.pack(fill='x', padx=8, pady=6)
        self.dev.heading('ip', text='ADDRESS'); self.dev.heading('name', text='DEVICE'); self.dev.heading('num', text='')
        self.dev.bind('<Button-1>', self.toggle_dev)
        row = tk.Frame(self.body, bg=BG); row.pack(fill='x', pady=(2, 6))
        tk.Label(row, text='Other addresses', bg=BG, fg=FG, font=(UI, 10)).pack(side='left')
        known = N.devices(); self.picked |= set(known)
        self.extra = tk.StringVar(value='')
        tk.Entry(row, textvariable=self.extra, width=40, bg=BTN, fg=FG, insertbackground=FG, relief='flat', font=(UI, 10)).pack(side='left', padx=10, ipady=4)
        tk.Label(row, text='comma separated', bg=BG, fg=MUTED, font=(UI, 9)).pack(side='left')
        self.p('Give each chosen device a fixed address, or it can lose the tunnel when its address changes. On phones, first '
               'turn off rotating private addresses for this Wi-Fi (iPhone: Private Wi-Fi Address set to Off or Fixed; Android: '
               'use device MAC). Then GL.iNet web panel > CLIENTS: turn on the Reserved IP slider next to the device.', muted=True)
        self.buttons(('Push to router', lambda: self.work(self.push_devices, self.extra.get(), self.all_dev.get()), True))
        self.log_box(5); self.work(self.load_devices)

    def load_devices(self):
        rows = N.router_clients()
        shown = self.shown = {ip for ip, _ in rows}
        def fill():
            self.dev.delete(*self.dev.get_children())
            for i, (ip, name) in enumerate(rows):
                self.dev.insert('', 'end', iid=ip, values=('✓' if ip in self.picked else '', ip, name), tags=('odd' if i % 2 else 'even',))
            self.extra.set(', '.join(sorted(self.picked - shown)))
        self.after(0, fill)

    def toggle_dev(self, e):
        ip = self.dev.identify_row(e.y)
        if ip:
            self.picked ^= {ip}; self.dev.set(ip, 'num', '✓' if ip in self.picked else '')
        return 'break'

    def push_devices(self, extra, every=False):
        extra = [x for x in re.split(r'[\s,;]+', extra) if x]
        bad = [x for x in extra if not N.lan_ip(x)]
        if bad:
            raise RuntimeError(f"not an address: {', '.join(bad)}")
        ips = ['all'] if every else sorted(set(extra) | (self.picked & getattr(self, 'shown', set())))
        if not ips:
            raise RuntimeError('pick at least one device, or every device')
        N.settings_save({'devices': ', '.join(ips), 'router': 1})
        if N.router_only() and not N.nodes_of(N.ROUTER_CFG):   # no nodes yet: find them from the router's side
            self.say('router: finding the first nodes (tested on the router)...')
            nodes = N.scan(); N.apply(nodes, 6)
        who = 'every device on the main network' if every else ', '.join(ips)
        self.say(f"router: pushing the config for {who}...")
        out = N.push(); self.say(out)
        if 'sing-box service: running' not in out:
            raise RuntimeError('the router did not take the config; see the log above')
        self.done_ok('devices', f"The router tunnel is running for {who}. Press Next.")

    def page_vpn(self):
        if N.router_only():
            self.h('Your VPN app')
            self.p('In router-only mode your VPN app needs no proxy setting: the router puts the traffic of the chosen devices '
                   'through the tunnel. Connect the VPN app as usual. OpenVPN over TCP port 443 is the most reliable through the '
                   'tunnel.')
            self.p('If PIA on this PC still uses the SOCKS5 proxy 127.0.0.1:2080, turn it off: Settings → Multi-Hop, untick '
                   '"Multi-hop and Obfuscation".', muted=True)
            return
        self.h('Point your VPN app at the tunnel')
        self.p('Your VPN app connects through Reality Hop\'s SOCKS5 proxy: address 127.0.0.1, port 2080, no username or password. '
               'Use OpenVPN over TCP port 443: a SOCKS5 proxy carries TCP only, and 443 is the normal HTTPS port.')
        cols = tk.Frame(self.body, bg=BG); cols.pack(fill='both', expand=True)
        left = tk.Frame(cols, bg=BG, width=330); left.pack(side='left', fill='y', anchor='n'); left.pack_propagate(False)
        self.p('Private Internet Access (PIA)', bold=True, parent=left)
        self.items(['PIA → Settings → Protocols: Protocol OpenVPN, Transport TCP, Remote Port 443.',
                    'Settings → Multi-Hop: tick "Multi-hop and Obfuscation" and choose SOCKS5 Proxy.',
                    'Click EDIT: host 127.0.0.1, port 2080. Leave username and password empty. Save.',
                    'Connect.'], parent=left, wrap=280, numbered=True)
        row = tk.Frame(left, bg=BG); row.pack(fill='x', pady=(6, 4))
        ttk.Button(row, text='Check PIA', command=lambda: (lambda r: self.note(r, 'ok' if 'correct' in r else 'warn'))(N.vpn_check())).pack(side='left')
        self.p('Other VPN apps', bold=True, parent=left)
        self.p('Any VPN app with a SOCKS5 proxy setting works the same way; set it to OpenVPN over TCP, port 443. In an '
               'OpenVPN config file: "proto tcp", the server on port 443, and the line "socks-proxy 127.0.0.1 2080".',
               parent=left, wrap=310)
        self.p('Games or apps that should skip the VPN: use your VPN app\'s split tunnel (PIA: Settings → Split Tunnel).',
               muted=True, parent=left, wrap=310)
        try:
            self.img = tk.PhotoImage(file=os.path.join(N.RES, 'pia-multihop.png')).subsample(2)
            f = tk.Frame(cols, bg=LINE); f.pack(side='left', anchor='n', padx=(18, 0))
            tk.Label(f, image=self.img, bg=CARD, bd=0).pack(padx=1, pady=1)
        except tk.TclError:
            pass

    def page_finish(self):
        self.h('Almost done')
        s = N.settings_load()
        self.bg, self.boot, self.upd = tk.BooleanVar(value=True), tk.BooleanVar(value=True), tk.BooleanVar(value=bool(s['auto_update']))
        self.files = tk.BooleanVar(value=bool(s['backend_files']))
        for text, var in (('Run Reality Hop in the background at logon (tray icon; it repairs dead nodes on its own)', self.bg),
                          *((('Start the tunnel at boot, before the VPN app (asks for admin rights once)', self.boot),)
                            if not N.router_only() else ()),
                          ('Install Reality Hop updates automatically', self.upd),
                          ('Show backend files: buttons for the PC, Router and logs folders, to see what runs behind the scenes', self.files)):
            t = Toggle(self.body, text, var); t.configure(bg=BG); t.c.configure(bg=BG); t.lab.configure(bg=BG, font=(UI, 10))
            t.pack(anchor='w', pady=(4, 10))
        self.p('You can change all of this later in the Reality Hop window. Press Finish to open it.', muted=True)
        self.log_box(5)

    def finish(self, bg, boot, upd, files):
        N.settings_save({'auto_update': int(upd), 'backend_files': int(files)})
        N.startup(bool(bg))
        if N.router_only():
            if N.boot_task_exists():   # no PC tunnel to start at boot any more
                N.boot_task(False)
        elif boot and not N.boot_task_exists():
            N.boot_task(True)
        N.set_this_pc(setup_done=True, stopped=False)
        self.after(0, lambda: restart_window([]))


def uninstall_dialog():
    root = tk.Tk(); root.withdraw()
    if not messagebox.askyesno('Uninstall Reality Hop', 'Remove Reality Hop from this PC?\n\nThe tunnel, the background watcher, the boot '
                               'start (one UAC prompt), the shortcuts and the program are removed.'):
        return
    keep = messagebox.askyesno('Uninstall Reality Hop', 'Keep your settings and node vault for a later install?')
    router = (not N.pc_only()) and messagebox.askyesno('Uninstall Reality Hop', 'Also switch the tunnel on the router off?')
    out = install.uninstall(keep_data=keep, router_too=router)
    messagebox.showinfo('Uninstall Reality Hop', 'Reality Hop is removed.\n\n' + '\n'.join(out))


if __name__ == '__main__':
    if '--watch' in sys.argv:   # RealityHop.exe --watch = the background watcher
        N.watch(); sys.exit(0)
    if '--once' in sys.argv:
        where = N.known_where()
        for key, label, rel, cfg in LISTS:
            nodes = N.nodes_of(cfg); N.tcp_probe(nodes)
            if key == 'pc':
                st = N.pc_status()
                print(f"PC tunnel: service {'running' if st['service'] else 'stopped'}, 2080 {'open' if st['listening'] else 'closed'}, "
                      f"API {'up' if st['api'] else 'down'}, using {st['now'] or '?'}, exit {st['exit'] or '?'}")
                if st['api']:
                    N.check_pc(nodes)
            print(f'\n{label}  ({rel})')
            for n in nodes:
                tun = n.get('pc_ms') if n.get('pc_ms') is not None else (n.get('pc_why') or '-')
                print(f"  {n['tag']:8} {n['server']:16} {n['sni']:20} {where.get(n['server'], '?'):26} 443: {ms(n['tcp_ms'], 'no answer'):>10}   tunnel: {tun}")
        print('\nfiles newer than deployed:', N.pending_deploy())
        sys.exit(0)
    try:
        import ctypes; ctypes.windll.shcore.SetProcessDpiAwareness(1)
    except Exception:
        pass
    if '--uninstall' in sys.argv:
        uninstall_dialog(); sys.exit(0)
    install.cleanup()
    if N.is_setup() and (install.running_legacy() or (install.needs_install() and
                                                       (os.path.exists(N.INSTALLED_EXE) or os.path.exists(install.LEGACY_EXE)))):
        # set up already: this copy came from Downloads, or it is an old per-user install. Install it to Program Files.
        root = tk.Tk(); root.withdraw()
        q = (f'Reality Hop now installs to {N.INSTALL_DIR}. Move it there now? (one admin prompt)' if install.running_legacy() else
             f"Reality Hop {install.installed_version() or ''} is installed. Replace it with this copy ({N.VERSION}) and open it?")
        if messagebox.askyesno('Reality Hop', q + '\n\nYour settings, nodes and the running tunnel are kept.'):
            try:
                install.install()
            except Exception as e:
                messagebox.showerror('Reality Hop', str(e)); sys.exit(1)
            subprocess.Popen([N.INSTALLED_EXE], cwd=N.DATA, env=N.own_env(), creationflags=subprocess.DETACHED_PROCESS)
            sys.exit(0)
        if not install.running_legacy():
            sys.exit(0)
        root.destroy()   # keep running from the old place this time; asked again next start
    if N.is_setup() and install.needs_move():   # installed by 1.0.x: the data is still in %LOCALAPPDATA%
        root = tk.Tk(); root.withdraw()
        if messagebox.askyesno(N.NAME, f'Reality Hop now keeps everything in its program folder:\n{N.INSTALL_DIR}\n\nMove your '
                               f'settings, nodes, logs and the tunnel engine there from {N.OLD_DATA} now? The tunnel stops '
                               'for a few seconds. One admin prompt: the folder becomes yours alone, and the boot start '
                               'follows the move.'):
            try:
                install.move_here()
            except Exception as e:
                messagebox.showerror(N.NAME, f'Not moved: {e}\n\nEverything stays where it was; you are asked again next time.')
            else:
                restart_window()   # opens again from the new place
        root.destroy()
    if '--setup' in sys.argv or install.needs_install() or not N.is_setup():
        Wizard(router_only='--router' in sys.argv).mainloop(); sys.exit(0)
    for _ in range(6):   # a window that is restarting lets go of its lock within a moment
        if not N.port_taken(N.GUI_PORT):
            break
        time.sleep(0.5)
    else:   # already open (the Start menu or desktop icon clicked again): bring that one forward instead of a second copy
        import ctypes
        h = ctypes.windll.user32.FindWindowW('TkTopLevel', N.NAME)
        if h:
            ctypes.windll.user32.ShowWindowAsync(h, 9); ctypes.windll.user32.SetForegroundWindow(h)   # SW_RESTORE, no wait
        sys.exit(0)
    App().mainloop()
