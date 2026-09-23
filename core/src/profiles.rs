use crate::{contained, profile, Profile, Result};
use std::{fs, path::Path};

pub fn list(root: &Path) -> Result<Vec<(String, Profile)>> {
    let mut rows = Vec::new();
    for entry in fs::read_dir(root.join("profiles")).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("Invalid profile filename")?;
        rows.push((id.to_string(), profile(root, id)?));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(rows)
}
fn checked_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err("Use a profile name of 1–80 characters.".into());
    }
    Ok(name.into())
}
pub fn save(root: &Path, id: &str, p: &Profile) -> Result<()> {
    let path = contained(&root.join("profiles"), &format!("{id}.toml"))?;
    let pending = path.with_extension("pending");
    fs::write(
        &pending,
        toml::to_string_pretty(p).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(pending, path).map_err(|e| e.to_string())
}
pub fn create(root: &Path, name: &str, copy: Option<&str>) -> Result<String> {
    let name = checked_name(name)?;
    let mods = if let Some(id) = copy {
        profile(root, id)?.mods
    } else {
        Vec::new()
    };
    let entries = list(root)?;
    if entries
        .iter()
        .any(|(_, p)| p.name.to_lowercase() == name.to_lowercase())
    {
        return Err("A profile with that name already exists.".into());
    }
    for i in 1..10000 {
        let id = format!("profile-{i}");
        if !root.join("profiles").join(format!("{id}.toml")).exists() {
            let p = Profile { name, mods };
            let path = root.join("profiles").join(format!("{id}.toml"));
            use std::io::Write;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|e| e.to_string())?;
            file.write_all(
                toml::to_string_pretty(&p)
                    .map_err(|e| e.to_string())?
                    .as_bytes(),
            )
            .map_err(|e| e.to_string())?;
            return Ok(id);
        }
    }
    Err("Too many profiles".into())
}
pub fn rename(root: &Path, id: &str, name: &str) -> Result<()> {
    let name = checked_name(name)?;
    if list(root)?
        .iter()
        .any(|(other, p)| other != id && p.name.to_lowercase() == name.to_lowercase())
    {
        return Err("A profile with that name already exists.".into());
    }
    let mut p = profile(root, id)?;
    p.name = name;
    save(root, id, &p)
}
pub fn delete(root: &Path, id: &str) -> Result<String> {
    let entries = list(root)?;
    let next = entries
        .iter()
        .find(|(other, _)| other != id)
        .ok_or("Keep at least one profile.")?
        .0
        .clone();
    // Retain one recoverable copy per profile ID; deleting never removes mods.
    let path = contained(&root.join("profiles"), &format!("{id}.toml"))?;
    fs::rename(&path, path.with_extension("deleted")).map_err(|e| e.to_string())?;
    Ok(next)
}
