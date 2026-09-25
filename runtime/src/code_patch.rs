//! In-process code installation. Suspended window performs no Rust allocation,
//! logging, Lua calls, or game calls. Installed caves live until process exit.
use dlmod_core::{
    code::{decode_hex, relocate, Relocation},
    win::{Handle, Process},
    Memory, Patches, Result, Write,
};
use mlua::Table;
use std::{
    ffi::c_void,
    mem::{size_of, zeroed},
};
use windows_sys::Win32::{
    Foundation::*,
    System::{
        Diagnostics::{Debug::*, ToolHelp::*},
        Memory::*,
        Threading::*,
    },
};

struct Segment {
    offset: usize,
    bytes: Vec<u8>,
    relocations: Vec<Relocation>,
}
struct Site {
    rva: usize,
    expected: Vec<u8>,
    bytes: Vec<u8>,
    relocations: Vec<Relocation>,
}
pub struct Plan {
    size: usize,
    segments: Vec<Segment>,
    sites: Vec<Site>,
}

fn relocations(row: &Table) -> mlua::Result<Vec<Relocation>> {
    let mut out = Vec::new();
    if let Some(rows) = row.get::<Option<Table>>("rel32")? {
        for r in rows.sequence_values::<Table>() {
            if out.len() >= 4096 {
                return Err(mlua::Error::external("Too many relocations"));
            }
            let r = r?;
            let kind: String = r.get("kind")?;
            if kind != "cave" && kind != "image" {
                return Err(mlua::Error::external(
                    "Relocation kind must be cave or image",
                ));
            }
            out.push(Relocation {
                offset: r.get("offset")?,
                target: r.get("target")?,
                cave: kind == "cave",
            });
        }
    }
    Ok(out)
}
impl Plan {
    pub fn parse(row: Table) -> mlua::Result<Self> {
        let size: usize = row.get("size")?;
        if !(4096..=65536).contains(&size) || !size.is_multiple_of(4096) {
            return Err(mlua::Error::external(
                "Cave size must be 4..64 KiB in whole pages",
            ));
        }
        let mut segments = Vec::new();
        let mut used = std::collections::BTreeSet::new();
        for r in row.get::<Table>("segments")?.sequence_values::<Table>() {
            if segments.len() >= 64 {
                return Err(mlua::Error::external("Too many segments"));
            }
            let r = r?;
            let offset: usize = r.get("offset")?;
            let bytes = decode_hex(&r.get::<String>("bytes")?).map_err(mlua::Error::external)?;
            let end = offset
                .checked_add(bytes.len())
                .ok_or_else(|| mlua::Error::external("Segment overflow"))?;
            if end > size || (offset..end).any(|i| !used.insert(i)) {
                return Err(mlua::Error::external(
                    "Overlapping/out-of-bounds cave segment",
                ));
            }
            segments.push(Segment {
                offset,
                bytes,
                relocations: relocations(&r)?,
            });
        }
        let mut sites = Vec::new();
        for r in row.get::<Table>("sites")?.sequence_values::<Table>() {
            if sites.len() >= 64 {
                return Err(mlua::Error::external("Too many code sites"));
            }
            let r = r?;
            let expected =
                decode_hex(&r.get::<String>("expected")?).map_err(mlua::Error::external)?;
            let bytes = decode_hex(&r.get::<String>("bytes")?).map_err(mlua::Error::external)?;
            if bytes.len() != expected.len() || bytes.len() > 256 {
                return Err(mlua::Error::external(
                    "Code site must replace 1..256 bytes at equal length",
                ));
            }
            sites.push(Site {
                rva: r.get("rva")?,
                expected,
                bytes,
                relocations: relocations(&r)?,
            });
        }
        if sites.is_empty() {
            return Err(mlua::Error::external("No code sites"));
        }
        Ok(Self {
            size,
            segments,
            sites,
        })
    }
}

struct Cave {
    address: usize,
    keep: bool,
}
impl Drop for Cave {
    fn drop(&mut self) {
        if !self.keep {
            unsafe {
                VirtualFree(self.address as _, 0, MEM_RELEASE);
            }
        }
    }
}
fn near(base: usize, size: usize) -> Result<Cave> {
    for distance in (0x10000..0x70000000usize).step_by(0x10000) {
        for candidate in [base.checked_sub(distance), base.checked_add(distance)]
            .into_iter()
            .flatten()
        {
            if candidate < 0x10000 {
                continue;
            }
            let p = unsafe {
                VirtualAlloc(
                    candidate as _,
                    size,
                    MEM_RESERVE | MEM_COMMIT,
                    PAGE_EXECUTE_READWRITE,
                )
            };
            if !p.is_null() {
                return Ok(Cave {
                    address: p as usize,
                    keep: false,
                });
            }
        }
    }
    Err("Cannot allocate nearby code cave".into())
}

// Handle storage is allocated before any thread is suspended.
fn threads() -> Result<Vec<Handle>> {
    unsafe {
        let snapshot = Handle(CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0));
        if snapshot.0 == INVALID_HANDLE_VALUE {
            return Err("Thread snapshot failed".into());
        }
        let mut entry: THREADENTRY32 = zeroed();
        entry.dwSize = size_of::<THREADENTRY32>() as u32;
        let mut result = Vec::new();
        let mut ok = Thread32First(snapshot.0, &mut entry);
        while ok != 0 {
            if entry.th32OwnerProcessID == GetCurrentProcessId()
                && entry.th32ThreadID != GetCurrentThreadId()
            {
                let h = OpenThread(
                    THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_LIMITED_INFORMATION,
                    0,
                    entry.th32ThreadID,
                );
                if h.is_null() {
                    // A thread can finish between the snapshot and OpenThread.
                    // Windows reports ERROR_INVALID_PARAMETER for a vanished ID.
                    if GetLastError() == ERROR_INVALID_PARAMETER {
                        ok = Thread32Next(snapshot.0, &mut entry);
                        continue;
                    }
                    return Err("Cannot open a game thread; retry code installation".into());
                }
                result.push(Handle(h));
            }
            ok = Thread32Next(snapshot.0, &mut entry);
        }
        Ok(result)
    }
}

/// All buffers and page protections prepared before this function. No heap use while paused.
#[repr(C, align(16))]
struct AlignedContext(CONTEXT);

unsafe fn commit(writes: &[Write], handles: &[Handle], win32_error: &mut u32) -> u32 {
    let mut suspended = 0;
    let mut error = 0;
    for handle in handles {
        if SuspendThread(handle.0) == u32::MAX {
            *win32_error = GetLastError();
            error = 1;
            break;
        }
        suspended += 1;
        // windows-sys gives CONTEXT only 8-byte Rust alignment on x64.
        // The Windows context API requires a 16-byte-aligned buffer.
        let mut context: AlignedContext = zeroed();
        context.0.ContextFlags = CONTEXT_CONTROL_AMD64;
        if GetThreadContext(handle.0, &mut context.0) == 0 {
            *win32_error = GetLastError();
            error = 2;
            break;
        }
        if writes
            .iter()
            .any(|w| (w.address..w.address + w.value.len()).contains(&(context.0.Rip as usize)))
        {
            error = 3;
            break;
        }
    }
    if error == 0 {
        for w in writes {
            if std::slice::from_raw_parts(w.address as *const u8, w.expected.len()) != w.expected {
                error = 4;
                break;
            }
        }
    }
    if error == 0 {
        for w in writes {
            std::ptr::copy_nonoverlapping(w.value.as_ptr(), w.address as *mut u8, w.value.len());
        }
        for w in writes {
            if FlushInstructionCache(
                GetCurrentProcess(),
                w.address as *const c_void,
                w.value.len(),
            ) == 0
            {
                error = 5;
            }
        }
        if error != 0 {
            for w in writes {
                std::ptr::copy_nonoverlapping(
                    w.expected.as_ptr(),
                    w.address as *mut u8,
                    w.expected.len(),
                );
                FlushInstructionCache(
                    GetCurrentProcess(),
                    w.address as *const c_void,
                    w.expected.len(),
                );
            }
        }
    }
    for h in handles[..suspended].iter().rev() {
        if ResumeThread(h.0) == u32::MAX {
            error = 6;
        }
    }
    error
}

pub fn install(memory: &Process, patches: &mut Patches, owner: &str, plan: Plan) -> Result<usize> {
    let base = memory.base;
    let pe = memory.u32(base + 0x3c)? as usize;
    let image_size = memory.u32(base + pe + 24 + 56)? as usize;
    let mut cave = near(base, plan.size)?;
    for mut s in plan.segments {
        relocate(
            &mut s.bytes,
            cave.address + s.offset,
            base,
            cave.address,
            plan.size,
            &s.relocations,
        )?;
        memory.write(cave.address + s.offset, &s.bytes)?;
    }
    let mut writes = Vec::new();
    for mut s in plan.sites {
        if s.rva
            .checked_add(s.bytes.len())
            .is_none_or(|end| end > image_size)
        {
            return Err("Code site outside game image".into());
        }
        let address = base + s.rva;
        let mut info: MEMORY_BASIC_INFORMATION = unsafe { zeroed() };
        if unsafe {
            VirtualQuery(
                address as _,
                &mut info,
                size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        } == 0
            || info.State != MEM_COMMIT
            || info.Protect
                & (PAGE_EXECUTE
                    | PAGE_EXECUTE_READ
                    | PAGE_EXECUTE_READWRITE
                    | PAGE_EXECUTE_WRITECOPY)
                == 0
            || address + s.bytes.len() > info.BaseAddress as usize + info.RegionSize
        {
            return Err("Code site must lie inside one committed executable region".into());
        }
        relocate(
            &mut s.bytes,
            address,
            base,
            cave.address,
            plan.size,
            &s.relocations,
        )?;
        writes.push(Write {
            address,
            expected: s.expected,
            value: s.bytes,
        });
    }
    patches.validate(memory, owner, &writes)?;
    let handles = threads()?;
    let mut pages = std::collections::BTreeMap::new();
    for w in &writes {
        for page in (w.address & !4095..=(w.address + w.value.len() - 1) & !4095).step_by(4096) {
            pages.insert(page, 0u32);
        }
    }
    let mut protection_ok = true;
    for (page, old) in &mut pages {
        if unsafe { VirtualProtect(*page as _, 4096, PAGE_EXECUTE_READWRITE, old) } == 0 {
            protection_ok = false;
            break;
        }
    }
    let mut win32_error = 0;
    let code = if protection_ok {
        if unsafe { FlushInstructionCache(GetCurrentProcess(), cave.address as _, plan.size) } == 0
        {
            7
        } else {
            unsafe { commit(&writes, &handles, &mut win32_error) }
        }
    } else {
        8
    };
    let mut restored = true;
    for (page, old) in &pages {
        if *old != 0 {
            let mut unused = 0;
            if unsafe { VirtualProtect(*page as _, 4096, *old, &mut unused) } == 0 {
                restored = false;
            }
        }
    }
    // Resume failures may occur after publication: never free a possibly referenced cave.
    if code == 0 || code == 6 {
        cave.keep = true;
        patches.claim(owner, &writes);
    }
    if code != 0 || !restored {
        return Err(format!(
            "Code installation status={code}, Win32={win32_error}, protections restored={restored}; {}",
            if cave.keep {
                "restart game before retrying"
            } else {
                "no code sites published; retry"
            }
        ));
    }
    Ok(cave.address)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_context_buffer_has_required_alignment() {
        assert_eq!(std::mem::align_of::<AlignedContext>(), 16);
        let context: AlignedContext = unsafe { zeroed() };
        assert_eq!((&context.0 as *const CONTEXT as usize) & 15, 0);
        assert_eq!(size_of::<AlignedContext>(), size_of::<CONTEXT>());
    }
    #[test]
    fn parses_hook_and_rejects_bad_plans() {
        let lua = mlua::Lua::new();
        let good = "{size=4096,segments={{offset=0,bytes='B8 02 00 00 00 C3'}},sites={{rva=16,expected='B8 01 00 00 00 C3',bytes='E9 00 00 00 00 90',rel32={{offset=1,kind='cave',target=0}}}}}";
        assert!(Plan::parse(lua.load(format!("return {good}")).eval().unwrap()).is_ok());
        for bad in [
            good.replace("size=4096", "size=1"),
            good.replace("offset=0,bytes", "offset=4095,bytes"),
            good.replace("kind='cave'", "kind='unknown'"),
            good.replace("expected='B8 01 00 00 00 C3'", "expected='90'"),
        ] {
            assert!(Plan::parse(lua.load(format!("return {bad}")).eval().unwrap()).is_err());
        }
    }
    #[test]
    fn native_transaction_changes_code_and_refuses_stale_bytes() {
        unsafe {
            let p = VirtualAlloc(
                std::ptr::null(),
                4096,
                MEM_RESERVE | MEM_COMMIT,
                PAGE_EXECUTE_READWRITE,
            );
            assert!(!p.is_null());
            let old = vec![0xb8, 1, 0, 0, 0, 0xc3];
            std::ptr::copy_nonoverlapping(old.as_ptr(), p as *mut u8, old.len());
            FlushInstructionCache(GetCurrentProcess(), p, old.len());
            let f: unsafe extern "C" fn() -> u32 = std::mem::transmute(p);
            assert_eq!(f(), 1);
            let w = Write {
                address: p as usize,
                expected: old,
                value: vec![0xb8, 2, 0, 0, 0, 0xc3],
            };
            let handles = threads().unwrap();
            assert_eq!(commit(std::slice::from_ref(&w), &handles, &mut 0), 0);
            assert_eq!(f(), 2);
            assert_eq!(commit(std::slice::from_ref(&w), &handles, &mut 0), 4);
            assert_eq!(f(), 2);
            assert_ne!(VirtualFree(p, 0, MEM_RELEASE), 0);
        }
    }
    #[test]
    fn complete_install_relocates_executes_restores_protection_and_claims_sites() {
        unsafe {
            let image = VirtualAlloc(
                std::ptr::null(),
                4096,
                MEM_RESERVE | MEM_COMMIT,
                PAGE_EXECUTE_READWRITE,
            );
            assert!(!image.is_null());
            let base = image as usize;
            std::ptr::write_unaligned((base + 0x3c) as *mut u32, 0x80);
            std::ptr::write_unaligned((base + 0x80 + 24 + 56) as *mut u32, 4096);
            let original: [u8; 6] = [0xb8, 1, 0, 0, 0, 0xc3];
            std::ptr::copy_nonoverlapping(original.as_ptr(), (base + 0x100) as _, 6);
            let mut old = 0;
            assert_ne!(VirtualProtect(image, 4096, PAGE_EXECUTE_READ, &mut old), 0);
            let handle = Handle(OpenProcess(
                PROCESS_QUERY_INFORMATION
                    | PROCESS_VM_READ
                    | PROCESS_VM_WRITE
                    | PROCESS_VM_OPERATION,
                0,
                GetCurrentProcessId(),
            ));
            assert!(!handle.0.is_null());
            let process = Process {
                handle,
                pid: GetCurrentProcessId(),
                base,
            };
            let plan = Plan::parse(mlua::Lua::new().load("return {size=4096,segments={{offset=0,bytes='B8 2A 00 00 00 C3'}},sites={{rva=256,expected='B8 01 00 00 00 C3',bytes='E9 00 00 00 00 90',rel32={{offset=1,kind='cave',target=0}}}}}").eval().unwrap()).unwrap();
            let mut patches = Patches::default();
            let cave = install(&process, &mut patches, "test.code", plan).unwrap();
            let f: unsafe extern "C" fn() -> u32 = std::mem::transmute(base + 0x100);
            assert_eq!(f(), 42);
            let mut info: MEMORY_BASIC_INFORMATION = zeroed();
            VirtualQuery(image, &mut info, size_of::<MEMORY_BASIC_INFORMATION>());
            assert_eq!(info.Protect, PAGE_EXECUTE_READ);
            let current = process.read(base + 0x100, 6).unwrap();
            assert!(patches
                .validate(
                    &process,
                    "another-mod",
                    &[Write {
                        address: base + 0x100,
                        expected: current.clone(),
                        value: current
                    }]
                )
                .is_err());
            VirtualFree(image, 0, MEM_RELEASE);
            VirtualFree(cave as _, 0, MEM_RELEASE);
        }
    }
    #[test]
    fn lua_code_mod_installs_only_once() {
        let lua = mlua::Lua::new();
        let api = lua.create_table().unwrap();
        api.set("api_version", 2).unwrap();
        api.set("build_layout", "steam-1.820.5.1").unwrap();
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let c = calls.clone();
        api.set(
            "install_code",
            lua.create_function(move |_, t: Table| {
                let p = Plan::parse(t)?;
                assert_eq!(p.sites.len(), 1);
                c.set(c.get() + 1);
                Ok(0x1000000)
            })
            .unwrap(),
        )
        .unwrap();
        api.set(
            "read_u32",
            lua.create_function(|_, _: usize| Ok(0)).unwrap(),
        )
        .unwrap();
        lua.globals().set("game", api).unwrap();
        let tick: mlua::Function = lua
            .load(
                r#"
            assert(game.api_version >= 2)
            local cave
            return function()
                if not cave then
                    cave = game.install_code({size=4096,
                        segments={{offset=0,bytes='B8 2A 00 00 00 C3'}},
                        sites={{rva=256,expected='B8 01 00 00 00 C3',
                            bytes='E9 00 00 00 00 90',
                            rel32={{offset=1,kind='cave',target=0}}}}})
                end
                return 'Installed'
            end
        "#,
            )
            .eval()
            .unwrap();
        tick.call::<String>(()).unwrap();
        tick.call::<String>(()).unwrap();
        assert_eq!(calls.get(), 1);
    }
}
