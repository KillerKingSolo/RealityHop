//! Quoting for the scripts the engine builds.

// s as a PowerShell single-quoted string. Without this a ' in a path (a user called O'Brien, a renamed download)
// ends the string early. PowerShell takes the four typographic single quotes for a ' as well, so each is doubled too.
pub fn psq(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        out.push(c);
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}') {
            out.push(c);
        }
    }
    out.push('\'');
    out
}
