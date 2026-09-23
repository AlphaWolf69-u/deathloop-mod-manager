use dlmod_core::{
    win::{error, narrow, wide},
    Result,
};
use std::path::PathBuf;
use windows_sys::Win32::UI::{Controls::Dialogs::*, Shell::*, WindowsAndMessaging::*};
pub fn message(text: &str) {
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide(text).as_ptr(),
            wide("Deathloop Mod Manager").as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}
pub fn confirm(text: &str) -> bool {
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide(text).as_ptr(),
            wide("Deathloop Mod Manager").as_ptr(),
            MB_YESNO | MB_ICONQUESTION,
        ) == IDYES
    }
}
pub fn open(path: &str) -> Result<()> {
    unsafe {
        if ShellExecuteW(
            std::ptr::null_mut(),
            wide("open").as_ptr(),
            wide(path).as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        ) as usize
            <= 32
        {
            return Err(error("Open"));
        }
    }
    Ok(())
}
pub fn pick(game: bool) -> Option<PathBuf> {
    unsafe {
        let mut buffer = vec![0u16; 32768];
        let filter = wide(if game {
            "Deathloop executable\0Deathloop.exe\0\0"
        } else {
            "Mod package\0*.zip;mod.toml\0\0"
        });
        let title = wide(if game {
            "Select Deathloop.exe"
        } else {
            "Install a mod"
        });
        let mut dialog: OPENFILENAMEW = std::mem::zeroed();
        dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        dialog.lpstrFilter = filter.as_ptr();
        dialog.lpstrFile = buffer.as_mut_ptr();
        dialog.nMaxFile = buffer.len() as u32;
        dialog.lpstrTitle = title.as_ptr();
        dialog.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
        if GetOpenFileNameW(&mut dialog) == 0 {
            None
        } else {
            Some(PathBuf::from(narrow(&buffer)))
        }
    }
}
