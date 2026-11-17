"""i18n.py  -  the languages Reality Hop speaks. The English text is the key; resources/lang/<code>.json maps it to the
other language. A text without a translation shows in English. The activity log and problem reports stay in English."""
import json, os, sys

LANGS = (('en', 'English'), ('es', 'Español'), ('zh', '简体中文'), ('ru', 'Русский'), ('uk', 'Українська'),
         ('fa', 'فارسی'), ('ar', 'العربية'), ('tr', 'Türkçe'), ('vi', 'Tiếng Việt'))
CODES = {c for c, _ in LANGS}
RTL_LANGS = {'fa', 'ar'}   # written right to left
WIN_IDS = {0x0a: 'es', 0x04: 'zh', 0x19: 'ru', 0x22: 'uk', 0x29: 'fa', 0x01: 'ar', 0x1f: 'tr', 0x2a: 'vi'}   # primary language ids
DIR = os.path.join(getattr(sys, '_MEIPASS', os.path.dirname(os.path.abspath(__file__))), 'resources', 'lang')
LANG, RTL, CAT = 'en', False, {}


def system_lang():
    """The Windows display language (macOS: the first preferred language), if Reality Hop has it, else English."""
    if sys.platform == 'darwin':
        try:
            import subprocess
            out = subprocess.run(['/usr/bin/defaults', 'read', '-g', 'AppleLanguages'], capture_output=True, text=True,
                                 timeout=10).stdout
            code = out.strip().strip('()').split(',')[0].strip().strip('"')[:2].lower()   # ( "en-US", es ) -> en
            return code if code in CODES else 'en'
        except Exception:
            return 'en'
    try:
        import ctypes
        return WIN_IDS.get(ctypes.windll.kernel32.GetUserDefaultUILanguage() & 0x3ff, 'en')
    except Exception:
        return 'en'


def use(code):
    """Switch to a language ('' = the Windows display language)."""
    global LANG, RTL, CAT
    code = code if code in CODES else system_lang()
    if code == LANG and (CAT or code == 'en'):
        return
    cat = {}
    if code != 'en':
        try:
            with open(os.path.join(DIR, code + '.json'), encoding='utf-8') as f:
                cat = json.load(f)
        except (OSError, ValueError):
            pass
    LANG, RTL, CAT = code, code in RTL_LANGS, cat


MAC = sys.platform == 'darwin'
_NOT_FOLDER = r'(?!\s*[,،、]\s*Router)'   # the folder named PC keeps its name
# On a Mac, "PC" becomes "Mac" (Chinese, Persian and Arabic say "computer": no change). Another PC on the router can be
# any computer, so that one becomes "computer". Turkish endings follow the new word (PC'den -> Mac'ten).
MAC_WORDS = {
    'en': [(r'\banother PC\b', 'another computer'), (r'\bPC\b' + _NOT_FOLDER, 'Mac')],
    'es': [(r'\botro PC\b', 'otro equipo'), (r'\bPC\b' + _NOT_FOLDER, 'Mac')],
    'ru': [(r'\bдругой ПК\b', 'другой компьютер'), (r'\bПК\b', 'Mac')],
    'uk': [(r'\bінший ПК\b', "інший комп'ютер"), (r'\bПК\b', 'Mac')],
    'tr': [(r'\b[Bb]aşka bir PC\b', lambda m: m.group(0)[0] + 'aşka bir bilgisayar'), (r"\bPC'deki\b", "Mac'teki"), (r"\bPC'de\b", "Mac'te"),
           (r"\bPC'den\b", "Mac'ten"), (r"\bPC'ye\b", "Mac'e"), (r"\bPC'yi\b", "Mac'i"), (r"\bPC'nin\b", "Mac'in"),
           (r'\bPC\b' + _NOT_FOLDER, 'Mac')],
    'vi': [(r'\bmáy PC khác\b', 'máy tính khác'), (r'\bPC\b' + _NOT_FOLDER, 'Mac')],
}


def mac_words(s, lang):
    import re
    for pat, rep in MAC_WORDS.get(lang, ()):
        s = re.sub(pat, rep, s)
    return s


def T(text, **kw):
    """text in the current language; {name} fields are filled from kw. On a Mac, "PC" reads "Mac" (mac_words)."""
    s = CAT.get(text) or text
    if MAC:   # before the fields go in: a node name or path must not change
        s = mac_words(s, LANG if text in CAT else 'en')
    if kw:
        try:
            s = s.format(**kw)
        except (KeyError, IndexError, ValueError):   # a broken translation must not break the window
            s = text.format(**kw)
    if RTL and s:   # each line in a right-to-left embedding: Windows draws text left to right unless told
        s = '\n'.join(RLE + p + PDF if p.strip() else p for p in s.split('\n'))
    return s


RLE, PDF = '‫', '‬'
