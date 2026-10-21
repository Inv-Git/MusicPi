//! Lancement au démarrage de Windows (clé « Run » de l'utilisateur, sans droits admin).

#[cfg(windows)]
mod sys {
    use std::ffi::c_void;
    #[link(name = "advapi32")]
    extern "system" {
        pub fn RegSetKeyValueW(hkey: isize, sub: *const u16, value: *const u16, ty: u32, data: *const c_void, len: u32) -> i32;
        pub fn RegDeleteKeyValueW(hkey: isize, sub: *const u16, value: *const u16) -> i32;
        pub fn RegGetValueW(hkey: isize, sub: *const u16, value: *const u16, flags: u32, ty: *mut u32, data: *mut c_void, len: *mut u32) -> i32;
    }
    pub const HKEY_CURRENT_USER: isize = 0x8000_0001u32 as i32 as isize;
    pub const REG_SZ: u32 = 1;
    pub const RRF_RT_REG_SZ: u32 = 0x02;
    pub fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE: &str = "MusicPi";
/// Ancien nom de l'app : sa valeur est retirée.
const OLD_VALUE: &str = "Music";

#[cfg(windows)]
pub fn set_autostart(on: bool) -> Result<(), String> {
    use sys::*;
    let (k, v) = (wide(RUN_KEY), wide(VALUE));
    let old = wide(OLD_VALUE);
    unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), old.as_ptr()) };
    let r = if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let cmd = wide(&format!("\"{}\" --minimized", exe.display()));
        unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), v.as_ptr(), REG_SZ, cmd.as_ptr() as *const std::ffi::c_void, (cmd.len() * 2) as u32) }
    } else {
        let r = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), v.as_ptr()) };
        // 2 = déjà absent.
        if r == 2 {
            0
        } else {
            r
        }
    };
    if r == 0 {
        Ok(())
    } else {
        Err(format!("Registre Windows : erreur {r}"))
    }
}

#[cfg(windows)]
pub fn autostart_enabled() -> bool {
    use sys::*;
    let (k, v) = (wide(RUN_KEY), wide(VALUE));
    let mut buf = [0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    unsafe { RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), v.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr() as *mut std::ffi::c_void, &mut len) == 0 }
}

#[cfg(not(windows))]
pub fn set_autostart(_on: bool) -> Result<(), String> {
    Err("Disponible seulement sous Windows".into())
}

#[cfg(not(windows))]
pub fn autostart_enabled() -> bool {
    false
}
