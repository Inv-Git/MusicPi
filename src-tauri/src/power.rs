//! Mode ambiance : empêcher la mise en veille de l'écran, et savoir depuis quand
//! personne n'a touché l'ordinateur (pour lancer l'ambiance tout seul).

#[cfg(windows)]
mod sys {
    #[repr(C)]
    pub struct LastInputInfo {
        pub size: u32,
        pub time: u32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        pub fn SetThreadExecutionState(flags: u32) -> u32;
        pub fn GetTickCount() -> u32;
    }
    #[link(name = "user32")]
    extern "system" {
        pub fn GetLastInputInfo(info: *mut LastInputInfo) -> i32;
    }
    pub const ES_CONTINUOUS: u32 = 0x8000_0000;
    pub const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
    pub const ES_DISPLAY_REQUIRED: u32 = 0x0000_0002;
}

/// Écran toujours allumé (true) ou retour au comportement normal (false).
/// L'état vaut pour le fil d'exécution appelant : on l'appelle toujours depuis le même.
#[cfg(windows)]
pub fn keep_awake(on: bool) {
    use sys::*;
    let flags = if on { ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED } else { ES_CONTINUOUS };
    unsafe { SetThreadExecutionState(flags) };
}

/// Secondes depuis la dernière action au clavier ou à la souris, toutes applications confondues.
#[cfg(windows)]
pub fn idle_seconds() -> u32 {
    use sys::*;
    let mut info = LastInputInfo { size: std::mem::size_of::<LastInputInfo>() as u32, time: 0 };
    unsafe {
        if GetLastInputInfo(&mut info) == 0 {
            return 0;
        }
        GetTickCount().wrapping_sub(info.time) / 1000
    }
}

#[cfg(not(windows))]
pub fn keep_awake(_on: bool) {}

#[cfg(not(windows))]
pub fn idle_seconds() -> u32 {
    0
}
