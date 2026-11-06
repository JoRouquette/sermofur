//! The only module of the port with `unsafe` code (derogation in `PARITY.md`, decision A1 of
//! 2026-11-06): calls to the Windows API that the standard library does not offer. Each call
//! copies what it reads into owned Rust values before it returns, so no raw pointer leaves the
//! module.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{
    GetLengthSid, GetTokenInformation, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    SID_AND_ATTRIBUTES, TOKEN_GROUPS, TOKEN_INFORMATION_CLASS, TOKEN_QUERY, TOKEN_USER,
    TokenGroups, TokenUser,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// The user and the groups of the token of the current process, as `WindowsIdentity.GetCurrent`
/// reads them when the thread does not impersonate.
pub(crate) struct ProcessToken {
    /// Binary form of the user SID.
    pub user: Vec<u8>,
    /// Binary form and attributes of each group SID.
    pub groups: Vec<(Vec<u8>, u32)>,
}

/// The NUL-terminated UTF-16 form of a path.
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}

/// Copies the binary form of a SID.
///
/// # Safety
/// `sid` points to a valid SID that stays alive during the call.
unsafe fn copy_sid(sid: PSID) -> Vec<u8> {
    // SAFETY: the caller guarantees a valid SID; GetLengthSid reads its header only.
    let length = unsafe { GetLengthSid(sid) } as usize;
    // SAFETY: a valid SID spans exactly GetLengthSid bytes.
    unsafe { std::slice::from_raw_parts(sid.cast::<u8>(), length) }.to_vec()
}

/// The binary form of the owner SID of a file or folder, read by name with
/// `GetNamedSecurityInfoW` as `FileSystemAclExtensions.GetAccessControl` does; `None` when the
/// security descriptor has no owner.
pub(crate) fn owner_sid(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let name = wide(path);
    let mut owner: PSID = ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: `name` is NUL-terminated and outlives the call; the out pointers are valid; the
    // API allocates `descriptor` with LocalAlloc, and `owner` points inside it.
    let status = unsafe {
        GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let copied = if owner.is_null() {
        None
    } else {
        // SAFETY: on success `owner` is a valid SID inside `descriptor`, freed only below.
        Some(unsafe { copy_sid(owner) })
    };
    // SAFETY: `descriptor` was allocated by the API with LocalAlloc and is freed once.
    unsafe { LocalFree(descriptor) };
    Ok(copied)
}

/// Reads one class of token information into a buffer aligned for the structures it holds.
fn token_information(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<Vec<u64>> {
    let mut length = 0u32;
    // SAFETY: a null buffer of length 0 asks only for the needed length.
    unsafe { GetTokenInformation(token, class, ptr::null_mut(), 0, &mut length) };
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut buffer = vec![0u64; (length as usize).div_ceil(8)];
    // SAFETY: the buffer is writable for at least `length` bytes and aligned to 8.
    let ok = unsafe {
        GetTokenInformation(
            token,
            class,
            buffer.as_mut_ptr().cast::<c_void>(),
            length,
            &mut length,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(buffer)
}

/// The user and groups of the token of the current process.
pub(crate) fn process_token() -> io::Result<ProcessToken> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle; `token` is a valid out pointer.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let read = || -> io::Result<ProcessToken> {
        let user_buffer = token_information(token, TokenUser)?;
        // SAFETY: the buffer holds a TOKEN_USER whose SID points inside the same buffer.
        let user = unsafe {
            let info = &*user_buffer.as_ptr().cast::<TOKEN_USER>();
            copy_sid(info.User.Sid)
        };
        let group_buffer = token_information(token, TokenGroups)?;
        // SAFETY: the buffer holds a TOKEN_GROUPS with GroupCount entries that follow the
        // count, each SID pointing inside the same buffer.
        let groups = unsafe {
            let info = group_buffer.as_ptr().cast::<TOKEN_GROUPS>();
            let count = (*info).GroupCount as usize;
            let first = ptr::addr_of!((*info).Groups).cast::<SID_AND_ATTRIBUTES>();
            (0..count)
                .map(|i| {
                    let group = &*first.add(i);
                    (copy_sid(group.Sid), group.Attributes)
                })
                .collect()
        };
        Ok(ProcessToken { user, groups })
    };
    let result = read();
    // SAFETY: `token` was opened above and is closed once.
    unsafe { CloseHandle(token) };
    result
}

/// Whether `root` (a volume root such as `C:\`) is a mapped network drive, as
/// `DriveInfo.DriveType == DriveType.Network`.
pub(crate) fn is_network_drive(root: &Path) -> bool {
    const DRIVE_REMOTE: u32 = 4;
    let name = wide(root);
    // SAFETY: `name` is NUL-terminated and outlives the call, which only reads it.
    unsafe { windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(name.as_ptr()) == DRIVE_REMOTE }
}

/// `GetLongPathNameW` on a UTF-16 path without terminator: the long form, or the Win32 error.
pub(crate) fn long_path_name(path: &[u16]) -> Result<Vec<u16>, u32> {
    let input: Vec<u16> = path.iter().copied().chain([0]).collect();
    let mut buffer = vec![0u16; 260];
    loop {
        let capacity = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
        // SAFETY: `input` is NUL-terminated and outlives the call; `buffer` is writable for
        // `capacity` units, which the API never exceeds.
        let length = unsafe {
            windows_sys::Win32::Storage::FileSystem::GetLongPathNameW(
                input.as_ptr(),
                buffer.as_mut_ptr(),
                capacity,
            )
        };
        if length == 0 {
            let error = io::Error::last_os_error().raw_os_error().unwrap_or(0);
            return Err(u32::try_from(error).unwrap_or(0));
        }
        if length as usize > buffer.len() {
            // Too small: `length` is the size needed, terminator included.
            buffer.resize(length as usize, 0);
            continue;
        }
        buffer.truncate(length as usize);
        return Ok(buffer);
    }
}
