use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

pub type Result<T> = std::result::Result<T, String>;
pub mod code;
pub mod layout;
pub mod profiles;
pub const GAME_BUILD: &str = "VoidEngine v1.820.5.1 Content v551_Retail_env2_playfab";
pub const GAME_VERSION: &str = "VoidEngine v1.820.5.1 Content v551";
pub const PROTOCOL: &str = "dlmods-v2";
#[cfg(windows)]
pub mod win;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub mods: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub game_build: String,
    pub entry: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<String>,
}

#[derive(Debug)]
pub struct Package {
    pub manifest: Manifest,
    pub script: String,
    pub digest: String,
}

#[derive(Debug)]
pub struct Plan {
    pub name: String,
    pub packages: Vec<Package>,
    pub fingerprint: String,
}

pub fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

pub fn contained(root: &Path, relative: &str) -> Result<PathBuf> {
    if relative.is_empty()
        || relative.contains(':')
        || Path::new(relative)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(format!("Unsafe package path: {relative}"));
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !path.starts_with(&root) {
        return Err("Package path leaves its directory".into());
    }
    Ok(path)
}

/// Install a single-entry Lua package without overwriting existing mods.
pub fn install(root: &Path, manifest_path: &Path) -> Result<String> {
    if manifest_path
        .extension()
        .and_then(|v| v.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("zip"))
    {
        use std::io::Read;
        let file = fs::File::open(manifest_path).map_err(|e| e.to_string())?;
        if file.metadata().map_err(|e| e.to_string())?.len() > 16 * 1024 * 1024 {
            return Err("Mod archive exceeds 16 MiB".into());
        }
        let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        let mut read_entry = |name: &str| -> Result<String> {
            let file = archive.by_name(name).map_err(|e| e.to_string())?;
            if file.size() > 1024 * 1024 {
                return Err("Mod entry exceeds 1 MiB".into());
            }
            let mut result = String::new();
            file.take(1024 * 1024 + 1)
                .read_to_string(&mut result)
                .map_err(|e| e.to_string())?;
            if result.len() > 1024 * 1024 {
                return Err("Expanded mod entry exceeds 1 MiB".into());
            }
            Ok(result)
        };
        let raw = read_entry("mod.toml")?;
        let manifest: Manifest = toml::from_str(&raw).map_err(|e| e.to_string())?;
        validate_manifest(&manifest)?;
        let script = read_entry(&manifest.entry)?;
        return install_contents(root, &manifest, raw, script);
    }
    if manifest_path.file_name().and_then(|v| v.to_str()) != Some("mod.toml") {
        return Err("Select mod.toml".into());
    }
    let folder = manifest_path.parent().ok_or("Manifest directory missing")?;
    let raw = text(&contained(folder, "mod.toml")?)?;
    let manifest: Manifest = toml::from_str(&raw).map_err(|e| e.to_string())?;
    validate_manifest(&manifest)?;
    let source = text(&contained(folder, &manifest.entry)?)?;
    install_contents(root, &manifest, raw, source)
}

fn validate_manifest(m: &Manifest) -> Result<()> {
    if !valid_id(&m.id)
        || m.version.is_empty()
        || m.game_build != GAME_BUILD
        || m.entry.is_empty()
        || m.entry.contains(':')
        || m.entry.eq_ignore_ascii_case("mod.toml")
        || Path::new(&m.entry)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Unsupported or invalid mod manifest".into());
    }
    Ok(())
}

fn install_contents(
    root: &Path,
    manifest: &Manifest,
    raw: String,
    source: String,
) -> Result<String> {
    let mods = contained(root, "mods")?;
    let destination = mods.join(&manifest.id);
    if destination.exists() {
        return Err(format!("{} is already installed. Keep its backup and remove the old folder before installing a replacement.",manifest.id));
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let staging = mods.join(format!(".install-{}-{nonce}", std::process::id()));
    fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let result = (|| -> Result<()> {
        let entry = staging.join(&manifest.entry);
        fs::create_dir_all(entry.parent().ok_or("Invalid entry path")?)
            .map_err(|e| e.to_string())?;
        fs::write(&entry, source).map_err(|e| e.to_string())?;
        fs::write(staging.join("mod.toml"), raw).map_err(|e| e.to_string())?;
        fs::rename(&staging, &destination).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result?;
    Ok(manifest.id.clone())
}

pub fn installed(root: &Path) -> Result<Vec<Manifest>> {
    let mut result = Vec::new();
    for entry in fs::read_dir(root.join("mods")).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let id = entry.file_name().to_string_lossy().to_string();
        if !valid_id(&id) || !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            continue;
        }
        let folder = contained(&root.join("mods"), &id)?;
        let m: Manifest =
            toml::from_str(&text(&contained(&folder, "mod.toml")?)?).map_err(|e| e.to_string())?;
        validate_manifest(&m)?;
        if m.id != id {
            return Err(format!("Mod folder/ID mismatch: {id}"));
        }
        result.push(m);
    }
    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}
pub fn profile(root: &Path, id: &str) -> Result<Profile> {
    if !valid_id(id) {
        return Err("Invalid profile ID".into());
    }
    toml::from_str(&text(&contained(
        &root.join("profiles"),
        &format!("{id}.toml"),
    )?)?)
    .map_err(|e| e.to_string())
}
pub fn compatibility_report(root: &Path, id: &str) -> Result<String> {
    let p = plan(root, id)?;
    let mut report=format!("Deathloop matchmaking setup\r\nProfile: {}\r\nProtocol: {PROTOCOL}\r\nSupported game build: {GAME_BUILD}\r\n",p.name);
    if p.packages.is_empty() {
        report.push_str("Mode: vanilla (no enabled mods)\r\n");
    } else {
        report.push_str(&format!(
            "Mode: modded\r\nMatchmaking pool: {}\r\nFull setup fingerprint: {}\r\n",
            pool_key(&p.fingerprint, GAME_BUILD)?,
            p.fingerprint
        ));
    }
    report.push_str("\r\nEnabled mods in load order:\r\n");
    for (i, pkg) in p.packages.iter().enumerate() {
        report.push_str(&format!(
            "{}. {} | ID: {} | version: {}\r\n   Package checksum: {}\r\n",
            i + 1,
            pkg.manifest.name,
            pkg.manifest.id,
            pkg.manifest.version,
            pkg.digest
        ));
    }
    report.push_str("\r\nCompare mode, protocol, game build, enabled mod IDs, versions, load order and compatibility checksums. Checksums cover functional manifest fields and exact script contents. Display names, descriptions and manifest formatting are excluded. Script comments still count. Disabled mods and profile names do not affect the pool.\r\n\r\nThis records the selected launch setup; consult LAUNCH RESULT and the runtime log for activation. Both players must restart through the manager after changes. Matching identifiers do not guarantee network connectivity.\r\n");
    Ok(report)
}
pub fn set_enabled(root: &Path, profile_id: &str, id: &str, enabled: bool) -> Result<()> {
    let mut p = profile(root, profile_id)?;
    let available: BTreeMap<_, _> = installed(root)?
        .into_iter()
        .map(|m| (m.id.clone(), m))
        .collect();
    if !available.contains_key(id) {
        return Err("Mod is not installed".into());
    }
    if enabled && !p.mods.iter().any(|m| m == id) {
        p.mods.push(id.into());
    }
    if !enabled {
        p.mods.retain(|m| m != id);
    }
    fn visit(
        id: &str,
        all: &BTreeMap<String, Manifest>,
        visiting: &mut BTreeSet<String>,
        order: &mut Vec<String>,
    ) -> Result<()> {
        if order.iter().any(|s| s == id) {
            return Ok(());
        }
        if !visiting.insert(id.into()) {
            return Err(format!("Dependency cycle involving {id}"));
        }
        let m = all
            .get(id)
            .ok_or_else(|| format!("Required mod not installed: {id}"))?;
        for dep in &m.dependencies {
            visit(dep, all, visiting, order)?;
        }
        visiting.remove(id);
        order.push(id.into());
        Ok(())
    }
    let mut order = Vec::new();
    for id in &p.mods {
        visit(id, &available, &mut BTreeSet::new(), &mut order)?;
    }
    if !enabled && order.iter().any(|s| s == id) {
        return Err(format!(
            "Another enabled mod requires {id}. Disable that mod first."
        ));
    }
    for id in &order {
        for conflict in &available[id].conflicts {
            if order.contains(conflict) {
                return Err(format!("{id} conflicts with {conflict}"));
            }
        }
    }
    p.mods = order;
    let dir = contained(root, "profiles")?;
    let pending = dir.join(format!("{profile_id}.pending"));
    fs::write(
        &pending,
        toml::to_string_pretty(&p).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(&pending, dir.join(format!("{profile_id}.toml"))).map_err(|e| e.to_string())?;
    Ok(())
}

fn text(path: &Path) -> Result<String> {
    if fs::metadata(path).map_err(|e| e.to_string())?.len() > 1024 * 1024 {
        return Err(format!("File exceeds 1 MiB: {}", path.display()));
    }
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn hash(parts: &[&[u8]]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update((p.len() as u64).to_le_bytes());
        h.update(p);
    }
    format!("{:x}", h.finalize())
}

pub fn plan(root: &Path, profile: &str) -> Result<Plan> {
    if !valid_id(profile) {
        return Err("Invalid profile ID".into());
    }
    let profiles = root.join("profiles");
    let p: Profile = toml::from_str(&text(&contained(&profiles, &format!("{profile}.toml"))?)?)
        .map_err(|e| e.to_string())?;
    if p.mods.len() > 64 {
        return Err("Too many mods in profile".into());
    }
    let mut packages = Vec::new();
    let mut loaded = BTreeSet::new();
    for id in &p.mods {
        if !valid_id(id) || !loaded.insert(id.clone()) {
            return Err(format!("Invalid or duplicate mod: {id}"));
        }
        let folder = contained(&root.join("mods"), id)?;
        let raw = text(&contained(&folder, "mod.toml")?)?;
        let manifest: Manifest = toml::from_str(&raw).map_err(|e| format!("{id}: {e}"))?;
        if manifest.id != *id || manifest.version.is_empty() || manifest.game_build != GAME_BUILD {
            return Err(format!("{id}: ID/version/game build mismatch"));
        }
        let script = text(&contained(&folder, &manifest.entry)?)?;
        validate_manifest(&manifest)?;
        // Explicit functional fields: cosmetic labels, descriptions and TOML layout
        // do not split the pool. Code bytes remain exact, including Lua comments.
        let deps = manifest.dependencies.join("\n");
        let conflicts = manifest.conflicts.join("\n");
        let digest = hash(&[
            manifest.id.as_bytes(),
            manifest.version.as_bytes(),
            manifest.game_build.as_bytes(),
            manifest.entry.as_bytes(),
            deps.as_bytes(),
            conflicts.as_bytes(),
            script.as_bytes(),
        ]);
        packages.push(Package {
            manifest,
            script,
            digest,
        });
    }
    // Order is explicit and part of the compatibility fingerprint. Dependencies must precede consumers.
    let mut prior = BTreeSet::new();
    for p in &packages {
        for dep in &p.manifest.dependencies {
            if !prior.contains(dep) {
                return Err(format!(
                    "{} requires {dep} earlier in the profile",
                    p.manifest.id
                ));
            }
        }
        for conflict in &p.manifest.conflicts {
            if loaded.contains(conflict) {
                return Err(format!("{} conflicts with {conflict}", p.manifest.id));
            }
        }
        prior.insert(p.manifest.id.clone());
    }
    let digests = packages
        .iter()
        .map(|p| p.digest.as_str())
        .collect::<Vec<_>>()
        .join(":");
    let fingerprint = hash(&[
        PROTOCOL.as_bytes(),
        GAME_BUILD.as_bytes(),
        digests.as_bytes(),
    ]);
    Ok(Plan {
        name: p.name,
        packages,
        fingerprint,
    })
}

pub fn pool_key(fingerprint: &str, original: &str) -> Result<String> {
    let expected = format!("{}_Retail_env2_playfab", pool_version(fingerprint)?);
    if original != GAME_BUILD && original != format!("{GAME_BUILD}_deluxe") && original != expected
    {
        return Err(format!("Unsupported compatibility key: {original}"));
    }
    if fingerprint.len() != 64 || !fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid fingerprint".into());
    }
    Ok(format!(
        "{}_Retail_env2_playfab",
        pool_version(fingerprint)?
    ))
}

pub fn pool_version(fingerprint: &str) -> Result<String> {
    if fingerprint.len() != 64 || !fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid fingerprint".into());
    }
    let prefix = format!("{PROTOCOL}-");
    // Preserve the version component's length. Existing cache buffers need no reallocation.
    Ok(format!(
        "{prefix}{}",
        &fingerprint[..GAME_VERSION.len() - prefix.len()]
    ))
}

pub trait Memory {
    fn read(&self, address: usize, length: usize) -> Result<Vec<u8>>;
    fn write(&self, address: usize, data: &[u8]) -> Result<()>;
}

pub struct Write {
    pub address: usize,
    pub expected: Vec<u8>,
    pub value: Vec<u8>,
}

#[derive(Default)]
pub struct Patches {
    owners: BTreeMap<usize, String>,
}

impl Patches {
    pub fn validate(&self, memory: &impl Memory, owner: &str, writes: &[Write]) -> Result<()> {
        if writes.len() > 4096 {
            return Err("Transaction too large".into());
        }
        let mut bytes = BTreeSet::new();
        for w in writes {
            if w.value.is_empty() || w.value.len() > 256 || w.value.len() != w.expected.len() {
                return Err("Invalid patch length".into());
            }
            let end = w
                .address
                .checked_add(w.value.len())
                .ok_or("Address overflow")?;
            for a in w.address..end {
                if !bytes.insert(a) {
                    return Err("Overlapping writes in transaction".into());
                }
                if let Some(other) = self.owners.get(&a) {
                    if other != owner {
                        return Err(format!("Patch conflict: {owner} overlaps {other} at {a:X}"));
                    }
                }
            }
            if memory.read(w.address, w.expected.len())? != w.expected {
                return Err(format!("Memory changed at {:X}", w.address));
            }
        }
        Ok(())
    }

    /// Reserve successfully installed code sites in the same ownership map as data mods.
    pub fn claim(&mut self, owner: &str, writes: &[Write]) {
        for w in writes {
            for a in w.address..w.address + w.value.len() {
                self.owners.insert(a, owner.into());
            }
        }
    }

    pub fn apply(&mut self, memory: &impl Memory, owner: &str, writes: &[Write]) -> Result<()> {
        self.validate(memory, owner, writes)?;
        for (i, w) in writes.iter().enumerate() {
            let result = memory.write(w.address, &w.value).and_then(|_| {
                if memory.read(w.address, w.value.len())? == w.value {
                    Ok(())
                } else {
                    Err("Write verification failed".into())
                }
            });
            if let Err(e) = result {
                let mut rollback = Vec::new();
                for old in writes[..=i].iter().rev() {
                    match memory.read(old.address, old.value.len()) {
                        Ok(now) if now == old.value => {
                            if let Err(e) = memory.write(old.address, &old.expected) {
                                rollback.push(e);
                            }
                        }
                        Ok(now) if now == old.expected => {}
                        _ => rollback.push(format!("Cannot safely restore {:X}", old.address)),
                    }
                }
                return Err(format!("{e}; rollback problems: {rollback:?}"));
            }
        }
        self.claim(owner, writes);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    struct Ram(RefCell<Vec<u8>>);
    impl Memory for Ram {
        fn read(&self, a: usize, n: usize) -> Result<Vec<u8>> {
            self.0
                .borrow()
                .get(a..a + n)
                .map(|v| v.to_vec())
                .ok_or("bounds".into())
        }
        fn write(&self, a: usize, v: &[u8]) -> Result<()> {
            self.0.borrow_mut()[a..a + v.len()].copy_from_slice(v);
            Ok(())
        }
    }
    #[test]
    fn identifiers() {
        assert!(valid_id("all-maps-are-invadable"));
        for s in ["../x", "C:\\foo", "x/y", "", "UPPER"] {
            assert!(!valid_id(s));
        }
    }
    #[test]
    fn same_length_non_vanilla_key() {
        for key in [GAME_BUILD.to_owned(), format!("{GAME_BUILD}_deluxe")] {
            let result = pool_key(&"a".repeat(64), &key).unwrap();
            assert!(result.len() <= key.len());
            assert_eq!(result.len(), GAME_BUILD.len());
            assert_ne!(result, key);
            assert!(result.starts_with("dlmods-v2-"));
        }
        assert!(pool_key(&"a".repeat(64), "other-build").is_err());
    }
    #[test]
    fn rejects_conflicts_and_validates_before_writes() {
        let ram = Ram(RefCell::new(vec![0; 8]));
        let mut p = Patches::default();
        p.apply(
            &ram,
            "a",
            &[Write {
                address: 0,
                expected: vec![0],
                value: vec![1],
            }],
        )
        .unwrap();
        assert!(p
            .apply(
                &ram,
                "b",
                &[Write {
                    address: 0,
                    expected: vec![1],
                    value: vec![2]
                }]
            )
            .is_err());
        assert!(p
            .apply(
                &ram,
                "a",
                &[
                    Write {
                        address: 1,
                        expected: vec![0],
                        value: vec![2]
                    },
                    Write {
                        address: 2,
                        expected: vec![9],
                        value: vec![2]
                    }
                ]
            )
            .is_err());
        assert_eq!(*ram.0.borrow(), vec![1, 0, 0, 0, 0, 0, 0, 0]);
    }
    #[test]
    fn bundled_profile() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("package");
        let p = plan(&root, "default").unwrap();
        assert_eq!(p.packages.len(), 0);
        assert_eq!(p.fingerprint, plan(&root, "default").unwrap().fingerprint);
    }

    #[test]
    fn refuses_path_traversal() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        for path in ["../Cargo.toml", "C:\\Windows\\win.ini", "/etc/passwd", ""] {
            assert!(contained(root, path).is_err());
        }
    }

    #[test]
    fn normalized_suffix_uses_same_pool() {
        let digest = "a".repeat(64);
        assert_eq!(
            pool_key(&digest, GAME_BUILD).unwrap(),
            pool_key(&digest, &format!("{GAME_BUILD}_deluxe")).unwrap()
        );
        assert_ne!(
            pool_key(&digest, GAME_BUILD).unwrap(),
            pool_key(&"b".repeat(64), GAME_BUILD).unwrap()
        );
    }

    #[test]
    fn overlapping_transaction_is_rejected() {
        let ram = Ram(RefCell::new(vec![0; 4]));
        let mut patches = Patches::default();
        assert!(patches
            .apply(
                &ram,
                "a",
                &[
                    Write {
                        address: 0,
                        expected: vec![0, 0],
                        value: vec![1, 1]
                    },
                    Write {
                        address: 1,
                        expected: vec![0],
                        value: vec![1]
                    },
                ]
            )
            .is_err());
        assert_eq!(*ram.0.borrow(), vec![0; 4]);
    }

    #[test]
    fn write_failure_rolls_back_successful_prior_writes() {
        struct Failing(Ram);
        impl Memory for Failing {
            fn read(&self, a: usize, n: usize) -> Result<Vec<u8>> {
                self.0.read(a, n)
            }
            fn write(&self, a: usize, v: &[u8]) -> Result<()> {
                if a == 1 {
                    Err("injected write failure".into())
                } else {
                    self.0.write(a, v)
                }
            }
        }
        let ram = Failing(Ram(RefCell::new(vec![0; 4])));
        let mut patches = Patches::default();
        let err = patches
            .apply(
                &ram,
                "a",
                &[
                    Write {
                        address: 0,
                        expected: vec![0],
                        value: vec![1],
                    },
                    Write {
                        address: 1,
                        expected: vec![0],
                        value: vec![1],
                    },
                ],
            )
            .unwrap_err();
        assert!(err.contains("rollback problems: []"));
        assert_eq!(*ram.0 .0.borrow(), vec![0; 4]);
    }
}
