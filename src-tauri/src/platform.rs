//! Typed Windows platform helpers.
//!
//! Every Win32 call lives here so services stay free of `unsafe` and raw flags.

use std::{ffi::OsStr, os::windows::ffi::OsStrExt, path::PathBuf, sync::OnceLock};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, WIN32_ERROR};
use windows::Win32::System::Threading::{CreateMutexW, OpenMutexW, SYNCHRONIZATION_ACCESS_RIGHTS};
use windows::Win32::UI::Shell::IsUserAnAdmin;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE,
};

use crate::errors::{AppError, Result};

const ERROR_ALREADY_EXISTS: u32 = 183;

pub fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Windows 10/11 version information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsVersion {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
}

impl WindowsVersion {
    pub fn is_win10_or_newer(&self) -> bool {
        self.major > 10 || (self.major == 10 && self.build >= 10240)
    }

    pub fn is_win11(&self) -> bool {
        self.major > 10 || (self.major == 10 && self.build >= 22000)
    }
}

impl std::fmt::Display for WindowsVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.build)
    }
}

/// Read the real Windows version (manifest-safe, ignores compatibility shims).
pub fn windows_version() -> WindowsVersion {
    // RtlGetVersion is the reliable kernel call; fall back to GetVersionExW.
    #[repr(C)]
    #[allow(dead_code)] // Win32 ABI layout fields are required even when unread.
    struct OsVersionInfoExW {
        os_version_info_size: u32,
        major_version: u32,
        minor_version: u32,
        build_number: u32,
        platform_id: u32,
        sz_csd_version: [u16; 128],
        service_pack_major: u16,
        service_pack_minor: u16,
        suite_mask: u16,
        product_type: u8,
        reserved: u8,
    }
    extern "system" {
        fn RtlGetVersion(info: *mut OsVersionInfoExW) -> i32;
    }
    unsafe {
        let mut info = std::mem::zeroed::<OsVersionInfoExW>();
        info.os_version_info_size = std::mem::size_of::<OsVersionInfoExW>() as u32;
        let status = RtlGetVersion(&mut info);
        if status == 0 && info.major_version >= 6 {
            return WindowsVersion {
                major: info.major_version,
                minor: info.minor_version,
                build: info.build_number,
            };
        }
    }
    WindowsVersion {
        major: 10,
        minor: 0,
        build: 19045,
    }
}

pub fn is_admin() -> bool {
    unsafe { IsUserAnAdmin().as_bool() }
}

/// Acquire a named mutex. Returns `false` when another instance already owns it.
pub fn acquire_mutex(name: &str) -> bool {
    let full = format!("Local\\{name}");
    let wide_name = wide(&full);
    unsafe {
        let handle = match CreateMutexW(None, false, PCWSTR(wide_name.as_ptr())) {
            Ok(handle) => handle,
            Err(_) => return false,
        };
        let created = WIN32_ERROR(0);
        let err = windows::Win32::Foundation::GetLastError();
        if err == WIN32_ERROR(ERROR_ALREADY_EXISTS) {
            let _ = CloseHandle(handle);
            return false;
        }
        let _ = created;
        true
    }
}

pub fn mutex_held(name: &str) -> bool {
    let full = format!("Local\\{name}");
    let wide_name = wide(&full);
    unsafe {
        match OpenMutexW(
            SYNCHRONIZATION_ACCESS_RIGHTS(0x0010_0000),
            false,
            PCWSTR(wide_name.as_ptr()),
        ) {
            Ok(handle) => {
                let _ = CloseHandle(handle);
                true
            }
            Err(_) => false,
        }
    }
}

/// Handle to a held mutex that releases on drop.
pub struct MutexGuard {
    handle: HANDLE,
}

impl Drop for MutexGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

pub fn lock_mutex(name: &str) -> Option<MutexGuard> {
    let full = format!("Local\\{name}");
    let wide_name = wide(&full);
    unsafe {
        let handle = CreateMutexW(None, false, PCWSTR(wide_name.as_ptr())).ok()?;
        if windows::Win32::Foundation::GetLastError() == WIN32_ERROR(ERROR_ALREADY_EXISTS) {
            let _ = CloseHandle(handle);
            return None;
        }
        Some(MutexGuard { handle })
    }
}

pub fn find_window_by_title(title: &str) -> Option<HWND> {
    let wide_title = wide(title);
    unsafe {
        let hwnd = FindWindowW(None, PCWSTR(wide_title.as_ptr())).ok()?;
        if hwnd.0.is_null() {
            None
        } else {
            Some(hwnd)
        }
    }
}

pub fn restore_and_focus(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Apply the native Windows 10/11 dark titlebar attribute to a window.
pub fn set_dark_titlebar(hwnd: HWND, dark: bool) -> Result<()> {
    use windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE;
    let attr = DWMWINDOWATTRIBUTE(20i32); // DWMWA_USE_IMMERSIVE_DARK_MODE
    unsafe {
        windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(
            hwnd,
            attr,
            &(dark as i32) as *const _ as *const _,
            std::mem::size_of::<i32>() as u32,
        )
        .map_err(|e| AppError::Other(format!("DwmSetWindowAttribute failed: {e}")))
    }
}

/// Extract the WebView2 window handle from a Tauri window.
pub fn native_hwnd(window: &tauri::WebviewWindow) -> Option<HWND> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        let raw = window.hwnd().ok()?;
        Some(HWND(raw.0))
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        None
    }
}

pub fn current_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("opencode-egress-manager.exe"))
}

/// Install location used to discover bundled helper binaries.
pub fn exe_dir() -> PathBuf {
    current_exe()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Locate a helper binary next to the executable (Tauri externalBin output).
pub fn find_sidecar(name: &str) -> Option<PathBuf> {
    let dir = exe_dir();
    let direct = dir.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    let nested = dir.join("bin").join(name);
    if nested.is_file() {
        return Some(nested);
    }
    None
}

/// Launch an executable elevated (UAC prompt) with fixed arguments.
///
/// Returns true when ShellExecute accepted the request. This is only used for the
/// one-shot privileged firewall helper.
pub fn run_elevated(program: &str, arguments: &str) -> Result<i32> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let prog = wide(program);
    let args = wide(arguments);
    let verb = wide("runas");
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(prog.as_ptr()),
            PCWSTR(args.as_ptr()),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    let code = result.0 as usize as i32;
    if code <= 32 {
        Err(AppError::Other(format!(
            "elevation request failed (ShellExecute error {code})"
        )))
    } else {
        Ok(code)
    }
}

/// Open a URL in the default browser after validating the scheme.
pub fn open_external_url(url: &str) -> Result<()> {
    let parsed = url::Url::parse(url).map_err(|_| {
        AppError::validation(format!("invalid URL: {}", crate::errors::redact(url)))
    })?;
    match parsed.scheme() {
        "http" | "https" => {
            open::that_detached(url)
                .map_err(|e| AppError::Other(format!("could not open URL: {e}")))?;
            Ok(())
        }
        other => Err(AppError::validation(format!(
            "refusing to open non-HTTP URL (scheme {other})"
        ))),
    }
}

/// Read the Windows system accent color.
///
/// `HKCU\Software\Microsoft\DWM\AccentColor` holds a COLORREF (`0x00BBGGRR`).
/// Returns `#RRGGBB`, or `None` when the value is missing or unreadable.
pub fn system_accent_color() -> Option<String> {
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
        REG_SAM_FLAGS, REG_VALUE_TYPE,
    };

    const SUBKEY: &str = r"Software\Microsoft\DWM";
    const VALUE: &str = "AccentColor";

    unsafe {
        let subkey = wide(SUBKEY);
        let mut key: HKEY = HKEY(std::ptr::null_mut());
        let status = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            REG_SAM_FLAGS(KEY_READ.0),
            &mut key,
        );
        if status != WIN32_ERROR(0) {
            return None;
        }

        let value_name = wide(VALUE);
        let mut kind = REG_VALUE_TYPE::default();
        let mut size: u32 = 0;
        let query = RegQueryValueExW(
            key,
            PCWSTR(value_name.as_ptr()),
            None,
            Some(&mut kind),
            None,
            Some(&mut size),
        );
        if query != WIN32_ERROR(0) || size != 4 {
            let _ = RegCloseKey(key);
            return None;
        }

        let mut buf = [0u8; 4];
        let read = RegQueryValueExW(
            key,
            PCWSTR(value_name.as_ptr()),
            None,
            Some(&mut kind),
            Some(buf.as_mut_ptr()),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);
        if read != WIN32_ERROR(0) {
            return None;
        }

        let raw = u32::from_ne_bytes(buf);
        let red = (raw & 0xFF) as u8;
        let green = ((raw >> 8) & 0xFF) as u8;
        let blue = ((raw >> 16) & 0xFF) as u8;
        Some(format!("#{red:02X}{green:02X}{blue:02X}"))
    }
}

/// Cached Windows version (avoids repeated syscalls).
pub fn version() -> &'static WindowsVersion {
    static VERSION: OnceLock<WindowsVersion> = OnceLock::new();
    VERSION.get_or_init(windows_version)
}
