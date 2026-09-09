#[cfg(windows)]
use std::ffi::c_void;

const TARGET_PREFIX: &str = "SageDataBridge/connection/";

fn target_name(connection_id: &str) -> String {
    format!("{}{}", TARGET_PREFIX, connection_id)
}

#[cfg(windows)]
#[repr(C)]
struct CredentialW {
    flags: u32,
    credential_type: u32,
    target_name: *mut u16,
    comment: *mut u16,
    last_written: u64,
    credential_blob_size: u32,
    credential_blob: *mut u8,
    persist: u32,
    attribute_count: u32,
    attributes: *mut c_void,
    target_alias: *mut u16,
    user_name: *mut u16,
}

#[cfg(windows)]
#[link(name = "Advapi32")]
extern "system" {
    fn CredWriteW(credential: *const CredentialW, flags: u32) -> i32;
    fn CredReadW(target_name: *const u16, credential_type: u32, flags: u32, credential: *mut *mut CredentialW) -> i32;
    fn CredDeleteW(target_name: *const u16, credential_type: u32, flags: u32) -> i32;
    fn CredFree(buffer: *mut c_void);
}

#[cfg(windows)]
const CRED_TYPE_GENERIC: u32 = 1;
#[cfg(windows)]
const CRED_PERSIST_LOCAL_MACHINE: u32 = 2;

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn last_error(action: &str) -> String {
    format!("Windows Credential Manager {} failed: {}", action, std::io::Error::last_os_error())
}

/// Stores SQL passwords in the current Windows user's Credential Manager vault.
#[cfg(windows)]
pub fn write(connection_id: &str, secret: &str) -> Result<(), String> {
    let mut target = wide(&target_name(connection_id));
    let mut user_name = wide("Sage Data Bridge");
    let mut blob = secret.as_bytes().to_vec();
    let credential = CredentialW {
        flags: 0,
        credential_type: CRED_TYPE_GENERIC,
        target_name: target.as_mut_ptr(),
        comment: std::ptr::null_mut(),
        last_written: 0,
        credential_blob_size: blob.len().try_into().map_err(|_| "Database password is too large".to_string())?,
        credential_blob: blob.as_mut_ptr(),
        persist: CRED_PERSIST_LOCAL_MACHINE,
        attribute_count: 0,
        attributes: std::ptr::null_mut(),
        target_alias: std::ptr::null_mut(),
        user_name: user_name.as_mut_ptr(),
    };
    let result = unsafe { CredWriteW(&credential, 0) };
    if result == 0 {
        return Err(last_error("write"));
    }
    Ok(())
}

#[cfg(windows)]
pub fn read(connection_id: &str) -> Result<Option<String>, String> {
    let target = wide(&target_name(connection_id));
    let mut credential = std::ptr::null_mut();
    let result = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) };
    if result == 0 {
        let error = std::io::Error::last_os_error();
        // ERROR_NOT_FOUND is the normal case for Windows-auth and newly-created connections.
        if error.raw_os_error() == Some(1168) {
            return Ok(None);
        }
        return Err(format!("Windows Credential Manager read failed: {}", error));
    }
    let bytes = unsafe {
        std::slice::from_raw_parts((*credential).credential_blob, (*credential).credential_blob_size as usize)
    };
    let value = String::from_utf8(bytes.to_vec())
        .map_err(|_| "Stored database credential is not valid UTF-8".to_string());
    unsafe { CredFree(credential.cast::<c_void>()) };
    value.map(Some)
}

#[cfg(windows)]
pub fn delete(connection_id: &str) -> Result<(), String> {
    let target = wide(&target_name(connection_id));
    let result = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
    if result == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(1168) {
            return Err(format!("Windows Credential Manager delete failed: {}", error));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn write(_connection_id: &str, _secret: &str) -> Result<(), String> {
    Err("Windows Credential Manager is only available on Windows".to_string())
}

#[cfg(not(windows))]
pub fn read(_connection_id: &str) -> Result<Option<String>, String> {
    Err("Windows Credential Manager is only available on Windows".to_string())
}

#[cfg(not(windows))]
pub fn delete(_connection_id: &str) -> Result<(), String> {
    Err("Windows Credential Manager is only available on Windows".to_string())
}
