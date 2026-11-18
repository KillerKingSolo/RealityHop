//! The languages Reality Hop speaks (the port of i18n.py). The English text is the key; resources/lang/<code>.json
//! maps it to the other language. A text without a translation shows in English. Logs and reports stay English.

use fancy_regex::{Captures, Regex};
use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// (code, name in its own language), in the order the language picker shows them.
pub const LANGS: [(&str, &str); 9] = [
    ("en", "English"),
    ("es", "Español"),
    ("zh", "简体中文"),
    ("ru", "Русский"),
    ("uk", "Українська"),
    ("fa", "فارسی"),
    ("ar", "العربية"),
    ("tr", "Türkçe"),
    ("vi", "Tiếng Việt"),
];
/// Written right to left.
pub const RTL_LANGS: [&str; 2] = ["fa", "ar"];
/// Windows primary language ids (GetUserDefaultUILanguage & 0x3ff) of the languages Reality Hop has.
pub const WIN_IDS: [(u16, &str); 8] =
    [(0x0a, "es"), (0x04, "zh"), (0x19, "ru"), (0x22, "uk"), (0x29, "fa"), (0x01, "ar"), (0x1f, "tr"), (0x2a, "vi")];
const RLE: char = '\u{202b}';
const PDF: char = '\u{202c}';

/// One language's texts.
#[derive(Debug, Clone, Default)]
pub struct Translations {
    pub lang: String,
    pub rtl: bool,
    catalog: HashMap<String, String>,
}

/// A replacement in mac_words: fixed text, or the Turkish "Başka/başka bir PC" that keeps its first letter.
enum Rep {
    Text(&'static str),
    KeepFirst(&'static str),
}

// The folder named PC keeps its name
const NOT_FOLDER: &str = r"(?!\s*[,،、]\s*Router)";

// On a Mac "PC" becomes "Mac" (Chinese, Persian and Arabic say "computer": no change); "another PC" becomes
// "another computer". Turkish endings follow the new word (PC'den -> Mac'ten).
static MAC_WORDS: LazyLock<HashMap<&'static str, Vec<(Regex, Rep)>>> = LazyLock::new(|| {
    let r = |p: &str| Regex::new(p).unwrap();
    let pc = || r(&format!(r"\bPC\b{NOT_FOLDER}"));
    HashMap::from([
        ("en", vec![(r(r"\banother PC\b"), Rep::Text("another computer")), (pc(), Rep::Text("Mac"))]),
        ("es", vec![(r(r"\botro PC\b"), Rep::Text("otro equipo")), (pc(), Rep::Text("Mac"))]),
        ("ru", vec![(r(r"\bдругой ПК\b"), Rep::Text("другой компьютер")), (r(r"\bПК\b"), Rep::Text("Mac"))]),
        ("uk", vec![(r(r"\bінший ПК\b"), Rep::Text("інший комп'ютер")), (r(r"\bПК\b"), Rep::Text("Mac"))]),
        (
            "tr",
            vec![
                (r(r"\b[Bb]aşka bir PC\b"), Rep::KeepFirst("aşka bir bilgisayar")),
                (r(r"\bPC'deki\b"), Rep::Text("Mac'teki")),
                (r(r"\bPC'de\b"), Rep::Text("Mac'te")),
                (r(r"\bPC'den\b"), Rep::Text("Mac'ten")),
                (r(r"\bPC'ye\b"), Rep::Text("Mac'e")),
                (r(r"\bPC'yi\b"), Rep::Text("Mac'i")),
                (r(r"\bPC'nin\b"), Rep::Text("Mac'in")),
                (pc(), Rep::Text("Mac")),
            ],
        ),
        ("vi", vec![(r(r"\bmáy PC khác\b"), Rep::Text("máy tính khác")), (pc(), Rep::Text("Mac"))]),
    ])
});

// "PC" as a Mac user reads it, in one language (i18n.mac_words)
pub fn mac_words(s: &str, lang: &str) -> String {
    let mut s = s.to_string();
    for (pat, rep) in MAC_WORDS.get(lang).into_iter().flatten() {
        let mut out = String::new();
        let mut last = 0;
        for c in pat.captures_iter(s.as_str()).flatten() {
            let m = c.get(0).unwrap();
            out.push_str(&s[last..m.start()]);
            out.push_str(&replacement(&c, rep));
            last = m.end();
        }
        out.push_str(&s[last..]);
        s = out;
    }
    s
}

// The text one match is replaced with
fn replacement(c: &Captures<'_, str>, rep: &Rep) -> String {
    match rep {
        Rep::Text(t) => t.to_string(),
        Rep::KeepFirst(t) => format!("{}{t}", c[0].chars().next().unwrap_or_default()),
    }
}

// str.format with keyword fields only: {name} filled from kw, {{ and }} as braces; None when Python would raise
pub fn format(s: &str, kw: &[(&str, String)]) -> Option<String> {
    let (mut out, mut it) = (String::new(), s.chars().peekable());
    while let Some(c) = it.next() {
        match c {
            '{' if it.peek() == Some(&'{') => {
                it.next();
                out.push('{');
            }
            '}' if it.peek() == Some(&'}') => {
                it.next();
                out.push('}');
            }
            '}' => return None,
            '{' => {
                let mut name = String::new();
                loop {
                    match it.next()? {
                        '}' => break,
                        '{' => return None,
                        ch => name.push(ch),
                    }
                }
                out.push_str(&kw.iter().find(|(k, _)| *k == name)?.1);
            }
            _ => out.push(c),
        }
    }
    Some(out)
}

impl Translations {
    // The texts of a language from its catalog file's JSON ("" or unknown codes: English)
    pub fn new(lang: &str, catalog_json: Option<&str>) -> Translations {
        let catalog = if lang == "en" { None } else { catalog_json.and_then(|j| serde_json::from_str(j).ok()) };
        Translations { lang: lang.into(), rtl: RTL_LANGS.contains(&lang), catalog: catalog.unwrap_or_default() }
    }

    // text in this language, {name} fields filled from kw; on a Mac "PC" reads "Mac" (i18n.T)
    pub fn t(&self, text: &str, kw: &[(&str, String)], mac: bool) -> String {
        let found = self.catalog.get(text).filter(|s| !s.is_empty());
        let mut s = found.cloned().unwrap_or_else(|| text.to_string());
        if mac {
            s = mac_words(&s, if found.is_some() { &self.lang } else { "en" });
        }
        if !kw.is_empty() {
            s = format(&s, kw).or_else(|| format(text, kw)).unwrap_or_else(|| text.to_string());
        }
        if self.rtl && !s.is_empty() {
            s =
                s.split('\n')
                    .map(|p| {
                        if crate::python_compat::strip(p).is_empty() { p.to_string() } else { format!("{RLE}{p}{PDF}") }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
        }
        s
    }
}

// The catalog of a language other than English (resources/lang/<code>.json), built into the program
pub fn catalog(code: &str) -> Option<&'static str> {
    Some(match code {
        "es" => include_str!("../../../src/resources/lang/es.json"),
        "zh" => include_str!("../../../src/resources/lang/zh.json"),
        "ru" => include_str!("../../../src/resources/lang/ru.json"),
        "uk" => include_str!("../../../src/resources/lang/uk.json"),
        "fa" => include_str!("../../../src/resources/lang/fa.json"),
        "ar" => include_str!("../../../src/resources/lang/ar.json"),
        "tr" => include_str!("../../../src/resources/lang/tr.json"),
        "vi" => include_str!("../../../src/resources/lang/vi.json"),
        _ => return None,
    })
}

// The Windows display language (macOS: the first preferred language) when Reality Hop speaks it, else English
// (i18n.system_lang)
pub fn system_lang() -> &'static str {
    #[cfg(windows)]
    {
        let id = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() } & 0x3ff;
        WIN_IDS.iter().find(|(k, _)| *k == id).map_or("en", |(_, c)| c)
    }
    #[cfg(not(windows))]
    {
        let _ = WIN_IDS;
        let out = crate::process::run(
            &["/usr/bin/defaults", "read", "-g", "AppleLanguages"],
            None,
            std::time::Duration::from_secs(10),
            None,
        )
        .map(|o| o.stdout)
        .unwrap_or_default();
        let first = crate::python_compat::strip(&out).trim_matches(['(', ')']).split(',').next().unwrap_or_default();
        let code: String = first.trim().trim_matches('"').chars().take(2).collect::<String>().to_lowercase();
        LANGS.iter().map(|(c, _)| *c).find(|c| *c == code).unwrap_or("en")
    }
}

static CURRENT: RwLock<Option<Translations>> = RwLock::new(None);

// Switch to a language; "" or an unknown code: the system's display language (i18n.use)
pub fn use_lang(code: &str) {
    let code = LANGS.iter().map(|(c, _)| *c).find(|c| *c == code).unwrap_or_else(system_lang);
    if CURRENT.read().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|t| t.lang == code) {
        return;
    }
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = Some(Translations::new(code, catalog(code)));
}

// text in the current language, {name} fields filled from kw; English until use_lang (i18n.T)
pub fn tr(text: &str, kw: &[(&str, String)]) -> String {
    let mac = !cfg!(windows);
    match CURRENT.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        Some(t) => t.t(text, kw, mac),
        None => Translations::new("en", None).t(text, kw, mac),
    }
}
