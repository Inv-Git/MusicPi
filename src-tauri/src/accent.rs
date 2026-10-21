//! Couleur d'accent choisie dans Windows (Paramètres › Personnalisation › Couleurs).

use serde::Serialize;

#[derive(Serialize, Default)]
pub struct WindowsAccent {
    /// Couleur d'accent, « #rrggbb ».
    pub accent: Option<String>,
    /// Nuances calculées par Windows, de la plus claire à la plus foncée (8 couleurs).
    pub palette: Vec<String>,
}

#[cfg(windows)]
mod sys {
    use std::ffi::c_void;
    #[link(name = "advapi32")]
    extern "system" {
        pub fn RegGetValueW(hkey: isize, sub: *const u16, value: *const u16, flags: u32, ty: *mut u32, data: *mut c_void, len: *mut u32) -> i32;
    }
    pub const HKEY_CURRENT_USER: isize = 0x8000_0001u32 as i32 as isize;
    pub const RRF_RT_REG_DWORD: u32 = 0x10;
    pub const RRF_RT_REG_BINARY: u32 = 0x08;

    pub fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn read(sub: &str, value: &str, flags: u32, buf: &mut [u8]) -> Option<usize> {
        let (s, v) = (wide(sub), wide(value));
        let mut len = buf.len() as u32;
        let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, s.as_ptr(), v.as_ptr(), flags, std::ptr::null_mut(), buf.as_mut_ptr() as *mut c_void, &mut len) };
        (r == 0).then_some(len as usize)
    }
}

fn hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

#[cfg(windows)]
pub fn get() -> WindowsAccent {
    let mut out = WindowsAccent::default();
    // Nuances : 8 entrées RGBA.
    let mut pal = [0u8; 32];
    if let Some(n) = sys::read("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Accent", "AccentPalette", sys::RRF_RT_REG_BINARY, &mut pal) {
        if n >= 32 {
            out.palette = pal.chunks(4).map(|c| hex(c[0], c[1], c[2])).collect();
        }
    }
    // Couleur d'accent : DWORD 0xAABBGGRR.
    let mut d = [0u8; 4];
    if sys::read("Software\\Microsoft\\Windows\\DWM", "AccentColor", sys::RRF_RT_REG_DWORD, &mut d).is_some() {
        out.accent = Some(hex(d[0], d[1], d[2]));
    } else if out.palette.len() == 8 {
        out.accent = Some(out.palette[3].clone());
    }
    out
}

#[cfg(not(windows))]
pub fn get() -> WindowsAccent {
    WindowsAccent::default()
}
