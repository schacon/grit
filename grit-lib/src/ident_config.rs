//! Default identity values from config and the system (Git `ident.c`).

use crate::config::ConfigSet;

/// Git `ident_default_name()` for this merged config: `user.name` if that key was ever set
/// (including to `""`), otherwise the passwd short name (Unix) or `USER` / `"unknown"`.
#[must_use]
pub fn ident_default_name(config: &ConfigSet) -> String {
    if config.get_last_entry("user.name").is_some() {
        config
            .get("user.name")
            .map(|s| s.trim().to_owned())
            .unwrap_or_default()
    } else {
        passwd_short_username()
    }
}

#[cfg(unix)]
fn passwd_short_username() -> String {
    // SAFETY: `getpwuid_r` writes through `buf` for the duration of the call; `pwd` is stack-local.
    let uid = unsafe { libc::getuid() };
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    let mut buf = vec![0u8; 16_384];
    let rv = unsafe {
        libc::getpwuid_r(
            uid,
            &mut pwd,
            buf.as_mut_ptr().cast(),
            buf.len(),
            &mut result,
        )
    };
    if rv != 0 || result.is_null() {
        return std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "unknown".to_owned());
    }
    let name_ptr = pwd.pw_name;
    if name_ptr.is_null() {
        return std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "unknown".to_owned());
    }
    let cstr = unsafe { std::ffi::CStr::from_ptr(name_ptr) };
    cstr.to_string_lossy().into_owned()
}

#[cfg(not(unix))]
fn passwd_short_username() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_owned())
}
