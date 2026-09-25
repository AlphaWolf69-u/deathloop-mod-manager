use dlmod_core::{
    plan, pool_key, pool_version,
    win::{error, game_pid, modules, wide, Handle, Process},
    Memory, Result, GAME_BUILD,
};
use std::{
    ffi::c_void,
    path::Path,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    System::{LibraryLoader::*, Memory::*, Threading::*},
};

fn call(process: &Process, address: usize, argument: usize) -> Result<u32> {
    unsafe {
        let thread = Handle(CreateRemoteThread(
            process.handle.0,
            std::ptr::null(),
            0,
            Some(std::mem::transmute::<
                usize,
                unsafe extern "system" fn(*mut c_void) -> u32,
            >(address)),
            argument as *const c_void,
            0,
            std::ptr::null_mut(),
        ));
        if thread.0.is_null() {
            return Err(error("Start loader thread"));
        }
        if WaitForSingleObject(thread.0, 15000) != WAIT_OBJECT_0 {
            return Err("Loader call timed out; restart the game before retrying".into());
        }
        let mut exit = 0;
        if GetExitCodeThread(thread.0, &mut exit) == 0 {
            return Err(error("Read loader result"));
        }
        Ok(exit)
    }
}
fn remote_buffer(process: &Process, s: &str) -> Result<usize> {
    let data = wide(s)
        .iter()
        .flat_map(|u| u.to_le_bytes())
        .collect::<Vec<_>>();
    let address = unsafe {
        VirtualAllocEx(
            process.handle.0,
            std::ptr::null(),
            data.len(),
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    } as usize;
    if address == 0 {
        return Err(error("Allocate loader request"));
    }
    if let Err(e) = process.write(address, &data) {
        unsafe {
            VirtualFreeEx(process.handle.0, address as *mut c_void, 0, MEM_RELEASE);
        };
        return Err(e);
    }
    Ok(address)
}
fn exports(path: &Path) -> Result<(usize, usize)> {
    unsafe {
        let name = wide(&path.to_string_lossy());
        let module = LoadLibraryExW(
            name.as_ptr(),
            std::ptr::null_mut(),
            DONT_RESOLVE_DLL_REFERENCES,
        );
        if module.is_null() {
            return Err(error("Inspect runtime exports"));
        }
        let a = GetProcAddress(module, c"DLModStart".as_ptr().cast())
            .map(|f| f as usize - module as usize);
        let b = GetProcAddress(module, c"DLModStatus".as_ptr().cast())
            .map(|f| f as usize - module as usize);
        FreeLibrary(module);
        Ok((
            a.ok_or("Runtime start export missing")?,
            b.ok_or("Runtime status export missing")?,
        ))
    }
}

pub fn inspect(root: &Path, profile: &str) -> Result<String> {
    let plan = plan(root, profile)?;
    let process = Process::open(game_pid()?, false)?;
    process.preflight_for(Some(&plan.fingerprint))?;
    let (_, key) = process.compatibility()?;
    let version = process.string(process.q(process.base + process.layout()?.version)?, 128)?;
    if key.starts_with("dlmods-v2-") || version.starts_with("dlmods-v2-") {
        return Ok(format!(
            "Modded version marker is installed: {version}.\r\nOpen logs for mod status. Restart Deathloop to switch profiles or return to vanilla."
        ));
    }
    let pool = pool_key(
        &plan.fingerprint,
        if key.is_empty() { GAME_BUILD } else { &key },
    )?;
    Ok(format!(
        "Ready: {} ({} mods).\r\nModded pool: {pool}",
        plan.name,
        plan.packages.len()
    ))
}

pub fn enable(root: &Path, profile: &str) -> Result<String> {
    let plan = plan(root, profile)?;
    if plan.packages.is_empty() {
        return Err("Select a nonempty mod profile".into());
    }
    let process = Process::open(game_pid()?, true)?;
    if process.read(process.base + process.layout()?.protection, 1)? != [0xC3] {
        return Err("Restart using Launch game so mod startup support can disable protection before the game initializes.".into());
    }
    process.preflight_for(Some(&plan.fingerprint))?;
    let (_, key) = process.compatibility()?;
    let expected_pool = pool_key(
        &plan.fingerprint,
        if key.is_empty() { GAME_BUILD } else { &key },
    )?;
    let dll = root
        .join("dlmod_runtime.dll")
        .canonicalize()
        .map_err(|e| format!("Runtime DLL missing: {e}"))?;
    let (start, status) = exports(&dll)?;
    let before = modules(process.pid)?;
    if before
        .iter()
        .any(|m| m.0.eq_ignore_ascii_case("dlmod_runtime.dll"))
    {
        return Err(
            "Loader already present. Restart Deathloop to retry or change profiles.".into(),
        );
    }
    // Resolve the actual containing system module: LoadLibraryW can be forwarded to KernelBase.
    let (system_name, offset) = unsafe {
        let kernel = GetModuleHandleW(wide("kernel32.dll").as_ptr());
        let proc = GetProcAddress(kernel, c"LoadLibraryW".as_ptr().cast())
            .ok_or("LoadLibraryW unavailable")? as usize;
        let mut container = std::ptr::null_mut();
        if GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            proc as *const u16,
            &mut container,
        ) == 0
        {
            return Err(error("Resolve system module"));
        }
        let mut path = vec![0u16; 32768];
        let n = GetModuleFileNameW(container, path.as_mut_ptr(), path.len() as u32);
        let path = String::from_utf16_lossy(&path[..n as usize]);
        (
            Path::new(&path)
                .file_name()
                .ok_or("System module path missing")?
                .to_string_lossy()
                .to_string(),
            proc - container as usize,
        )
    };
    let remote_system = before
        .iter()
        .find(|m| m.0.eq_ignore_ascii_case(&system_name))
        .ok_or("System module not loaded in game")?
        .1;
    let argument = remote_buffer(&process, &dll.to_string_lossy())?;
    let loaded = call(&process, remote_system + offset, argument);
    // On timeout leave the tiny request allocated: its thread may still be reading it.
    if loaded.is_ok() {
        unsafe {
            VirtualFreeEx(process.handle.0, argument as *mut c_void, 0, MEM_RELEASE);
        }
    }
    loaded?;
    let loaded_module = modules(process.pid)?
        .into_iter()
        .find(|m| m.0.eq_ignore_ascii_case("dlmod_runtime.dll"))
        .ok_or("Windows did not load the runtime DLL")?;
    if Path::new(&loaded_module.2)
        .canonicalize()
        .map_err(|e| e.to_string())?
        != dll
    {
        return Err("Different runtime DLL loaded".into());
    }
    let request = format!(
        "{}\n{profile}\n{}",
        root.canonicalize().map_err(|e| e.to_string())?.display(),
        plan.fingerprint
    );
    let argument = remote_buffer(&process, &request)?;
    let result = call(&process, loaded_module.1 + start, argument);
    if result.is_ok() {
        unsafe {
            VirtualFreeEx(process.handle.0, argument as *mut c_void, 0, MEM_RELEASE);
        }
    }
    if result? != 0 {
        return Err(
            "Runtime initialization rejected. See logs; restart Deathloop before retrying.".into(),
        );
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        match call(&process, loaded_module.1 + status, 0)? {
            2 => {
                let key = process.compatibility()?.1;
                if (!key.is_empty() && key != expected_pool)
                    || process.string(process.q(process.base + process.layout()?.version)?, 128)?
                        != pool_version(&plan.fingerprint)?
                {
                    return Err("Pool verification failed".into());
                }
                crate::startup::cancel(root)?;
                return Ok(format!("Launched with profile: {}.", plan.name));
            }
            3 => {
                return Err(
                    "Runtime stopped. Open logs for the reason; restart Deathloop before retrying."
                        .into(),
                )
            }
            _ => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    Err("Runtime has not acknowledged startup; inspect logs".into())
}
