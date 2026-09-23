//! Startup-only patches. No allocations, worker threads or LoadLibrary calls in DllMain.
#[allow(dead_code)]
#[path = "../../core/src/layout.rs"]
mod layout;
use std::{
    ffi::c_void,
    sync::{
        atomic::{AtomicU32, Ordering},
        OnceLock,
    },
};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::*,
    System::{
        Diagnostics::Debug::*, LibraryLoader::*, Memory::*, SystemInformation::*, Threading::*,
    },
};

static STATUS: AtomicU32 = AtomicU32::new(0);
// Last completed startup checkpoint. Written with Win32 only, under loader lock.
static STAGE: AtomicU32 = AtomicU32::new(0);
unsafe fn diagnostic(ok: bool) {
    let mut filename = [0u16; 32768];
    let suffix = [
        100, 108, 109, 111, 100, 45, 115, 116, 97, 114, 116, 117, 112, 46, 108, 111, 103,
    ];
    if !path(&suffix, &mut filename) {
        return;
    }
    let file = CreateFileW(
        filename.as_ptr(),
        FILE_APPEND_DATA,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        std::ptr::null(),
        OPEN_ALWAYS,
        FILE_ATTRIBUTE_NORMAL,
        std::ptr::null_mut(),
    );
    if file == INVALID_HANDLE_VALUE {
        return;
    }
    let stage = STAGE.load(Ordering::Relaxed);
    let mut message = *b"Startup stage=00 result=FAIL pid=00000000\r\n";
    message[14] = b'0' + (stage / 10) as u8;
    message[15] = b'0' + (stage % 10) as u8;
    if ok {
        message[24..28].copy_from_slice(b"OK  ");
    }
    let pid = GetCurrentProcessId();
    for i in 0..8 {
        let nibble = ((pid >> ((7 - i) * 4)) & 0xf) as u8;
        message[33 + i] = b"0123456789ABCDEF"[nibble as usize];
    }
    let mut written = 0;
    WriteFile(
        file,
        message.as_ptr(),
        message.len() as u32,
        &mut written,
        std::ptr::null_mut(),
    );
    CloseHandle(file);
}
static mut VERSION: [u8; 64] = [0; 64];
const ORIGINAL_VERSION: &[u8] = b"VoidEngine v1.820.5.1 Content v551\0";
unsafe extern "C" fn skip_initializer() -> usize {
    0
}
unsafe fn read(a: usize, b: &mut [u8]) -> bool {
    let mut got = 0;
    ReadProcessMemory(
        GetCurrentProcess(),
        a as *const c_void,
        b.as_mut_ptr().cast(),
        b.len(),
        &mut got,
    ) != 0
        && got == b.len()
}
unsafe fn matches(a: usize, b: &[u8]) -> bool {
    if b.len() > 128 {
        return false;
    }
    let mut out = [0u8; 128];
    read(a, &mut out[..b.len()]) && &out[..b.len()] == b
}
unsafe fn q(a: usize) -> Option<usize> {
    let mut b = [0u8; 8];
    if read(a, &mut b) {
        Some(u64::from_le_bytes(b) as usize)
    } else {
        None
    }
}
unsafe fn replace(a: usize, b: &[u8]) -> bool {
    let mut old = 0;
    if VirtualProtect(
        a as *const c_void,
        b.len(),
        PAGE_EXECUTE_READWRITE,
        &mut old,
    ) == 0
    {
        return false;
    }
    std::ptr::copy_nonoverlapping(b.as_ptr(), a as *mut u8, b.len());
    FlushInstructionCache(GetCurrentProcess(), a as *const c_void, b.len());
    let mut discarded = 0;
    VirtualProtect(a as *const c_void, b.len(), old, &mut discarded) != 0
}
unsafe fn path(suffix: &[u16], out: &mut [u16; 32768]) -> bool {
    let n = GetModuleFileNameW(std::ptr::null_mut(), out.as_mut_ptr(), out.len() as u32) as usize;
    if n == 0 || n >= out.len() {
        return false;
    }
    let Some(pos) = out[..n].iter().rposition(|v| *v == b'\\' as u16) else {
        return false;
    };
    if pos + 1 + suffix.len() + 1 > out.len() {
        return false;
    }
    out[pos + 1..pos + 1 + suffix.len()].copy_from_slice(suffix);
    out[pos + 1 + suffix.len()] = 0;
    true
}
unsafe fn initialize() -> bool {
    STAGE.store(1, Ordering::Relaxed);
    let mut filename = [0u16; 32768];
    let marker: [u16; 12] = [100, 108, 109, 111, 100, 46, 108, 97, 117, 110, 99, 104]; // dlmod.launch
    if !path(&marker, &mut filename) {
        return false;
    }
    let file = CreateFileW(
        filename.as_ptr(),
        GENERIC_READ,
        FILE_SHARE_READ,
        std::ptr::null(),
        OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL,
        std::ptr::null_mut(),
    );
    if file == INVALID_HANDLE_VALUE {
        return GetLastError() == ERROR_FILE_NOT_FOUND;
    }
    let mut data = [0u8; 80];
    STAGE.store(2, Ordering::Relaxed);
    let mut size = 0i64;
    let mut got = 0;
    let valid = GetFileSizeEx(file, &mut size) != 0
        && size == 80
        && ReadFile(
            file,
            data.as_mut_ptr().cast(),
            80,
            &mut got,
            std::ptr::null_mut(),
        ) != 0
        && got == 80;
    CloseHandle(file);
    if !valid || &data[..8] != b"DLMOD02\0" {
        return false;
    }
    let mut clock: FILETIME = std::mem::zeroed();
    STAGE.store(3, Ordering::Relaxed);
    GetSystemTimeAsFileTime(&mut clock);
    let now = (((clock.dwHighDateTime as u64) << 32) | clock.dwLowDateTime as u64) / 10_000_000
        - 11_644_473_600;
    let expiry = u64::from_le_bytes(data[72..80].try_into().unwrap());
    if expiry < now || expiry > now + 600 {
        return false;
    }
    let version = &data[8..72];
    STAGE.store(4, Ordering::Relaxed);
    let Some(n) = version.iter().position(|v| *v == 0) else {
        return false;
    };
    if n != ORIGINAL_VERSION.len() - 1
        || !version.starts_with(b"dlmods-v2-")
        || !version[..n].iter().all(u8::is_ascii)
    {
        return false;
    }
    let base = GetModuleHandleW(std::ptr::null()) as usize;
    STAGE.store(5, Ordering::Relaxed);
    let mut head = [0u8; 4096];
    if !read(base, &mut head) || &head[..2] != b"MZ" {
        return false;
    }
    let pe = u32::from_le_bytes(head[60..64].try_into().unwrap()) as usize;
    if pe + 84 > head.len() || &head[pe..pe + 6] != b"PE\0\0\x64\x86" {
        return false;
    }
    let Some(layout) = layout::identify(
        u32::from_le_bytes(head[pe + 8..pe + 12].try_into().unwrap()),
        u32::from_le_bytes(head[pe + 80..pe + 84].try_into().unwrap()),
    ) else {
        return false;
    };
    let Some(original) = q(base + layout.version) else {
        return false;
    };
    let Some(suffix) = q(base + layout.suffix) else {
        return false;
    };
    if !matches(original, ORIGINAL_VERSION)
        || !(matches(suffix, b"\0") || matches(suffix, b"_deluxe\0"))
        || !matches(base + layout.empty, b"\0")
        || !matches(
            base + layout.protection,
            &[0x4c, 0x8b, 0xdc, 0x48, 0x81, 0xec, 0x88, 0, 0, 0],
        )
    {
        return false;
    }
    let targets = layout.initializers;
    STAGE.store(6, Ordering::Relaxed);
    for (slot, expected) in targets {
        if q(base + slot) != Some(base + expected) {
            return false;
        }
    }
    // Steam may hand the launch to a second process. Keep the short-lived request
    // until the manager has activated the final game process, then remove it there.
    STAGE.store(7, Ordering::Relaxed);
    std::ptr::copy_nonoverlapping(
        version.as_ptr(),
        std::ptr::addr_of_mut!(VERSION).cast::<u8>(),
        64,
    );
    let replacement = skip_initializer as *const () as usize;
    STAGE.store(8, Ordering::Relaxed);
    for (slot, _) in targets {
        if !replace(base + slot, &replacement.to_le_bytes()) {
            return false;
        }
    }
    if !replace(base + layout.protection, &[0xC3])
        || !replace(base + layout.suffix, &(base + layout.empty).to_le_bytes())
        || !replace(
            base + layout.version,
            &(std::ptr::addr_of!(VERSION) as usize).to_le_bytes(),
        )
    {
        return false;
    }
    STATUS.store(1, Ordering::Release);
    STAGE.store(9, Ordering::Relaxed);
    true
}

/// # Safety
/// Called by Windows with the DLL entry-point ABI during process initialization.
#[no_mangle]
pub unsafe extern "system" fn DllMain(_: HINSTANCE, reason: u32, _: *mut c_void) -> i32 {
    if reason == 1 {
        let ok = initialize();
        diagnostic(ok);
        if !ok {
            STATUS.store(2, Ordering::Release);
            return 0;
        }
    }
    1
}

/// # Safety
/// The arguments must follow the DirectInput8Create API contract.
#[no_mangle]
pub unsafe extern "system" fn DirectInput8Create(
    instance: HINSTANCE,
    version: u32,
    iid: *const c_void,
    result: *mut *mut c_void,
    outer: *mut c_void,
) -> i32 {
    thread_local! { static FORWARDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
    if FORWARDING.with(|busy| busy.replace(true)) {
        return 0x80004005u32 as i32;
    }
    struct ForwardGuard;
    impl Drop for ForwardGuard {
        fn drop(&mut self) {
            FORWARDING.with(|busy| busy.set(false));
        }
    }
    let _guard = ForwardGuard;
    static REAL: OnceLock<usize> = OnceLock::new();
    let address = *REAL.get_or_init(|| {
        // EOS hooks both dinput8 exports when we lazily load System32's DLL.
        // Its single stored original then points back here, forming a loop.
        // The manager copies the local Windows DLL under a private basename.
        let mut filename = [0u16; 32768];
        let suffix: Vec<u16> = "dlmod-system-input.dll".encode_utf16().collect();
        if !path(&suffix, &mut filename) {
            return 0;
        }
        let module = LoadLibraryExW(
            filename.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        );
        if module.is_null() {
            return 0;
        }
        let resolved = GetProcAddress(module, c"DirectInput8Create".as_ptr().cast())
            .map(|f| f as usize)
            .unwrap_or(0);
        if resolved == DirectInput8Create as *const () as usize {
            0
        } else {
            resolved
        }
    });
    if address == 0 {
        return 0x80004005u32 as i32;
    }
    let function: unsafe extern "system" fn(
        HINSTANCE,
        u32,
        *const c_void,
        *mut *mut c_void,
        *mut c_void,
    ) -> i32 = std::mem::transmute(address);
    function(instance, version, iid, result, outer)
}
#[no_mangle]
pub extern "system" fn DLModBootstrapStatus(_: *mut c_void) -> u32 {
    STATUS.load(Ordering::Acquire)
}
