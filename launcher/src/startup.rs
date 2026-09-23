use dlmod_core::{
    plan, pool_version,
    win::{game_pid, modules},
    Result,
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn install_system_input(folder: &Path) -> Result<()> {
    let mut path = [0u16; 32768];
    let n = unsafe {
        windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(
            path.as_mut_ptr(),
            path.len() as u32,
        )
    } as usize;
    if n == 0 || n >= path.len() {
        return Err("Cannot locate Windows system directory".into());
    }
    let source = PathBuf::from(String::from_utf16_lossy(&path[..n])).join("dinput8.dll");
    let bytes = fs::read(source).map_err(|e| format!("Cannot read Windows input library: {e}"))?;
    install_input_copy(folder, &bytes)
}
fn install_input_copy(folder: &Path, bytes: &[u8]) -> Result<()> {
    let hash = digest(bytes);
    let target = folder.join("dlmod-system-input.dll");
    let receipt = folder.join("dlmod-system-input.sha256");
    if target.exists() {
        let current = digest(&fs::read(&target).map_err(|e| e.to_string())?);
        if current != hash
            && fs::read_to_string(&receipt)
                .map(|r| r.trim() != current)
                .unwrap_or(true)
        {
            return Err(
                "A different dlmod-system-input.dll exists; it was not overwritten.".into(),
            );
        }
        if current == hash {
            return fs::write(receipt, hash).map_err(|e| e.to_string());
        }
    }
    let pending = folder.join("dlmod-system-input.pending");
    fs::write(&pending, bytes).map_err(|e| e.to_string())?;
    fs::rename(pending, target).map_err(|e| e.to_string())?;
    fs::write(receipt, hash).map_err(|e| e.to_string())
}
// Only detach a DLL whose contents match our installation receipt. Never restore
// the older protection-disabling DLL automatically during a vanilla launch.
fn detach(folder: &Path) -> Result<()> {
    let target = folder.join("dinput8.dll");
    if target.exists() {
        let bytes = fs::read(&target).map_err(|e| e.to_string())?;
        let hash = digest(&bytes);
        let receipt = fs::read_to_string(folder.join("dlmod-startup.sha256"))
            .map_err(|_| "Cannot identify installed dinput8.dll; it was left untouched")?;
        if receipt.trim() != hash {
            return Err(
                "Installed dinput8.dll does not match our receipt; it was left untouched.".into(),
            );
        }
        let backup = folder.join(format!("dlmod-disabled-{hash}.dll"));
        if backup.exists() {
            if fs::read(&backup).map_err(|e| e.to_string())? != bytes {
                return Err("Disabled DLL backup differs; no files changed.".into());
            }
            fs::remove_file(&target).map_err(|e| e.to_string())?;
        } else {
            fs::rename(&target, &backup).map_err(|e| e.to_string())?;
        }
    }
    let marker = folder.join("dlmod.launch");
    if marker.exists() {
        fs::remove_file(marker).map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub fn game_path(root: &Path) -> Option<PathBuf> {
    if let Ok(raw) = fs::read_to_string(root.join("game-path.txt")) {
        let path = PathBuf::from(raw.trim());
        if path.is_file() {
            return Some(path);
        }
    }
    if let Ok(pid) = game_pid() {
        if let Ok(list) = modules(pid) {
            return list
                .into_iter()
                .find(|m| m.0.eq_ignore_ascii_case("Deathloop.exe"))
                .map(|m| PathBuf::from(m.2));
        }
    }
    None
}
pub fn save_game_path(root: &Path, path: &Path) -> Result<()> {
    if !path
        .file_name()
        .is_some_and(|s| s.eq_ignore_ascii_case("Deathloop.exe"))
        || !path.is_file()
    {
        return Err("Select Deathloop.exe in the game's installation folder".into());
    }
    fs::write(
        root.join("game-path.txt"),
        path.to_string_lossy().as_bytes(),
    )
    .map_err(|e| e.to_string())
}
pub fn prepare(root: &Path, game: &Path, profile: &str) -> Result<bool> {
    match game_pid() {
        Ok(_) => return Err("Close Deathloop before launching with different mods.".into()),
        Err(e) if e.contains("found 0.") => {}
        Err(e) => return Err(e),
    }
    save_game_path(root, game)?;
    let folder = game.parent().ok_or("Game folder missing")?;
    let p = plan(root, profile)?;
    let modded = !p.packages.is_empty();
    if !modded {
        detach(folder)?;
        return Ok(false);
    }
    let diagnostic = folder.join("dlmod-startup.log");
    if diagnostic.exists() {
        fs::remove_file(diagnostic).map_err(|e| e.to_string())?;
    }
    let source = fs::read(root.join("dlmod_startup.dll"))
        .map_err(|e| format!("Startup support is missing: {e}"))?;
    let hash = digest(&source);
    let target = folder.join("dinput8.dll");
    let record = folder.join("dlmod-startup.sha256");
    if target.exists() {
        let current = fs::read(&target).map_err(|e| e.to_string())?;
        let existing = digest(&current);
        if existing != hash {
            let ours = fs::read_to_string(&record)
                .map(|s| s.trim() == existing)
                .unwrap_or(false);
            let legacy = [
                "628495e5d5d6dd15745d3ef10222e16e48c7842502a4a5a102adf367631a87ad",
                "98fb8fdca2ab57600e3900a026acea960df9ff1b86475da4fb5f04914c3cb1d0",
            ]
            .contains(&existing.as_str());
            if !ours && !legacy {
                return Err("A different dinput8.dll is installed. It has not been changed. Remove or relocate that other loader before using this manager.".into());
            }
            if !ours {
                let backup = folder.join("dlmod-original-dinput8.dll");
                if backup.exists() {
                    if fs::read(&backup).map_err(|e| e.to_string())? != current {
                        return Err("A different original-DLL backup already exists; it has not been overwritten.".into());
                    }
                } else {
                    fs::copy(&target, &backup).map_err(|e| {
                        format!("Cannot back up startup DLL (try running as administrator): {e}")
                    })?;
                }
            }
        }
    }
    install_system_input(folder)?;
    // Prepare a new file before replacing the previous one. Preserve the original separately.
    if !target.exists() || digest(&fs::read(&target).map_err(|e| e.to_string())?) != hash {
        let pending = folder.join("dlmod-startup.pending");
        fs::write(&pending, &source).map_err(|e| {
            format!("Cannot install startup support. Run the manager as administrator. {e}")
        })?;
        fs::rename(&pending, &target).map_err(|e| {
            format!("Cannot replace startup support; close the game and retry. {e}")
        })?;
    }
    fs::write(&record, hash).map_err(|e| e.to_string())?;
    let marker = folder.join("dlmod.launch");
    if modded {
        let mut bytes = [0u8; 80];
        bytes[..8].copy_from_slice(b"DLMOD02\0");
        let version = pool_version(&p.fingerprint)?;
        bytes[8..8 + version.len()].copy_from_slice(version.as_bytes());
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs()
            + 300;
        bytes[72..80].copy_from_slice(&expiry.to_le_bytes());
        fs::write(&marker, bytes).map_err(|e| e.to_string())?;
    } else if marker.exists() {
        fs::remove_file(&marker).map_err(|e| e.to_string())?;
    }
    Ok(modded)
}
pub fn cancel(root: &Path) -> Result<()> {
    if let Some(path) = game_path(root) {
        let marker = path
            .parent()
            .ok_or("Game folder missing")?
            .join("dlmod.launch");
        if marker.exists() {
            fs::remove_file(marker).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_input_copy_preserves_unknown_files_and_updates_owned_copy() {
        let dir = std::env::temp_dir().join(format!(
            "dl-input-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&dir).unwrap();
        install_input_copy(&dir, b"system-v1").unwrap();
        install_input_copy(&dir, b"system-v2").unwrap();
        let target = dir.join("dlmod-system-input.dll");
        assert_eq!(fs::read(&target).unwrap(), b"system-v2");
        fs::write(&target, b"unknown").unwrap();
        assert!(install_input_copy(&dir, b"system-v3").is_err());
        assert_eq!(fs::read(&target).unwrap(), b"unknown");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn vanilla_detaches_only_owned_dll_and_preserves_backup() {
        let dir = std::env::temp_dir().join(format!(
            "dl-detach-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&dir).unwrap();
        let dll = dir.join("dinput8.dll");
        fs::write(&dll, b"unknown").unwrap();
        assert!(detach(&dir).is_err());
        assert_eq!(fs::read(&dll).unwrap(), b"unknown");
        fs::write(dir.join("dlmod-startup.sha256"), digest(b"owned")).unwrap();
        assert!(detach(&dir).is_err());
        fs::write(&dll, b"owned").unwrap();
        fs::write(dir.join("dlmod.launch"), b"request").unwrap();
        detach(&dir).unwrap();
        assert!(!dll.exists());
        assert!(!dir.join("dlmod.launch").exists());
        let backup = dir.join(format!("dlmod-disabled-{}.dll", digest(b"owned")));
        assert_eq!(fs::read(&backup).unwrap(), b"owned");
        fs::write(&dll, b"owned").unwrap();
        detach(&dir).unwrap();
        assert!(!dll.exists());
        assert_eq!(fs::read(&backup).unwrap(), b"owned");
        fs::remove_dir_all(dir).unwrap();
    }
}
