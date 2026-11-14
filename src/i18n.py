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
    """The Windows display language, if Reality Hop has it, else English."""
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


def T(text, **kw):
    """text in the current language; {name} fields are filled from kw."""
    s = CAT.get(text) or text
    if kw:
        try:
            s = s.format(**kw)
        except (KeyError, IndexError, ValueError):   # a broken translation must not break the window
            s = text.format(**kw)
    if RTL and s:   # each line in a right-to-left embedding: Windows draws text left to right unless told
        s = '\n'.join(RLE + p + PDF if p.strip() else p for p in s.split('\n'))
    return s


RLE, PDF = '‫', '‬'
