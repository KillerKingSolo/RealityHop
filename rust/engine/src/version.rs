//! Version strings: sing-box release order, the app's own versions and release file names.

// Leading ASCII digits of s as a number, and the rest
fn digits(s: &str) -> Option<(u64, &str)> {
    let n = s.bytes().take_while(u8::is_ascii_digit).count();
    (n > 0).then(|| (s[..n].parse().unwrap_or(u64::MAX), &s[n..]))
}

// Sort key for sing-box versions: 1.13.0-alpha.2 < 1.13.0-beta.1 < 1.13.0-rc.1 < 1.13.0; [0] when it is no version
pub fn vkey(tag: &str) -> Vec<u64> {
    let s = tag.strip_prefix('v').unwrap_or(tag);
    let parse = || -> Option<Vec<u64>> {
        let (a, s) = digits(s)?;
        let (b, s) = digits(s.strip_prefix('.')?)?;
        let (c, s) = digits(s.strip_prefix('.')?)?;
        let mut k = vec![a, b, c, 3, 0];
        if let Some(rest) = s.strip_prefix('-') {
            for (word, rank) in [("alpha", 0), ("beta", 1), ("rc", 2)] {
                if let Some(r) = rest.strip_prefix(word) {
                    k[3] = rank;
                    k[4] = digits(r.strip_prefix('.').unwrap_or(r)).map_or(0, |(n, _)| n);
                    break;
                }
            }
        }
        Some(k)
    };
    parse().unwrap_or_else(|| vec![0])
}

// The first three numbers in a version string (install.vtuple)
pub fn vtuple(v: &str) -> Vec<u64> {
    let (mut out, mut s) = (Vec::new(), v);
    while out.len() < 3 && !s.is_empty() {
        match digits(s) {
            Some((n, rest)) => {
                out.push(n);
                s = rest;
            }
            None => s = &s[s.chars().next().map_or(1, char::len_utf8)..],
        }
    }
    out
}

// The sing-box release file for this PC: windows-amd64.zip, or darwin-<arch>.tar.gz on a Mac (arch: arm64 or amd64)
pub fn core_asset(tag: &str, mac_arch: Option<&str>) -> String {
    let v = tag.trim_start_matches('v');
    match mac_arch {
        Some(a) => format!("sing-box-{v}-darwin-{a}.tar.gz"),
        None => format!("sing-box-{v}-windows-amd64.zip"),
    }
}
