use std::io;
use std::path::Path;

#[cfg(windows)]
pub(crate) fn protect(path: &Path, directory: bool) -> io::Result<()> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, GENERIC_ALL, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        EXPLICIT_ACCESS_W, GRANT_ACCESS, SE_FILE_OBJECT, SetEntriesInAclW, SetNamedSecurityInfoW,
        TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
    };
    use windows_sys::Win32::Security::{
        ACL, CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, GetTokenInformation,
        OBJECT_INHERIT_ACE, PROTECTED_DACL_SECURITY_INFORMATION, TOKEN_QUERY, TOKEN_USER,
        TokenUser,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut handle = null_mut();
    // The output pointer is writable. A successful call returns an owned token handle.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = unsafe { OwnedHandle::from_raw_handle(handle) };
    let mut bytes = 0;
    // An empty output buffer requests the needed token-user buffer size.
    unsafe { GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut bytes) };
    if (bytes as usize) < size_of::<TOKEN_USER>() {
        return Err(io::Error::last_os_error());
    }
    let mut user = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
    // The buffer is pointer-aligned for TOKEN_USER and remains live through ACL creation.
    let read = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            user.as_mut_ptr().cast::<c_void>(),
            bytes,
            &mut bytes,
        )
    };
    if read == 0 {
        return Err(io::Error::last_os_error());
    }
    let sid = unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let requested = EXPLICIT_ACCESS_W {
        grfAccessPermissions: GENERIC_ALL,
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: if directory {
            CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE
        } else {
            0
        },
        Trustee: TRUSTEE_W {
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: sid.cast(),
            ..Default::default()
        },
    };
    let mut acl: *mut ACL = null_mut();
    // With no original ACL, the returned DACL contains the single explicit entry above.
    let create = unsafe { SetEntriesInAclW(1, &requested, null(), &mut acl) };
    if create != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(create as i32));
    }

    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // The name is NUL-terminated and the allocated DACL stays live until the call returns.
    let change = unsafe {
        SetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            acl,
            null(),
        )
    };
    unsafe { LocalFree(acl.cast()) };
    if change == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(change as i32))
    }
}

#[cfg(unix)]
pub(crate) fn protect(path: &Path, directory: bool) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        path,
        std::fs::Permissions::from_mode(if directory { 0o700 } else { 0o600 }),
    )
}

#[cfg(not(any(windows, unix)))]
pub(crate) fn protect(_path: &Path, _directory: bool) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Private backup files are unsupported on this platform",
    ))
}
