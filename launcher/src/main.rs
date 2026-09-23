#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod dialogs;
mod inject;
mod startup;
mod ui;
use dlmod_core::{plan, Result};
use std::path::PathBuf;

fn root() -> Result<PathBuf> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("Executable directory missing")?;
    if dir.join("profiles").is_dir() {
        Ok(dir.into())
    } else {
        Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("package"))
    }
}
fn main() {
    let result = (|| -> Result<()> {
        let root = root()?;
        let args = std::env::args().skip(1).collect::<Vec<_>>();
        if args.is_empty() {
            return ui::show(root);
        }
        let profile = args.get(1).map(|s| s.as_str()).unwrap_or("default");
        let output = match args[0].as_str() {
            "--report" => dlmod_core::compatibility_report(&root, profile)?,
            "--prepare" => {
                let game = args
                    .get(2)
                    .map(PathBuf::from)
                    .or_else(|| startup::game_path(&root))
                    .ok_or("Select the game executable first")?;
                let modded = startup::prepare(&root, &game, profile)?;
                format!(
                    "Startup prepared: {}",
                    if modded { "modded" } else { "vanilla" }
                )
            }
            "--set-enabled" => {
                let id = args.get(2).ok_or("Supply a mod ID")?;
                let enabled = args.get(3).ok_or("Supply on or off")?;
                if enabled != "on" && enabled != "off" {
                    return Err("Use on or off".into());
                }
                dlmod_core::set_enabled(&root, profile, id, enabled == "on")?;
                format!("{id}: {enabled}")
            }
            "--install" => {
                let source = args.get(1).ok_or("Supply the path to the mod.toml file")?;
                format!(
                    "Installed {}",
                    dlmod_core::install(&root, std::path::Path::new(source))?
                )
            }
            "--validate" => {
                let p = plan(&root, profile)?;
                format!(
                    "{}\n{} mods\nfingerprint {}",
                    p.name,
                    p.packages.len(),
                    p.fingerprint
                )
            }
            "--check" => inject::inspect(&root, profile)?,
            "--enable" => inject::enable(&root, profile)?,
            "--ui-check" => return ui::show(root),
            "--ui-exercise" => {
                return ui::show(PathBuf::from(
                    args.get(1).ok_or("Supply an isolated test directory")?,
                ))
            }
            _ => return Err("Usage: --validate|--check|--enable [profile-id]".into()),
        };
        println!("{output}");
        std::fs::create_dir_all(root.join("logs")).map_err(|e| e.to_string())?;
        std::fs::write(root.join("logs/manager-last.txt"), output).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if let Err(e) = result {
        eprintln!("{e}");
        if let Ok(root) = root() {
            let _ = std::fs::create_dir_all(root.join("logs"));
            let _ = std::fs::write(root.join("logs/manager-last.txt"), &e);
        }
        if std::env::args().len() == 1 {
            ui::message(&e);
        }
        std::process::exit(1);
    }
}
