use crate::dialogs;
use dlmod_core::{profiles, Result};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{
    cell::RefCell,
    io::Write,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};
include!("view.rs");
pub use crate::dialogs::message;
struct State {
    root: PathBuf,
    selected: String,
    ids: Vec<String>,
}
fn refresh(ui: &Manager, s: &mut State) -> Result<()> {
    let entries = profiles::list(&s.root)?;
    if !entries.iter().any(|(id, _)| id == &s.selected) {
        s.selected = entries.first().ok_or("No profiles found")?.0.clone();
    }
    let index = entries
        .iter()
        .position(|(id, _)| id == &s.selected)
        .unwrap();
    ui.set_profiles(ModelRc::new(VecModel::from(
        entries
            .iter()
            .map(|(_, p)| p.name.clone().into())
            .collect::<Vec<_>>(),
    )));
    ui.set_profile_index(index as i32);
    ui.set_profile_name(entries[index].1.name.clone().into());
    let enabled = &entries[index].1.mods;
    ui.set_mode_text(if enabled.is_empty() {
        "Next launch: VANILLA".into()
    } else {
        format!("Next launch: MODDED · {} enabled", enabled.len()).into()
    });
    ui.set_mods(ModelRc::new(VecModel::from(
        dlmod_core::installed(&s.root)?
            .into_iter()
            .map(|m| ModRow {
                checked: enabled.contains(&m.id),
                id: m.id.into(),
                name: m.name.into(),
                version: m.version.into(),
                description: m.description.into(),
            })
            .collect::<Vec<_>>(),
    )));
    ui.set_game_path(
        crate::startup::game_path(&s.root)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Select Deathloop.exe".into())
            .into(),
    );
    s.ids = entries.into_iter().map(|(id, _)| id).collect();
    std::fs::write(s.root.join("selected-profile.txt"), &s.selected).map_err(|e| e.to_string())?;
    Ok(())
}
fn finish(ui: &Manager, s: &mut State, result: Result<String>) {
    ui.set_status_text(result.unwrap_or_else(|e| format!("Error: {e}")).into());
    if let Err(e) = refresh(ui, s) {
        ui.set_status_text(format!("Error: {e}").into());
    }
}
fn launch(root: &Path, profile: &str, game: &Path) -> Result<String> {
    let dir = root.join("logs");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    let mut log = std::fs::File::create(dir.join(format!("launch-{stamp}.log")))
        .map_err(|e| e.to_string())?;
    writeln!(log, "{}", dlmod_core::compatibility_report(root, profile)?)
        .map_err(|e| e.to_string())?;
    let result = (|| {
        writeln!(log, "Launch: direct\nExecutable: {}", game.display())
            .map_err(|e| e.to_string())?;
        let modded = crate::startup::prepare(root, game, profile)?;
        let mut child = match std::process::Command::new(game)
            .current_dir(game.parent().ok_or("Missing game directory")?)
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                let _ = crate::startup::cancel(root);
                return Err(format!("Cannot start {}: {e}", game.display()));
            }
        };
        let deadline = Instant::now() + Duration::from_secs(180);
        let mut observed = None;
        let mut exit_logged = false;
        let mut last = String::new();
        while Instant::now() < deadline {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                if !exit_logged {
                    writeln!(
                        log,
                        "Initial process exited: {:?}; waiting for a matching replacement process.",
                        status.code()
                    )
                    .map_err(|e| e.to_string())?;
                    exit_logged = true;
                }
                if status.code() == Some(0xC0000142u32 as i32) {
                    let _ = crate::startup::cancel(root);
                    return Err(
                        "Startup DLL initialization failed (0xC0000142). See startup diagnostic."
                            .into(),
                    );
                }
            }
            let matching = dlmod_core::win::game_pid().ok().and_then(|pid| {
                dlmod_core::win::modules(pid)
                    .ok()
                    .and_then(|modules| {
                        modules
                            .into_iter()
                            .find(|m| m.0.eq_ignore_ascii_case("Deathloop.exe"))
                    })
                    .and_then(|m| {
                        let actual = std::fs::canonicalize(&m.2).ok()?;
                        let selected = std::fs::canonicalize(game).ok()?;
                        actual
                            .to_string_lossy()
                            .eq_ignore_ascii_case(&selected.to_string_lossy())
                            .then_some(pid)
                    })
            });
            if let Some(pid) = matching {
                let since = observed.get_or_insert((pid, Instant::now()));
                if since.0 != pid {
                    *since = (pid, Instant::now());
                }
                if !modded && since.1.elapsed() >= Duration::from_secs(10) {
                    return Ok("Game launched without mods.".into());
                }
            } else {
                observed = None;
            }
            if modded && matching.is_some() {
                match crate::inject::inspect(root, profile) {
                    Ok(_) => return crate::inject::enable(root, profile),
                    Err(e) => last = e,
                }
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        let _ = crate::startup::cancel(root);
        Err(format!("Launch timed out: {last}"))
    })();
    if let Some(folder) = game.parent() {
        if let Ok(diagnostic) = std::fs::read_to_string(folder.join("dlmod-startup.log")) {
            let _ = writeln!(log, "Startup DLL diagnostic (last recorded): {diagnostic}");
        }
    }
    let _ = writeln!(
        log,
        "\nLAUNCH RESULT: {}",
        match &result {
            Ok(s) => s.as_str(),
            Err(e) => e.as_str(),
        }
    );
    result
}
pub fn show(root: PathBuf) -> Result<()> {
    let ui = Manager::new().map_err(|e| e.to_string())?;
    let selected = std::fs::read_to_string(root.join("selected-profile.txt"))
        .unwrap_or_else(|_| "default".into());
    let state = Rc::new(RefCell::new(State {
        root,
        selected,
        ids: vec![],
    }));
    refresh(&ui, &mut state.borrow_mut())?;
    {
        let weak = ui.as_weak();
        let s = state.clone();
        ui.on_choose_profile(move |i| {
            if let Some(ui) = weak.upgrade() {
                let mut s = s.borrow_mut();
                if let Some(id) = s.ids.get(i as usize) {
                    s.selected = id.clone();
                }
                finish(
                    &ui,
                    &mut s,
                    Ok("Selection saved. Changes apply on your next launch.".into()),
                );
            }
        });
    }
    {
        let weak = ui.as_weak();
        let s = state.clone();
        ui.on_toggle_mod(move |id, on| {
            if let Some(ui) = weak.upgrade() {
                let mut s = s.borrow_mut();
                let r = dlmod_core::set_enabled(&s.root, &s.selected, &id, on).map(|_| {
                    "Saved for the next launch. Restart if the game is already running.".into()
                });
                finish(&ui, &mut s, r);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let s = state.clone();
        ui.on_browse(move || {
            if let (Some(ui), Some(path)) = (weak.upgrade(), dialogs::pick(true)) {
                let mut s = s.borrow_mut();
                let r = crate::startup::save_game_path(&s.root, &path)
                    .map(|_| "Game location saved.".into());
                finish(&ui, &mut s, r);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let s = state.clone();
        ui.on_install(move || {
            if let (Some(ui), Some(path)) = (weak.upgrade(), dialogs::pick(false)) {
                let mut s = s.borrow_mut();
                let r = dlmod_core::install(&s.root, &path)
                    .map(|_| "Installed. Check the mod to enable it.".into());
                finish(&ui, &mut s, r);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let s = state.clone();
        ui.on_profile_action(move |action, name| {
            if let Some(ui) = weak.upgrade() {
                let mut s = s.borrow_mut();
                let r = (|| -> Result<String> {
                    match action.as_str() {
                        "new" => s.selected = profiles::create(&s.root, &name, None)?,
                        "duplicate" => {
                            s.selected = profiles::create(&s.root, &name, Some(&s.selected))?
                        }
                        "rename" => profiles::rename(&s.root, &s.selected, &name)?,
                        "delete" => {
                            if !dialogs::confirm("Delete this profile? Installed mods are kept.") {
                                return Ok("Cancelled.".into());
                            }
                            s.selected = profiles::delete(&s.root, &s.selected)?;
                        }
                        _ => return Err("Unknown profile action".into()),
                    }
                    Ok("Profile saved.".into())
                })();
                finish(&ui, &mut s, r);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let s = state.clone();
        ui.on_tool(move |action| {
            if let Some(ui) = weak.upgrade() {
                let s = s.borrow();
                let path = s.root.join(match action.as_str() {
                    "logs" => "logs",
                    "mods" => "mods",
                    _ => "README.md",
                });
                if action == "logs" {
                    let _ = std::fs::create_dir_all(&path);
                }
                if let Err(e) = dialogs::open(&path.to_string_lossy()) {
                    ui.set_status_text(e.into());
                }
            }
        });
    }
    {
        let weak = ui.as_weak();
        let s = state.clone();
        ui.on_remove_mod(move|id|{if let Some(ui)=weak.upgrade(){let mut s=s.borrow_mut();let r=(||->Result<String>{
        if dlmod_core::win::game_pid().is_ok(){return Err("Close Deathloop before removing mods.".into());}
        let users=profiles::list(&s.root)?.into_iter().filter(|(_,p)|p.mods.iter().any(|m|m==id.as_str())).map(|(_,p)|p.name).collect::<Vec<_>>();
        if !users.is_empty(){return Err(format!("Uncheck this mod in these profiles first: {}",users.join(", ")));}
        if !dialogs::confirm("Remove this mod from the installed list? Its files will be moved to the removed-mods folder."){return Ok("Cancelled.".into());}
        let source=dlmod_core::contained(&s.root.join("mods"),&id)?;
        let dir=s.root.join("removed-mods");std::fs::create_dir_all(&dir).map_err(|e|e.to_string())?;
        let n=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?.as_nanos();
        std::fs::rename(source,dir.join(format!("{id}-{n}"))).map_err(|e|e.to_string())?;
        Ok("Removed. Files can be recovered from removed-mods.".into())})();finish(&ui,&mut s,r);}});
    }
    {
        let weak = ui.as_weak();
        let s = state.clone();
        ui.on_launch(move || {
            if let Some(ui) = weak.upgrade() {
                let s = s.borrow();
                let Some(game) = crate::startup::game_path(&s.root).or_else(|| dialogs::pick(true))
                else {
                    return;
                };
                let root = s.root.clone();
                let profile = s.selected.clone();
                ui.set_busy(true);
                ui.set_status_text("Launching game…".into());
                let weak = ui.as_weak();
                std::thread::spawn(move || {
                    let result = std::panic::catch_unwind(|| launch(&root, &profile, &game))
                        .unwrap_or_else(|_| Err("Unexpected launch failure".into()));
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        ui.set_busy(false);
                        ui.set_status_text(result.unwrap_or_else(|e| format!("Error: {e}")).into());
                    });
                });
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.window().on_close_requested(move || {
            if weak.upgrade().is_some_and(|u| u.get_busy()) {
                slint::CloseRequestResponse::KeepWindowShown
            } else {
                slint::CloseRequestResponse::HideWindow
            }
        });
    }
    if std::env::args().any(|s| s == "--ui-exercise") {
        let root = state.borrow().root.clone();
        if !root
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("ui-test-"))
        {
            return Err("Use an isolated ui-test- directory".into());
        }
        ui.show().map_err(|e| e.to_string())?;
        let id = ui
            .get_mods()
            .row_data(0)
            .ok_or("Test needs an installed mod")?
            .id;
        ui.invoke_toggle_mod(id.clone(), true);
        if !ui.get_mods().row_data(0).unwrap().checked {
            return Err("Checkbox did not enable".into());
        }
        ui.invoke_profile_action("duplicate".into(), "Friends".into());
        if !ui.get_mods().row_data(0).unwrap().checked {
            return Err("Duplicate lost enabled mod".into());
        }
        ui.invoke_profile_action("rename".into(), "Evening".into());
        if ui.get_profile_name() != "Evening" {
            return Err("Rename failed".into());
        }
        ui.invoke_profile_action("new".into(), "Vanilla".into());
        if ui.get_mods().row_data(0).unwrap().checked {
            return Err("New profile not empty".into());
        }
        let index = state
            .borrow()
            .ids
            .iter()
            .position(|s| s == "default")
            .ok_or("Test needs default profile")?;
        ui.invoke_choose_profile(index as i32);
        if !ui.get_mods().row_data(0).unwrap().checked {
            return Err("Switch lost checked state".into());
        }
        std::fs::create_dir_all(root.join("logs")).map_err(|e| e.to_string())?;
        snapshot(&ui, &root.join("logs/main.png"))?;
        ui.set_editing(true);
        snapshot(&ui, &root.join("logs/profiles.png"))?;
        ui.invoke_toggle_mod(id, false);
        if ui.get_mods().row_data(0).unwrap().checked {
            return Err("Checkbox did not disable".into());
        }
        ui.hide().map_err(|e| e.to_string())?;
        return Ok(());
    }
    if std::env::args().any(|s| s == "--ui-check") {
        ui.show().map_err(|e| e.to_string())?;
        ui.hide().map_err(|e| e.to_string())?;
        return Ok(());
    }
    ui.run().map_err(|e| e.to_string())
}

fn snapshot(ui: &Manager, path: &Path) -> Result<()> {
    let pixels = ui.window().take_snapshot().map_err(|e| e.to_string())?;
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(file, pixels.width(), pixels.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .map_err(|e| e.to_string())?
        .write_image_data(pixels.as_bytes())
        .map_err(|e| e.to_string())
}
