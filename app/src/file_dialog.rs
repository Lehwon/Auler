use std::io;
use std::path::PathBuf;

use crate::AppWindow;

#[derive(Clone, Copy)]
pub enum FileDialog {
    OpenWorkspace,
    OpenBackup,
    SaveWorkspace,
    SaveCatalog,
    OpenWorkbook,
    SaveWorkbook,
    SavePdf,
    SaveHandover,
    OpenSchoolProfile,
    SaveSchoolSubmission,
    SaveBackup,
}

#[cfg(windows)]
pub fn choose(ui: &AppWindow, mode: FileDialog) -> io::Result<Option<PathBuf>> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use slint::ComponentHandle;
    use windows_sys::Win32::UI::Controls::Dialogs::{
        CommDlgExtendedError, GetOpenFileNameW, GetSaveFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST,
        OFN_NOCHANGEDIR, OFN_NOREADONLYRETURN, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };

    let handle = ui.window().window_handle();
    let owner = match handle.window_handle().map(|handle| handle.as_raw()) {
        Ok(RawWindowHandle::Win32(handle)) => handle.hwnd.get() as _,
        _ => std::ptr::null_mut(),
    };
    let save = matches!(
        mode,
        FileDialog::SaveWorkspace
            | FileDialog::SaveCatalog
            | FileDialog::SaveWorkbook
            | FileDialog::SavePdf
            | FileDialog::SaveHandover
            | FileDialog::SaveSchoolSubmission
            | FileDialog::SaveBackup
    );
    let filter = match mode {
        FileDialog::OpenBackup => "Auler (*.auler;*.auler-backup)\0*.auler;*.auler-backup\0\0",
        FileDialog::SaveBackup => "Auler backup (*.auler-backup)\0*.auler-backup\0\0",
        FileDialog::SaveCatalog => "CSV (*.csv)\0*.csv\0\0",
        FileDialog::OpenWorkbook | FileDialog::SaveWorkbook => "Excel (*.xlsx)\0*.xlsx\0\0",
        FileDialog::SavePdf => "PDF (*.pdf)\0*.pdf\0\0",
        FileDialog::SaveHandover | FileDialog::OpenSchoolProfile => "JSON (*.json)\0*.json\0\0",
        FileDialog::SaveSchoolSubmission => "CSV (*.csv)\0*.csv\0\0",
        _ => "Auler (*.auler)\0*.auler\0\0",
    };
    let filter: Vec<u16> = filter.encode_utf16().collect();
    let extension: Vec<u16> = match mode {
        FileDialog::SaveCatalog => "csv\0",
        FileDialog::OpenWorkbook | FileDialog::SaveWorkbook => "xlsx\0",
        FileDialog::SavePdf => "pdf\0",
        FileDialog::SaveHandover | FileDialog::OpenSchoolProfile => "json\0",
        FileDialog::SaveSchoolSubmission => "csv\0",
        FileDialog::SaveBackup => "auler-backup\0",
        _ => "auler\0",
    }
    .encode_utf16()
    .collect();
    let mut name = vec![0u16; 32768];
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: filter.as_ptr(),
        lpstrFile: name.as_mut_ptr(),
        nMaxFile: name.len() as u32,
        lpstrDefExt: extension.as_ptr(),
        Flags: OFN_EXPLORER
            | OFN_PATHMUSTEXIST
            | OFN_NOCHANGEDIR
            | if save {
                OFN_NOREADONLYRETURN
            } else {
                OFN_FILEMUSTEXIST
            },
        ..OPENFILENAMEW::default()
    };
    // All strings and the writable buffer stay valid during the synchronous native dialog.
    let success = unsafe {
        if save {
            GetSaveFileNameW(&mut dialog)
        } else {
            GetOpenFileNameW(&mut dialog)
        }
    };
    if success != 0 {
        let length = name
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(name.len());
        return Ok(Some(PathBuf::from(OsString::from_wide(&name[..length]))));
    }
    let error = unsafe { CommDlgExtendedError() };
    if error == 0 {
        Ok(None)
    } else {
        Err(io::Error::other(format!("File dialog error 0x{error:X}")))
    }
}

#[cfg(not(windows))]
pub fn choose(_ui: &AppWindow, _mode: FileDialog) -> io::Result<Option<PathBuf>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows file dialog required",
    ))
}
