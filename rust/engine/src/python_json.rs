//! JSON text exactly as Python's json.dump writes it (ensure_ascii, its separators and indent), and Python's str() of
//! a JSON value. The data files, the configs and the router's md5 checks depend on the bytes staying the same.

use serde_json::Value;
use std::fmt::Write;

// json.dumps(v, indent=indent)
pub fn dumps(v: &Value, indent: Option<usize>) -> String {
    let mut out = String::new();
    write(&mut out, v, indent, 0);
    out
}

// One value at nesting level lvl
fn write(out: &mut String, v: &Value, indent: Option<usize>, lvl: usize) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&number(n)),
        Value::String(s) => string(out, s),
        Value::Array(a) if a.is_empty() => out.push_str("[]"),
        Value::Object(m) if m.is_empty() => out.push_str("{}"),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                sep(out, indent, lvl + 1, i == 0);
                write(out, x, indent, lvl + 1);
            }
            close(out, indent, lvl, ']');
        }
        Value::Object(m) => {
            out.push('{');
            for (i, (k, x)) in m.iter().enumerate() {
                sep(out, indent, lvl + 1, i == 0);
                string(out, k);
                out.push_str(": ");
                write(out, x, indent, lvl + 1);
            }
            close(out, indent, lvl, '}');
        }
    }
}

// The separator before an item: ", " flat, ",\n" plus indent with indent
fn sep(out: &mut String, indent: Option<usize>, lvl: usize, first: bool) {
    match indent {
        None if !first => out.push_str(", "),
        None => {}
        Some(n) => {
            if !first {
                out.push(',');
            }
            out.push('\n');
            out.push_str(&" ".repeat(n * lvl));
        }
    }
}

// The closing bracket, on its own line with indent
fn close(out: &mut String, indent: Option<usize>, lvl: usize, c: char) {
    if let Some(n) = indent {
        out.push('\n');
        out.push_str(&" ".repeat(n * lvl));
    }
    out.push(c);
}

// A string with ensure_ascii escaping: everything outside ' '..'~' as \uXXXX (surrogate pairs above U+FFFF)
fn string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            ' '..='~' => out.push(c),
            _ => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    let _ = write!(out, "\\u{u:04x}");
                }
            }
        }
    }
    out.push('"');
}

// A number as Python prints it: ints as is, floats as repr()
fn number(n: &serde_json::Number) -> String {
    match n.as_f64() {
        Some(f) if n.is_f64() => float_repr(f),
        _ => n.to_string(),
    }
}

// Python's repr(float): shortest round-trip digits, fixed notation for exponents -4..15, else 1e+16 style
pub fn float_repr(f: f64) -> String {
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0" } else { "0.0" }.to_string();
    }
    let sci = format!("{:e}", f.abs());
    let (mant, exp) = sci.split_once('e').unwrap();
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let e: i32 = exp.parse().unwrap();
    let sign = if f < 0.0 { "-" } else { "" };
    let body = if (-4..16).contains(&e) {
        if e >= 0 {
            let int_len = e as usize + 1;
            if digits.len() <= int_len {
                format!("{digits}{}.0", "0".repeat(int_len - digits.len()))
            } else {
                format!("{}.{}", &digits[..int_len], &digits[int_len..])
            }
        } else {
            format!("0.{}{digits}", "0".repeat((-e - 1) as usize))
        }
    } else {
        let frac = if digits.len() > 1 { format!(".{}", &digits[1..]) } else { String::new() };
        format!("{}{frac}e{}{:02}", &digits[..1], if e < 0 { '-' } else { '+' }, e.abs())
    };
    format!("{sign}{body}")
}

// Python's str() of a value json.load returned (None, True, 1.5, [1, 'a'], {'a': 1})
pub fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        _ => py_repr(v),
    }
}

// Python's repr() of a value json.load returned
pub fn py_repr(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(b) => if *b { "True" } else { "False" }.into(),
        Value::Number(n) => number(n),
        Value::String(s) => str_repr(s),
        Value::Array(a) => format!("[{}]", a.iter().map(py_repr).collect::<Vec<_>>().join(", ")),
        Value::Object(m) => {
            format!(
                "{{{}}}",
                m.iter().map(|(k, x)| format!("{}: {}", str_repr(k), py_repr(x))).collect::<Vec<_>>().join(", ")
            )
        }
    }
}

// repr() of a str: single quotes unless it holds ' and no "
fn str_repr(s: &str) -> String {
    let q = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::from(q);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == q => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push(q);
    out
}
