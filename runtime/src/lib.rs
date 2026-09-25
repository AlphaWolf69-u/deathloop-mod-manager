use dlmod_core::{
    plan, pool_key, pool_version, win::Process, Memory, Patches, Result, Write, GAME_BUILD,
};
use mlua::{Function, HookTriggers, Lua, LuaOptions, StdLib, Table, VmState};
use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    fs::{self, OpenOptions},
    io::Write as IoWrite,
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

static STARTED: AtomicBool = AtomicBool::new(false);
mod code_patch;
static STATUS: AtomicU32 = AtomicU32::new(0);
static VERSION: std::sync::OnceLock<std::ffi::CString> = std::sync::OnceLock::new();
fn log(root: &Path, message: &str) {
    let directory = root.join("logs");
    let _ = fs::create_dir_all(&directory);
    let path = directory.join(format!("runtime-{}.log", unsafe { GetCurrentProcessId() }));
    // Bound each process log; retain one previous segment.
    if fs::metadata(&path)
        .map(|m| m.len() > 2 * 1024 * 1024)
        .unwrap_or(false)
    {
        let _ = fs::rename(&path, path.with_extension("previous.log"));
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let _ = writeln!(file, "{time} | {message}");
    }
}

fn script(
    memory: Rc<Process>,
    patches: Rc<RefCell<Patches>>,
    enabled: Rc<Cell<bool>>,
    owner: String,
    source: &str,
) -> Result<(Lua, Function)> {
    let lua = Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::MATH,
        LuaOptions::default(),
    )
    .map_err(|e| e.to_string())?;
    lua.set_memory_limit(16 * 1024 * 1024)
        .map_err(|e| e.to_string())?;
    // An accidental infinite Lua loop should stop this callback, not spin forever.
    let count = Rc::new(RefCell::new(0usize));
    let c = count.clone();
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(10000),
        move |_, _| {
            *c.borrow_mut() += 10000;
            if *c.borrow() > 2_000_000 {
                Err(mlua::Error::RuntimeError(
                    "Lua instruction budget exceeded".into(),
                ))
            } else {
                Ok(VmState::Continue)
            }
        },
    )
    .map_err(|e| e.to_string())?;
    lua.set_app_data(count);
    let api = lua.create_table().map_err(|e| e.to_string())?;
    api.set("api_version", 2).map_err(|e| e.to_string())?;
    api.set("base", memory.base).map_err(|e| e.to_string())?;
    let layout = memory.layout()?;
    api.set("build_layout", layout.name)
        .map_err(|e| e.to_string())?;
    api.set("campaign_registry", layout.campaign_registry)
        .map_err(|e| e.to_string())?;
    api.set("map_registry", layout.map_registry)
        .map_err(|e| e.to_string())?;
    api.set("world_pointer", layout.world)
        .map_err(|e| e.to_string())?;
    let m = memory.clone();
    api.set(
        "read_u64",
        lua.create_function(move |_, a: usize| m.q(a).map_err(mlua::Error::external))
            .unwrap(),
    )
    .unwrap();
    let m = memory.clone();
    api.set(
        "read_u32",
        lua.create_function(move |_, a: usize| m.u32(a).map_err(mlua::Error::external))
            .unwrap(),
    )
    .unwrap();
    let m = memory.clone();
    api.set(
        "read_u8",
        lua.create_function(move |_, a: usize| {
            m.read(a, 1).map(|b| b[0]).map_err(mlua::Error::external)
        })
        .unwrap(),
    )
    .unwrap();
    let m = memory.clone();
    api.set(
        "read_string",
        lua.create_function(move |_, (a, n): (usize, usize)| {
            m.string(a, n).map_err(mlua::Error::external)
        })
        .unwrap(),
    )
    .unwrap();
    let code_memory = memory.clone();
    let code_patches = patches.clone();
    let code_enabled = enabled.clone();
    let code_owner = owner.clone();
    let installed = Rc::new(Cell::new(false));
    api.set(
        "install_code",
        lua.create_function(move |_, plan: Table| {
            if !code_enabled.get() {
                return Err(mlua::Error::external(
                    "Code installation unavailable during initialization",
                ));
            }
            if installed.get() {
                return Err(mlua::Error::external(
                    "This mod already installed code; restart to change it",
                ));
            }
            let plan = code_patch::Plan::parse(plan)?;
            let address = code_patch::install(
                &code_memory,
                &mut code_patches.borrow_mut(),
                &code_owner,
                plan,
            )
            .map_err(mlua::Error::external)?;
            installed.set(true);
            Ok(address)
        })
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    api.set(
        "write_flags",
        lua.create_function(move |_, rows: Table| {
            if !enabled.get() {
                return Err(mlua::Error::external(
                    "Writes unavailable during initialization",
                ));
            }
            let mut writes = Vec::new();
            for row in rows.sequence_values::<Table>() {
                if writes.len() >= 4096 {
                    return Err(mlua::Error::external("Too many writes"));
                }
                let row = row?;
                let address: usize = row.get("address")?;
                let expected: u8 = row.get("expected")?;
                let value: u8 = row.get("value")?;
                if value > 1 || expected > 1 {
                    return Err(mlua::Error::external(
                        "write_flags accepts only Boolean bytes",
                    ));
                }
                writes.push(Write {
                    address,
                    expected: vec![expected],
                    value: vec![value],
                });
            }
            patches
                .borrow_mut()
                .apply(memory.as_ref(), &owner, &writes)
                .map_err(mlua::Error::external)
        })
        .unwrap(),
    )
    .unwrap();
    lua.globals().set("game", api).map_err(|e| e.to_string())?;
    let tick: Function = lua
        .load(source)
        .set_name("mod/main.lua")
        .eval()
        .map_err(|e| e.to_string())?;
    Ok((lua, tick))
}

fn run(root: &Path, profile: &str, fingerprint: &str) -> Result<()> {
    let p = plan(root, profile)?;
    if p.fingerprint != fingerprint {
        return Err("Profile changed since launcher validation".into());
    }
    if p.packages.is_empty() {
        return Err("Empty mod profile: use a normal vanilla launch instead".into());
    }
    let memory = Rc::new(Process::open(unsafe { GetCurrentProcessId() }, true)?);
    let layout = memory.layout()?;
    memory.preflight_for(Some(&p.fingerprint))?;
    let (address, original) = memory.compatibility()?;
    let key = pool_key(
        &p.fingerprint,
        if original.is_empty() {
            GAME_BUILD
        } else {
            &original
        },
    )?;
    let patches = Rc::new(RefCell::new(Patches::default()));
    let enabled = Rc::new(Cell::new(false));
    // Compile every mod before making any game changes. Script initialization has read-only API below.
    let mut scripts = Vec::new();
    for package in &p.packages {
        scripts.push((
            package.manifest.id.clone(),
            script(
                memory.clone(),
                patches.clone(),
                enabled.clone(),
                package.manifest.id.clone(),
                &package.script,
            )?,
            String::new(),
        ));
    }
    memory.idle()?;
    let source = VERSION
        .get_or_init(|| std::ffi::CString::new(pool_version(&p.fingerprint).unwrap()).unwrap());
    let mut namespace = vec![
        Write {
            address: memory.base + layout.version,
            expected: memory.read(memory.base + layout.version, 8)?,
            value: (source.as_ptr() as u64).to_le_bytes().to_vec(),
        },
        Write {
            address: memory.base + layout.suffix,
            expected: memory.read(memory.base + layout.suffix, 8)?,
            value: ((memory.base + layout.empty) as u64).to_le_bytes().to_vec(),
        },
    ];
    if !original.is_empty() {
        let mut value = key.as_bytes().to_vec();
        value.resize(original.len() + 1, 0);
        namespace.push(Write {
            address,
            expected: memory.read(address, original.len() + 1)?,
            value,
        });
        let length = memory.u32(memory.base + layout.compatibility + 8)?;
        namespace.push(Write {
            address: memory.base + layout.compatibility + 8,
            expected: length.to_le_bytes().to_vec(),
            value: ((length & 0xE0000000) | key.len() as u32)
                .to_le_bytes()
                .to_vec(),
        });
    }
    memory.preflight_for(Some(&p.fingerprint))?;
    patches
        .borrow_mut()
        .apply(memory.as_ref(), "loader.matchmaking", &namespace)?;
    log(
        root,
        &format!(
            "ACTIVE profile={} fingerprint={} pool={key}; restart game to return to vanilla",
            p.name, p.fingerprint
        ),
    );
    enabled.set(true);
    STATUS.store(2, Ordering::SeqCst);
    loop {
        let start = Instant::now();
        let (_, now_key) = memory.compatibility()?;
        if (!now_key.is_empty() && now_key != key)
            || memory.q(memory.base + layout.version)? != source.as_ptr() as usize
            || memory.q(memory.base + layout.suffix)? != memory.base + layout.empty
        {
            return Err(
                "Compatibility key changed externally. Mod callbacks stopped; restart Deathloop."
                    .into(),
            );
        }
        for (id, (lua, tick), previous) in &mut scripts {
            if let Some(c) = lua.app_data_ref::<Rc<RefCell<usize>>>() {
                *c.borrow_mut() = 0;
            }
            let status = match tick.call::<String>(()) {
                Ok(s) => s,
                Err(e) => format!("WAIT/ERROR: {e}"),
            };
            if *previous != status {
                log(root, &format!("{id}: {status}"));
                *previous = status;
            }
        }
        std::thread::sleep(Duration::from_secs(1).saturating_sub(start.elapsed()));
    }
}

/// Explicit initialization, never called from DllMain / under the Windows loader lock.
///
/// # Safety
/// Caller supplies readable, NUL-terminated UTF-16 containing root, profile ID and
/// fingerprint separated by newlines. Caller retains that buffer until this call returns.
#[no_mangle]
pub unsafe extern "system" fn DLModStart(argument: *mut c_void) -> u32 {
    if argument.is_null() {
        return 1;
    }
    if STARTED.swap(true, Ordering::SeqCst) {
        return 2;
    }
    STATUS.store(1, Ordering::SeqCst);
    let outcome = std::panic::catch_unwind(|| {
        let memory = Process::open(GetCurrentProcessId(), false)?;
        let mut units = Vec::new();
        for i in 0..32768 {
            let b = memory.read(argument as usize + i * 2, 2)?;
            let u = u16::from_le_bytes([b[0], b[1]]);
            if u == 0 {
                break;
            }
            units.push(u);
        }
        let request = String::from_utf16(&units).map_err(|e| e.to_string())?;
        let fields = request.split('\n').collect::<Vec<_>>();
        if fields.len() != 3 {
            return Err("Invalid initialization request".into());
        }
        let root = PathBuf::from(fields[0]);
        let profile = fields[1].to_owned();
        let fingerprint = fields[2].to_owned();
        // Validate request synchronously; game loop remains on a dedicated worker.
        plan(&root, &profile)?;
        std::thread::Builder::new()
            .name("Deathloop mods".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(|| run(&root, &profile, &fingerprint));
                STATUS.store(3, Ordering::SeqCst);
                match result {
                    Ok(Err(e)) => log(&root, &format!("STOPPED: {e}")),
                    Err(_) => log(&root, "STOPPED: Rust panic"),
                    _ => {}
                }
            })
            .map_err(|e| e.to_string())?;
        Ok::<_, String>(())
    });
    match outcome {
        Ok(Ok(())) => 0,
        _ => {
            STARTED.store(false, Ordering::SeqCst);
            STATUS.store(3, Ordering::SeqCst);
            3
        }
    }
}

#[no_mangle]
pub extern "system" fn DLModStatus(_: *mut c_void) -> u32 {
    STATUS.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mod_script_compiles_in_real_lua() {
        Lua::new()
            .load("return function() return 'ok' end")
            .into_function()
            .unwrap();
    }
    #[test]
    fn lua_environment_has_no_shell_or_module_loader() {
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH,
            LuaOptions::default(),
        )
        .unwrap();
        lua.load("assert(os == nil and io == nil and package == nil and require == nil)")
            .exec()
            .unwrap();
    }
}
