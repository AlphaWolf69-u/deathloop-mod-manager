use dlmod_core::{install, plan, GAME_BUILD};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static SEQUENCE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "dlmod-package-test-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::create_dir(path.join("mods")).unwrap();
        fs::create_dir(path.join("profiles")).unwrap();
        Self(path)
    }
    fn package(&self, id: &str, extra: &str) {
        let dir = self.0.join("mods").join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("mod.toml"),format!("id = {id:?}\nname = {id:?}\nversion = '1.0.0'\ngame_build = {GAME_BUILD:?}\nentry = 'main.lua'\n{extra}\n")).unwrap();
        fs::write(dir.join("main.lua"), "return function() return 'ok' end").unwrap();
    }
    fn profile(&self, mods: &str) {
        fs::write(
            self.0.join("profiles/test.toml"),
            format!("name = 'Test'\nmods = {mods}\n"),
        )
        .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn checkbox_changes_persist_and_resolve_dependencies() {
    let f = Fixture::new();
    f.package("a", "");
    f.package("b", "dependencies = ['a']");
    f.profile("[]");
    dlmod_core::set_enabled(&f.0, "test", "b", true).unwrap();
    assert_eq!(
        dlmod_core::profile(&f.0, "test").unwrap().mods,
        vec!["a", "b"]
    );
    assert!(dlmod_core::set_enabled(&f.0, "test", "a", false).is_err());
    dlmod_core::set_enabled(&f.0, "test", "b", false).unwrap();
    dlmod_core::set_enabled(&f.0, "test", "a", false).unwrap();
    assert!(plan(&f.0, "test").unwrap().packages.is_empty());
}

#[test]
fn zip_installs_without_enabling_or_extracting_extra_files() {
    use std::io::Write;
    let f = Fixture::new();
    f.profile("[]");
    let path = f.0.join("mod.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("mod.toml", options).unwrap();
    write!(
        zip,
        "id='a'\nname='Example'\nversion='1'\ngame_build={GAME_BUILD:?}\nentry='main.lua'\n"
    )
    .unwrap();
    zip.start_file("main.lua", options).unwrap();
    zip.write_all(b"return function() end").unwrap();
    zip.start_file("../../escape.txt", options).unwrap();
    zip.write_all(b"ignored").unwrap();
    zip.finish().unwrap();
    assert_eq!(install(&f.0, &path).unwrap(), "a");
    assert_eq!(dlmod_core::installed(&f.0).unwrap().len(), 1);
    assert!(plan(&f.0, "test").unwrap().packages.is_empty());
    assert_eq!(fs::read_dir(f.0.join("mods/a")).unwrap().count(), 2);
    assert!(install(&f.0, &path).is_err());
}

#[test]
fn dependencies_and_conflicts() {
    let f = Fixture::new();
    f.package("a", "");
    f.package("b", "dependencies = ['a']");
    f.profile("['b']");
    assert!(plan(&f.0, "test").unwrap_err().contains("requires a"));
    f.profile("['b','a']");
    assert!(plan(&f.0, "test").is_err());
    f.profile("['a','b']");
    assert!(plan(&f.0, "test").is_ok());
    f.package("c", "conflicts = ['a']");
    f.profile("['a','c']");
    assert!(plan(&f.0, "test").unwrap_err().contains("conflicts"));
}

#[test]
fn report_identifies_content_differences_and_ignores_disabled_mods() {
    let f = Fixture::new();
    f.package("a", "");
    f.profile("['a']");
    let first = dlmod_core::compatibility_report(&f.0, "test").unwrap();
    assert!(first.contains("Matchmaking pool: dlmods-v2-"));
    assert!(first.contains("ID: a | version: 1.0.0"));
    f.package("disabled", "");
    assert_eq!(
        first,
        dlmod_core::compatibility_report(&f.0, "test").unwrap()
    );
    fs::write(f.0.join("mods/a/main.lua"), "-- different bytes").unwrap();
    assert_ne!(
        first,
        dlmod_core::compatibility_report(&f.0, "test").unwrap()
    );
    f.profile("[]");
    let vanilla = dlmod_core::compatibility_report(&f.0, "test").unwrap();
    assert!(vanilla.contains("Mode: vanilla"));
    assert!(!vanilla.contains("Matchmaking pool:"));
}
#[test]
fn duplicate_missing_unknown_and_escaped_entries() {
    let f = Fixture::new();
    f.package("a", "");
    f.profile("['a','a']");
    assert!(plan(&f.0, "test").is_err());
    f.profile("['missing']");
    assert!(plan(&f.0, "test").is_err());
    f.package("a", "unexpected = true");
    f.profile("['a']");
    assert!(plan(&f.0, "test").is_err());
    f.package("a", "");
    let path = f.0.join("mods/a/mod.toml");
    let raw = fs::read_to_string(&path).unwrap();
    fs::write(path, raw.replace("'main.lua'", "'../main.lua'")).unwrap();
    assert!(plan(&f.0, "test").is_err());
}
#[test]
fn hash_tracks_contents_and_order_not_profile_name() {
    let f = Fixture::new();
    f.package("a", "");
    f.package("b", "");
    f.profile("['a','b']");
    let first = plan(&f.0, "test").unwrap().fingerprint;
    fs::write(
        f.0.join("profiles/other.toml"),
        "name = 'Different label'\nmods = ['a','b']",
    )
    .unwrap();
    assert_eq!(first, plan(&f.0, "other").unwrap().fingerprint);
    f.profile("['b','a']");
    assert_ne!(first, plan(&f.0, "test").unwrap().fingerprint);
    f.profile("['a','b']");
    fs::write(
        f.0.join("mods/a/main.lua"),
        "return function() return 'changed' end",
    )
    .unwrap();
    assert_ne!(first, plan(&f.0, "test").unwrap().fingerprint);
}
#[test]
fn installer_copies_package_and_refuses_overwrite() {
    let source = Fixture::new();
    source.package("a", "");
    let dest = Fixture::new();
    let manifest = source.0.join("mods/a/mod.toml");
    assert_eq!(install(&dest.0, &manifest).unwrap(), "a");
    dest.profile("['a']");
    assert!(plan(&dest.0, "test").is_ok());
    assert!(install(&dest.0, &manifest).is_err());
    assert_eq!(
        fs::read(dest.0.join("mods/a/main.lua")).unwrap(),
        fs::read(source.0.join("mods/a/main.lua")).unwrap()
    );
    assert_eq!(fs::read_dir(dest.0.join("mods")).unwrap().count(), 1);
}

#[test]
fn cosmetic_manifest_edits_do_not_split_matchmaking() {
    let f = Fixture::new();
    f.package("a", "");
    f.profile("['a']");
    let before = plan(&f.0, "test").unwrap().fingerprint;
    let path = f.0.join("mods/a/mod.toml");
    let raw = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        format!(
            "# comment\n{}\ndescription='New description'\n",
            raw.replace("name = \"a\"", "name = 'Display label'")
        ),
    )
    .unwrap();
    assert_eq!(before, plan(&f.0, "test").unwrap().fingerprint);
    fs::write(&path, raw.replace("'1.0.0'", "'2.0.0'")).unwrap();
    assert_ne!(before, plan(&f.0, "test").unwrap().fingerprint);
    fs::write(&path, &raw).unwrap();
    fs::write(
        f.0.join("mods/a/main.lua"),
        "-- comment\nreturn function() return 'ok' end",
    )
    .unwrap();
    assert_ne!(before, plan(&f.0, "test").unwrap().fingerprint);
}

#[test]
fn profile_editor_preserves_mods_and_rejects_invalid_names() {
    use dlmod_core::profiles;
    let f = Fixture::new();
    f.package("a", "");
    f.profile("['a']");
    let original = plan(&f.0, "test").unwrap().fingerprint;
    let copy = profiles::create(&f.0, "Friends", Some("test")).unwrap();
    assert_eq!(original, plan(&f.0, &copy).unwrap().fingerprint);
    profiles::rename(&f.0, &copy, "Evening").unwrap();
    assert_eq!(original, plan(&f.0, &copy).unwrap().fingerprint);
    assert!(profiles::create(&f.0, "evening", None).is_err());
    assert!(profiles::rename(&f.0, &copy, "\n").is_err());
    assert!(profiles::create(&f.0, "", None).is_err());
    let empty = profiles::create(&f.0, "Vanilla", None).unwrap();
    assert!(plan(&f.0, &empty).unwrap().packages.is_empty());
    profiles::delete(&f.0, &copy).unwrap();
    profiles::delete(&f.0, &empty).unwrap();
    assert!(profiles::delete(&f.0, "test").is_err());
    assert!(f.0.join("mods/a/main.lua").exists());
    assert_eq!(original, plan(&f.0, "test").unwrap().fingerprint);
}
