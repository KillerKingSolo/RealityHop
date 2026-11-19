//! The questions asked once when the window opens: the boot task's CPU priority, a copy to install over the installed
//! one or an old per-user install, and data a 1.0.x install left in %LOCALAPPDATA% (realityhop.pyw start-up).

use crate::jobs::Ctx;
use engine::app_install;
use engine::paths::Paths;
use engine::translations::tr;
use serde_json::{Map, Value, json};
use tauri::{AppHandle, Manager};

const KEPT: &str = "Your settings, nodes and the running tunnel are kept.";

// The questions due now, in order: [{key, text}]. The boot question is asked only once
fn asks(paths: &Paths) -> Vec<Value> {
    let mut out = Vec::new();
    if !engine::setup_steps::is_setup(paths) {
        return out;
    }
    let asked = engine::settings::this_pc(paths).get("boot_priority_asked").is_some_and(engine::settings::py_truthy);
    if !asked {
        let _ = engine::settings::set_this_pc(paths, Map::from_iter([("boot_priority_asked".into(), json!(true))]));
        if !matches!(engine::startup_tasks::boot_task_priority(), None | Some(5)) {
            let q = "The PC tunnel starts at boot with below-normal CPU priority, so it waits behind other programs when the PC is busy. Give it normal priority? One admin prompt; it applies from the next tunnel restart.";
            out.push(json!({"key": "boot", "text": tr(q, &[])}));
        }
    }
    if cfg!(debug_assertions) {
        return out; // a development build never installs or moves anything
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = app_install::install_dir().to_string_lossy().into_owned();
    let other =
        app_install::installed_exe(&exe).exists() || app_install::legacy_dir().join(app_install::EXE_NAME).exists();
    if app_install::running_legacy(&exe) {
        let q = tr("Reality Hop now installs to {dir}. Move it there now? (one admin prompt)", &[("dir", dir.clone())]);
        out.push(json!({"key": "legacy", "text": format!("{q}\n\n{}", tr(KEPT, &[]))}));
    } else if app_install::needs_install(&exe) && other {
        let q = tr(
            "Reality Hop {old} is installed. Replace it with this copy ({ver}) and open it?",
            &[("old", app_install::installed_version().unwrap_or_default()), ("ver", engine::VERSION.into())],
        );
        out.push(json!({"key": "replace", "text": format!("{q}\n\n{}", tr(KEPT, &[]))}));
    } else if app_install::needs_move(paths, &exe) {
        let q = "Reality Hop now keeps everything in its program folder:\n{dir}\n\nMove your settings, nodes, logs and the tunnel engine there from {old} now? The tunnel stops for a few seconds. One admin prompt: the folder becomes yours alone, and the boot start follows the move.";
        let old = app_install::old_data().to_string_lossy().into_owned();
        out.push(json!({"key": "move", "text": tr(q, &[("dir", dir), ("old", old)])}));
    }
    out
}

// The questions to ask now
#[tauri::command]
pub async fn start_asks(app: AppHandle) -> Result<Vec<Value>, String> {
    tauri::async_runtime::spawn_blocking(move || asks(&app.state::<Ctx>().paths)).await.map_err(|e| e.to_string())
}

// Open the installed copy once this process has ended
fn relaunch(app: &AppHandle, exe: &std::path::Path) -> Result<(), String> {
    std::process::Command::new(exe).arg("--after-update").spawn().map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}

// Act on an answer; a line to show, if any. A "No" to the replace question ends this copy; so does a failed install,
// once its error is shown
#[tauri::command]
pub async fn start_answer(app: AppHandle, key: String, yes: bool) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = app.state::<Ctx>();
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        match (key.as_str(), yes) {
            ("boot", true) => Ok(Some(if engine::startup_tasks::boot_task_priority_fix(&ctx.paths) {
                tr("PC tunnel: normal priority from the next restart", &[])
            } else {
                tr("PC tunnel priority not changed (UAC declined)", &[])
            })),
            ("replace", false) => {
                app.exit(0);
                Ok(None)
            }
            ("legacy" | "replace", true) if !cfg!(debug_assertions) => {
                ctx.watch.stop(); // the watcher must not restart rh-core while the data moves
                app_install::install(&ctx.paths, None, &exe, |_| {})?;
                relaunch(&app, &app_install::installed_exe(&exe))?;
                Ok(None)
            }
            ("move", true) if !cfg!(debug_assertions) => {
                app_install::move_here(&ctx.paths, &exe, || ctx.watch.stop()).map_err(|e| {
                    tr("Not moved: {e}\n\nEverything stays where it was; you are asked again next time.", &[("e", e)])
                })?;
                relaunch(&app, &exe)?;
                Ok(None)
            }
            _ => Ok(None),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
