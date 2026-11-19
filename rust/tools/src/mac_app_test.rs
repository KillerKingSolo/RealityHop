//! mac-app-test: checks a built RealityHop-mac-<chip>.dmg on a real Mac (the release workflow runs it on several
//! macOS versions). Usage: rh-tools mac-app-test <dmg> <chip> <screenshot folder>
//! Everything runs with HOME set to fresh folders under the temp folder: nothing touches the real home. The update
//! swap from the dmg and the router key script are tested in rust/engine/tests/mac_release.rs, in the build job.

#[cfg(not(unix))]
pub fn main(_: &[String]) -> Result<(), String> {
    Err("mac-app-test runs on a Mac".into())
}

#[cfg(unix)]
pub use mac::main;

#[cfg(unix)]
mod mac {
    use crate::{TempDir, sha256_hex};
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::Duration;

    const CORE_ID: &str = "com.realityhop.app.core"; // rh-core's signing id (macos_system::CORE_ID)

    struct T {
        chip: String,
        shots: PathBuf,
        tmp: PathBuf,
        fails: Vec<String>,
    }

    // stdout and stderr of a command; empty when it could not start
    fn sh(args: &[&str]) -> (bool, String, String) {
        match Command::new(args[0]).args(&args[1..]).output() {
            Ok(o) => (
                o.status.success(),
                String::from_utf8_lossy(&o.stdout).into_owned(),
                String::from_utf8_lossy(&o.stderr).into_owned(),
            ),
            Err(_) => (false, String::new(), String::new()),
        }
    }

    fn app_exe(app: &Path) -> PathBuf {
        app.join("Contents").join("MacOS").join("RealityHop")
    }

    fn s(p: &Path) -> &str {
        p.to_str().unwrap_or_default()
    }

    // One value from an app's Info.plist
    fn plist_value(app: &Path, key: &str) -> String {
        let info = app.join("Contents").join("Info.plist");
        sh(&["/usr/bin/plutil", "-extract", key, "raw", "-o", "-", s(&info)]).1.trim().to_string()
    }

    impl T {
        fn check(&mut self, what: &str, ok: bool, detail: &str) {
            let d = if detail.is_empty() { String::new() } else { format!(": {detail}") };
            println!("{}  {what}{d}", if ok { "PASS" } else { "FAIL" });
            if !ok {
                self.fails.push(what.into());
            }
        }

        // A fresh HOME; with lang, a settings.json (dns "": the France question is not answered yet)
        fn home(&self, name: &str, lang: Option<&str>, dns: &str) -> PathBuf {
            let h = self.tmp.join(name);
            let _ = std::fs::create_dir_all(&h);
            if let Some(lang) = lang {
                let d = h.join("Library").join("Application Support").join("RealityHop");
                let _ = std::fs::create_dir_all(&d);
                let _ = std::fs::write(d.join("settings.json"), format!(r#"{{"lang": "{lang}", "dns": "{dns}"}}"#));
            }
            h
        }

        // Start the app with HOME=h, take a screenshot after wait seconds; true when it still runs then and did not
        // panic. Its own process group: what it starts (an osascript alert) holds the stderr pipe and goes with it
        fn launch(&self, exe: &Path, h: &Path, shot: &str, args: &[&str], wait: u64) -> bool {
            let child = Command::new(exe)
                .args(args)
                .env("HOME", h)
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .process_group(0)
                .spawn();
            let Ok(mut child) = child else { return false };
            let mut err_pipe = child.stderr.take();
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let mut b = Vec::new();
                if let Some(p) = err_pipe.as_mut() {
                    let _ = p.read_to_end(&mut b);
                }
                let _ = tx.send(String::from_utf8_lossy(&b).into_owned());
            });
            std::thread::sleep(Duration::from_secs(wait));
            let png = self.shots.join(format!("{}-{shot}.png", self.chip));
            sh(&["/usr/sbin/screencapture", "-x", s(&png)]);
            let alive = matches!(child.try_wait(), Ok(None));
            unsafe {
                libc::killpg(child.id() as libc::pid_t, libc::SIGKILL);
            }
            let _ = child.wait();
            let err = rx.recv_timeout(Duration::from_secs(30)).unwrap_or_default();
            let panicked = err.contains("panicked");
            if panicked {
                let tail: String = err.chars().rev().take(2000).collect::<Vec<_>>().into_iter().rev().collect();
                println!("{tail}");
            }
            alive && !panicked
        }
    }

    // Mount like Finder does (under /Volumes), read-only; the mount point
    fn mount(dmg: &Path) -> Option<String> {
        let (_, out, _) =
            sh(&["/usr/bin/hdiutil", "attach", "-nobrowse", "-readonly", "-noautoopen", "-plist", s(dmg)]);
        let re = regex::Regex::new(r"<key>mount-point</key>\s*<string>([^<]+)</string>").unwrap();
        re.captures(&out).map(|c| c[1].to_string())
    }

    pub fn main(a: &[String]) -> Result<(), String> {
        let [dmg, chip, shots] = a else { return Err("usage: rh-tools mac-app-test <dmg> <chip> <shots>".into()) };
        let arch = match chip.as_str() {
            "apple-silicon" => "arm64",
            "intel" => "x86_64",
            _ => return Err(format!("unknown chip {chip}")),
        };
        let dmg = std::path::absolute(dmg).map_err(|e| e.to_string())?;
        let shots = std::path::absolute(shots).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&shots).map_err(|e| e.to_string())?;
        let tmp = TempDir::new("rh-app-test")?;
        let mut t = T { chip: chip.clone(), shots, tmp: tmp.0.clone(), fails: Vec::new() };

        // the disk image
        let bytes = std::fs::read(&dmg).map_err(|e| format!("{}: {e}", dmg.display()))?;
        t.check("dmg checksum readable", sha256_hex(&bytes).len() == 64, "");
        let vol = mount(&dmg).ok_or("the dmg did not mount")?;
        let volp = PathBuf::from(&vol);
        t.check("dmg mounts under /Volumes", vol.starts_with("/Volumes/"), &vol);
        let src = volp.join("RealityHop.app");
        t.check("dmg holds RealityHop.app", app_exe(&src).is_file(), "");
        let link = std::fs::read_link(volp.join("Applications"));
        t.check("dmg holds the Applications link", link.is_ok_and(|l| l == Path::new("/Applications")), "");
        let icon = std::fs::metadata(volp.join(".VolumeIcon.icns")).map_or(0, |m| m.len());
        let finder = sh(&["/usr/bin/xattr", "-px", "com.apple.FinderInfo", &vol]).1;
        t.check("disk icon", icon > 1000 && finder.split_whitespace().nth(8) == Some("04"), "");
        let exe = app_exe(&src);
        let archs = sh(&["/usr/bin/lipo", "-archs", s(&exe)]).1;
        t.check("chip", archs.split_whitespace().collect::<Vec<_>>() == [arch], archs.trim());
        t.check("signature", sh(&["/usr/bin/codesign", "--verify", "--deep", "--strict", s(&src)]).0, "");
        let id = plist_value(&src, "CFBundleIdentifier");
        t.check("bundle id", id == "com.realityhop.app", &id);
        let min = plist_value(&src, "LSMinimumSystemVersion");
        t.check("minimum macOS 15", min == "15.0", &min);
        let v = plist_value(&src, "CFBundleShortVersionString");
        t.check("version", !v.is_empty(), &v);
        println!(
            "macOS {}, {}",
            sh(&["/usr/bin/sw_vers", "-productVersion"]).1.trim(),
            sh(&["/usr/bin/uname", "-m"]).1.trim()
        );

        // run straight from the disk image: it must ask to be moved to Applications, and write nothing
        let h = t.home("from-dmg", None, "");
        let ok = t.launch(&exe, &h, "from-dmg", &[], 8);
        t.check("from the dmg: a message, still open", ok, "");
        t.check("from the dmg: no login item, no data folder", !h.join("Library").exists(), "");

        let apps = t.tmp.join("Applications");
        let _ = std::fs::create_dir_all(&apps);
        let app = apps.join("RealityHop.app");
        t.check("copy like a drag to Applications", sh(&["/usr/bin/ditto", s(&src), s(&app)]).0, "");
        sh(&["/usr/bin/hdiutil", "detach", &vol, "-force"]);
        let exe = app_exe(&app);

        let h = t.home("first", None, "");
        let ok = t.launch(&exe, &h, "language", &[], 10);
        t.check("first start: language picker", ok, "");
        let h = t.home("france", Some("en"), "");
        let ok = t.launch(&exe, &h, "france", &[], 10);
        t.check("first setup: France question", ok, "");
        // with an rh-core signed by someone else, as sing-box from GitHub is ("a.out")
        let h = t.home("wizard", Some("en"), "quad9");
        let core = h.join("Library").join("Application Support").join("RealityHop").join("PC").join("rh-core");
        let _ = std::fs::create_dir_all(core.parent().unwrap_or(&h));
        let _ = std::fs::copy("/bin/echo", &core);
        let ok = t.launch(&exe, &h, "wizard", &[], 10);
        t.check("setup wizard", ok, "");
        let shown = sh(&["/usr/bin/codesign", "-dv", s(&core)]).2;
        let ident: Vec<&str> = shown.lines().filter(|l| l.starts_with("Identifier=")).collect();
        let want = format!("Identifier={CORE_ID}");
        t.check(
            "the app signs rh-core with Reality Hop's id at start",
            ident == [want.as_str()],
            &format!("{ident:?}"),
        );
        let h = t.home("wizard-fa", Some("fa"), "quad9");
        let ok = t.launch(&exe, &h, "wizard-fa", &[], 10);
        t.check("setup wizard in Persian (right to left)", ok, "");
        let h = t.home("watcher", Some("en"), "quad9");
        let ok = t.launch(&exe, &h, "watcher", &["--watch"], 10);
        t.check("watcher with menu bar icon", ok, "");

        drop(tmp);
        if t.fails.is_empty() {
            println!("all passed");
            Ok(())
        } else {
            Err(format!("{} failed: {}", t.fails.len(), t.fails.join(", ")))
        }
    }
}
