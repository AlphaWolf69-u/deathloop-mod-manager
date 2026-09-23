use crate::{Memory, Result};
use std::{
    ffi::c_void,
    mem::{size_of, zeroed},
};
use windows_sys::Win32::{
    Foundation::*,
    System::{
        Diagnostics::{Debug::*, ToolHelp::*},
        Threading::*,
    },
};

pub struct Handle(pub HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
                CloseHandle(self.0);
            }
        }
    }
}
pub struct Process {
    pub handle: Handle,
    pub pid: u32,
    pub base: usize,
}
pub fn error(context: &str) -> String {
    format!("{context}: {}", std::io::Error::last_os_error())
}
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
pub fn narrow(s: &[u16]) -> String {
    String::from_utf16_lossy(&s[..s.iter().position(|v| *v == 0).unwrap_or(s.len())])
}

pub fn modules(pid: u32) -> Result<Vec<(String, usize, String)>> {
    unsafe {
        let snap = Handle(CreateToolhelp32Snapshot(
            TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32,
            pid,
        ));
        if snap.0 == INVALID_HANDLE_VALUE {
            return Err(error("List game modules"));
        }
        let mut entry: MODULEENTRY32W = zeroed();
        entry.dwSize = size_of::<MODULEENTRY32W>() as u32;
        let mut out = Vec::new();
        if Module32FirstW(snap.0, &mut entry) == 0 {
            return Err(error("Read first module"));
        }
        loop {
            out.push((
                narrow(&entry.szModule),
                entry.modBaseAddr as usize,
                narrow(&entry.szExePath),
            ));
            if Module32NextW(snap.0, &mut entry) == 0 {
                break;
            }
        }
        Ok(out)
    }
}
pub fn game_pid() -> Result<u32> {
    unsafe {
        let snap = Handle(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0));
        if snap.0 == INVALID_HANDLE_VALUE {
            return Err(error("List processes"));
        }
        let mut entry: PROCESSENTRY32W = zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut found = Vec::new();
        if Process32FirstW(snap.0, &mut entry) != 0 {
            loop {
                if narrow(&entry.szExeFile).eq_ignore_ascii_case("Deathloop.exe") {
                    found.push(entry.th32ProcessID);
                }
                if Process32NextW(snap.0, &mut entry) == 0 {
                    break;
                }
            }
        }
        if found.len() != 1 {
            return Err(format!(
                "Expected one running Deathloop.exe; found {}. Start it and stay at the menu.",
                found.len()
            ));
        }
        Ok(found[0])
    }
}
impl Process {
    pub fn open(pid: u32, write: bool) -> Result<Self> {
        let rights = PROCESS_QUERY_INFORMATION
            | PROCESS_VM_READ
            | if write {
                PROCESS_VM_WRITE | PROCESS_VM_OPERATION | PROCESS_CREATE_THREAD
            } else {
                0
            };
        let handle = Handle(unsafe { OpenProcess(rights, 0, pid) });
        if handle.0.is_null() {
            return Err(error(
                "Open Deathloop (try running the manager as administrator)",
            ));
        }
        let base = modules(pid)?
            .into_iter()
            .find(|m| m.0.eq_ignore_ascii_case("Deathloop.exe"))
            .ok_or("Game module missing")?
            .1;
        Ok(Self { handle, pid, base })
    }
    pub fn u32(&self, a: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(self.read(a, 4)?.try_into().unwrap()))
    }
    pub fn q(&self, a: usize) -> Result<usize> {
        Ok(u64::from_le_bytes(self.read(a, 8)?.try_into().unwrap()) as usize)
    }
    pub fn string(&self, a: usize, limit: usize) -> Result<String> {
        if limit > 4096 {
            return Err("String limit exceeds 4096".into());
        }
        let mut out = Vec::new();
        for i in 0..limit {
            let b = self.read(a.checked_add(i).ok_or("Address overflow")?, 1)?[0];
            if b == 0 {
                return String::from_utf8(out).map_err(|e| e.to_string());
            }
            out.push(b);
        }
        Err("Unterminated string".into())
    }
    pub fn idle(&self) -> Result<()> {
        let profile = self.q(self.q(self.base + self.layout()?.profile)?)?;
        if profile == 0 {
            return Err("Profile not initialized; wait at the menu".into());
        }
        if self.u32(profile + 0x4028 + 0xC0)? != 0 || self.u32(profile + 0x4028 + 0xBC)? != 0 {
            return Err("Finish matchmaking / disconnect before loading a profile".into());
        }
        Ok(())
    }
    pub fn compatibility(&self) -> Result<(usize, String)> {
        // Known x64 executable, then known data layout and exact build label.
        if self.read(self.base, 2)? != b"MZ" {
            return Err("Invalid game image".into());
        }
        let pe = self.u32(self.base + 0x3C)? as usize;
        if pe > 0x10000 || self.read(self.base + pe, 6)? != [0x50, 0x45, 0, 0, 0x64, 0x86] {
            return Err("Not the supported x64 image".into());
        }
        let layout = self.layout()?;
        let address = self.q(self.base + layout.compatibility)?;
        if address == 0 {
            return Ok((0, String::new()));
        }
        let value = self.string(address, 128)?;
        if (self.u32(self.base + layout.compatibility + 8)? & 0x1FFF_FFFF) as usize != value.len() {
            return Err("Unexpected compatibility string layout".into());
        }
        Ok((address, value))
    }
    pub fn preflight(&self) -> Result<()> {
        self.preflight_for(None)
    }
    pub fn preflight_for(&self, fingerprint: Option<&str>) -> Result<()> {
        let layout = self.layout()?;
        self.idle()?;
        if self.read(self.base + layout.builder, 16)?
            != [
                0x40, 0x57, 0x48, 0x83, 0xEC, 0x40, 0x48, 0xC7, 0x44, 0x24, 0x30, 0xFE, 0xFF, 0xFF,
                0xFF, 0x48,
            ]
        {
            return Err("Compatibility builder signature mismatch".into());
        }
        let version = self.string(self.q(self.base + layout.version)?, 128)?;
        let expected = fingerprint.map(crate::pool_version).transpose()?;
        if (version != crate::GAME_VERSION && expected.as_deref() != Some(version.as_str()))
            || self.string(self.q(self.base + layout.retail)?, 32)? != "Retail"
            || self.q(self.base + layout.transport)? != 1
        {
            return Err(
                "Unsupported build/version/transport, or a mod profile is already active".into(),
            );
        }
        let suffix = self.string(self.q(self.base + layout.suffix)?, 32)?;
        if !suffix.is_empty() && suffix != "_deluxe" {
            return Err("Unknown compatibility suffix".into());
        }
        if self.read(self.base + layout.empty, 1)? != [0] {
            return Err("Empty suffix literal mismatch".into());
        }
        let (_, key) = self.compatibility()?;
        if !key.is_empty() {
            crate::pool_key(fingerprint.unwrap_or(&"0".repeat(64)), &key)?;
        }
        Ok(())
    }
    pub fn layout(&self) -> Result<crate::layout::Layout> {
        if self.read(self.base, 2)? != b"MZ" {
            return Err("Invalid game image".into());
        }
        let pe = self.u32(self.base + 0x3c)? as usize;
        if pe > 0x10000 || self.read(self.base + pe, 6)? != [0x50, 0x45, 0, 0, 0x64, 0x86] {
            return Err("Unsupported PE image".into());
        }
        crate::layout::identify(
            self.u32(self.base + pe + 8)?,
            self.u32(self.base + pe + 24 + 56)?,
        )
        .ok_or_else(|| "Unrecognized executable layout; no offsets selected".into())
    }
}
impl Memory for Process {
    fn read(&self, a: usize, n: usize) -> Result<Vec<u8>> {
        if a < 0x10000 || n > 1048576 || a.checked_add(n).is_none() {
            return Err("Invalid memory read".into());
        }
        let mut bytes = vec![0; n];
        let mut got = 0;
        if unsafe {
            ReadProcessMemory(
                self.handle.0,
                a as *const c_void,
                bytes.as_mut_ptr().cast(),
                n,
                &mut got,
            )
        } == 0
            || got != n
        {
            return Err(error(&format!("Read {a:X}")));
        }
        Ok(bytes)
    }
    fn write(&self, a: usize, data: &[u8]) -> Result<()> {
        if a < 0x10000 || data.len() > 1048576 || a.checked_add(data.len()).is_none() {
            return Err("Invalid memory write".into());
        }
        let mut wrote = 0;
        if unsafe {
            WriteProcessMemory(
                self.handle.0,
                a as *mut c_void,
                data.as_ptr().cast(),
                data.len(),
                &mut wrote,
            )
        } == 0
            || wrote != data.len()
        {
            return Err(error(&format!("Write {a:X}")));
        }
        Ok(())
    }
}
