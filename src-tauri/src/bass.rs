//! Liaisons minimales vers BASS, BASSmix et BASSWASAPI.
//!
//! Les DLL sont chargées dynamiquement au lancement (aucune .lib nécessaire à la
//! compilation). Seules les fonctions utilisées par le moteur sont déclarées.

#![allow(non_snake_case, dead_code)]

use std::ffi::{c_char, c_void, CStr};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Constantes (bass.h, bassmix.h, basswasapi.h)
// ---------------------------------------------------------------------------
pub mod consts {
    // bass.h (vérifiées sur l'en-tête 2.4.18)
    pub const BASS_CONFIG_UNICODE: u32 = 42;
    pub const BASS_SAMPLE_FLOAT: u32 = 0x100;
    pub const BASS_STREAM_PRESCAN: u32 = 0x20000;
    pub const BASS_STREAM_AUTOFREE: u32 = 0x40000;
    pub const BASS_STREAM_DECODE: u32 = 0x200000;
    pub const BASS_UNICODE: u32 = 0x8000_0000;
    pub const BASS_FILE_NAME: u32 = 0;
    pub const BASS_POS_BYTE: u32 = 0;
    pub const BASS_SYNC_END: u32 = 2;
    pub const BASS_SYNC_MIXTIME: u32 = 0x4000_0000;
    pub const BASS_ATTRIB_VOL: u32 = 2;
    /// Volume appliqué en DSP : seul moyen de régler le volume d'un canal de décodage.
    pub const BASS_ATTRIB_VOLDSP: u32 = 19;
    pub const BASS_ATTRIB_SRC: u32 = 8;
    pub const BASS_DATA_AVAILABLE: u32 = 0;
    pub const BASS_ORIGRES_FLOAT: u32 = 0x10000;

    // bassmix.h
    pub const BASS_MIXER_NONSTOP: u32 = 0x20000;
    pub const BASS_MIXER_CHAN_NORAMPIN: u32 = 0x800000;
    pub const BASS_POS_MIXER_RESET: u32 = 0x10000;

    pub const BASS_SYNC_POS: u32 = 0;
    pub const BASS_MIXER_ENV_VOL: u32 = 2;
    // bassenc.h
    pub const BASS_ENCODE_AUTOFREE: u32 = 0x40000;

    // bass_fx.h
    pub const BASS_FX_FREESOURCE: u32 = 0x10000;
    pub const BASS_ATTRIB_TEMPO: u32 = 0x10000;

    // basswasapi.h
    pub const BASS_WASAPI_EXCLUSIVE: u32 = 1;
    pub const BASS_WASAPI_AUTOFORMAT: u32 = 2;
    pub const BASS_DEVICE_ENABLED: u32 = 1;
    pub const BASS_DEVICE_DEFAULT: u32 = 2;
    pub const BASS_DEVICE_LOOPBACK: u32 = 8;
    pub const BASS_DEVICE_INPUT: u32 = 16;
    pub const BASS_WASAPI_NOTIFY_DISABLED: u32 = 1;
    pub const BASS_WASAPI_NOTIFY_DEFOUTPUT: u32 = 2;
    pub const BASS_WASAPI_NOTIFY_FAIL: u32 = 0x100;
    pub const BASS_WASAPI_FORMAT_FLOAT: u32 = 0;
    pub const BASS_WASAPI_FORMAT_8BIT: u32 = 1;
    pub const BASS_WASAPI_FORMAT_16BIT: u32 = 2;
    pub const BASS_WASAPI_FORMAT_24BIT: u32 = 3;
    pub const BASS_WASAPI_FORMAT_32BIT: u32 = 4;
}

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------
#[repr(C)]
pub struct ChannelInfo {
    pub freq: u32,
    pub chans: u32,
    pub flags: u32,
    pub ctype: u32,
    pub origres: u32,
    pub plugin: u32,
    pub sample: u32,
    pub filename: *const c_char,
}

#[repr(C)]
pub struct WasapiDeviceInfo {
    pub name: *const c_char,
    pub id: *const c_char,
    pub kind: u32,
    pub flags: u32,
    pub minperiod: f32,
    pub defperiod: f32,
    pub mixfreq: u32,
    pub mixchans: u32,
}

#[repr(C)]
pub struct WasapiInfo {
    pub initflags: u32,
    pub freq: u32,
    pub chans: u32,
    pub format: u32,
    pub buflen: u32,
    pub volmax: f32,
    pub volmin: f32,
    pub volstep: f32,
}

pub type SyncProc = unsafe extern "system" fn(handle: u32, channel: u32, data: u32, user: *mut c_void);
pub type DspProc = unsafe extern "system" fn(dsp: u32, channel: u32, buffer: *mut c_void, length: u32, user: *mut c_void);
pub type WasapiNotifyProc = unsafe extern "system" fn(notify: u32, device: u32, user: *mut c_void);
pub type WasapiProc = unsafe extern "system" fn(buffer: *mut c_void, length: u32, user: *mut c_void) -> u32;

// ---------------------------------------------------------------------------
// Chargement dynamique
// ---------------------------------------------------------------------------
#[cfg(windows)]
mod sys {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    extern "system" {
        pub fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
        pub fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    }
    /// Résout les dépendances (bass.dll) dans le dossier de la DLL chargée.
    pub const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x8;
}

pub fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        s.encode_wide().chain(std::iter::once(0)).collect()
    }
    #[cfg(not(windows))]
    {
        s.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect()
    }
}

pub struct Library(usize);

impl Library {
    #[cfg(windows)]
    fn open(path: &Path) -> Result<Self, String> {
        let w = wide(path.as_os_str());
        let h = unsafe { sys::LoadLibraryExW(w.as_ptr(), std::ptr::null_mut(), sys::LOAD_WITH_ALTERED_SEARCH_PATH) };
        if h.is_null() {
            Err(format!("Impossible de charger {}", path.display()))
        } else {
            Ok(Library(h as usize))
        }
    }

    #[cfg(not(windows))]
    fn open(path: &Path) -> Result<Self, String> {
        Err(format!("Plateforme non prise en charge ({})", path.display()))
    }

    #[cfg(windows)]
    fn sym(&self, name: &str) -> Result<*mut c_void, String> {
        let mut n = name.as_bytes().to_vec();
        n.push(0);
        let p = unsafe { sys::GetProcAddress(self.0 as *mut c_void, n.as_ptr()) };
        if p.is_null() {
            Err(format!("Fonction {name} introuvable"))
        } else {
            Ok(p)
        }
    }

    #[cfg(not(windows))]
    fn sym(&self, name: &str) -> Result<*mut c_void, String> {
        Err(format!("Fonction {name} introuvable"))
    }
}

macro_rules! api {
    ($name:ident { $( $f:ident : fn($($a:ty),*) -> $r:ty ; )* }) => {
        pub struct $name { $( pub $f: unsafe extern "system" fn($($a),*) -> $r, )* }
        impl $name {
            fn load(lib: &Library) -> Result<Self, String> {
                unsafe {
                    Ok(Self { $( $f: std::mem::transmute::<*mut c_void, unsafe extern "system" fn($($a),*) -> $r>(lib.sym(stringify!($f))?), )* })
                }
            }
        }
    };
}

api!(Bass {
    BASS_GetVersion: fn() -> u32;
    BASS_ErrorGetCode: fn() -> i32;
    BASS_SetConfig: fn(u32, u32) -> i32;
    BASS_Init: fn(i32, u32, u32, *mut c_void, *const c_void) -> i32;
    BASS_PluginLoad: fn(*const c_void, u32) -> u32;
    BASS_StreamCreateFile: fn(u32, *const c_void, u64, u64, u32) -> u32;
    BASS_StreamFree: fn(u32) -> i32;
    BASS_ChannelGetInfo: fn(u32, *mut ChannelInfo) -> i32;
    BASS_ChannelGetLength: fn(u32, u32) -> u64;
    BASS_ChannelBytes2Seconds: fn(u32, u64) -> f64;
    BASS_ChannelSeconds2Bytes: fn(u32, f64) -> u64;
    BASS_ChannelGetData: fn(u32, *mut c_void, u32) -> u32;
    BASS_ChannelSetAttribute: fn(u32, u32, f32) -> i32;
    BASS_ChannelSetDSP: fn(u32, DspProc, *mut c_void, i32) -> u32;
});

api!(Mix {
    BASS_Mixer_StreamCreate: fn(u32, u32, u32) -> u32;
    BASS_Mixer_StreamAddChannel: fn(u32, u32, u32) -> i32;
    BASS_Mixer_ChannelGetPosition: fn(u32, u32) -> u64;
    BASS_Mixer_ChannelSetPosition: fn(u32, u64, u32) -> i32;
    BASS_Mixer_ChannelSetSync: fn(u32, u32, u64, SyncProc, *mut c_void) -> u32;
    BASS_Mixer_ChannelRemoveSync: fn(u32, u32) -> i32;
    BASS_Mixer_ChannelSetEnvelope: fn(u32, u32, *const MixerNode, u32) -> i32;
    BASS_Mixer_ChannelSetEnvelopePos: fn(u32, u32, u64) -> i32;
});

/// Point d'une enveloppe BASSmix (position en octets du mélangeur, valeur).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MixerNode {
    pub pos: u64,
    pub value: f32,
}

api!(Wasapi {
    BASS_WASAPI_GetDeviceInfo: fn(u32, *mut WasapiDeviceInfo) -> i32;
    BASS_WASAPI_CheckFormat: fn(i32, u32, u32, u32) -> u32;
    BASS_WASAPI_Init: fn(i32, u32, u32, u32, f32, f32, WasapiProc, *mut c_void) -> i32;
    BASS_WASAPI_Free: fn() -> i32;
    BASS_WASAPI_Start: fn() -> i32;
    BASS_WASAPI_Stop: fn(i32) -> i32;
    BASS_WASAPI_GetInfo: fn(*mut WasapiInfo) -> i32;
    BASS_WASAPI_GetData: fn(*mut c_void, u32) -> u32;
    BASS_WASAPI_SetNotify: fn(WasapiNotifyProc, *mut c_void) -> i32;
});

api!(Fx {
    BASS_FX_GetVersion: fn() -> u32;
    BASS_FX_TempoCreate: fn(u32, u32) -> u32;
});

api!(EncOpus {
    BASS_Encode_OPUS_StartFile: fn(u32, *const c_void, u32, *const c_void) -> u32;
});

pub struct Apis {
    pub bass: Bass,
    pub mix: Mix,
    pub wasapi: Wasapi,
    pub flac_plugin: bool,
    /// BASS_FX (changement de vitesse sans changer la hauteur), facultatif.
    pub fx: Option<Fx>,
    /// BASSenc + BASSenc_OPUS (version allégée pour le téléphone en 4G), facultatif.
    pub enc_opus: Option<EncOpus>,
}

fn find(dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    dirs.iter().map(|d| d.join(name)).find(|p| p.is_file())
}

/// Charge les DLL depuis le premier dossier qui les contient, puis initialise
/// BASS sur le périphérique « sans son » (décodage seul ; la sortie passe par WASAPI).
pub fn load(dirs: &[PathBuf]) -> Result<Apis, String> {
    let mut missing = vec![];
    let bass_p = find(dirs, "bass.dll");
    let mix_p = find(dirs, "bassmix.dll");
    let was_p = find(dirs, "basswasapi.dll");
    for (p, n) in [(&bass_p, "bass.dll"), (&mix_p, "bassmix.dll"), (&was_p, "basswasapi.dll")] {
        if p.is_none() {
            missing.push(n);
        }
    }
    if !missing.is_empty() {
        return Err(format!("Module(s) manquant(s) : {}", missing.join(", ")));
    }
    let bass = Bass::load(&Library::open(bass_p.as_ref().unwrap())?)?;
    let mix = Mix::load(&Library::open(mix_p.as_ref().unwrap())?)?;
    let wasapi = Wasapi::load(&Library::open(was_p.as_ref().unwrap())?)?;

    unsafe {
        (bass.BASS_SetConfig)(consts::BASS_CONFIG_UNICODE, 1);
        if (bass.BASS_Init)(0, 48000, 0, std::ptr::null_mut(), std::ptr::null()) == 0 {
            let code = (bass.BASS_ErrorGetCode)();
            if code != 14 {
                return Err(format!("Initialisation de BASS impossible (erreur {code})"));
            }
        }
    }

    let mut flac_plugin = false;
    if let Some(p) = find(dirs, "bassflac.dll") {
        let w = wide(p.as_os_str());
        flac_plugin = unsafe { (bass.BASS_PluginLoad)(w.as_ptr() as *const c_void, consts::BASS_UNICODE) } != 0;
    }

    let fx = find(dirs, "bass_fx.dll").and_then(|p| Library::open(&p).ok()).and_then(|l| Fx::load(&l).ok());

    // bassenc_opus.dll a besoin de bassenc.dll : on la charge d'abord.
    let enc_opus = find(dirs, "bassenc.dll")
        .and_then(|p| Library::open(&p).ok())
        .and_then(|_| find(dirs, "bassenc_opus.dll"))
        .and_then(|p| Library::open(&p).ok())
        .and_then(|l| EncOpus::load(&l).ok());

    Ok(Apis { bass, mix, wasapi, flac_plugin, fx, enc_opus })
}

pub fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

pub fn error_text(code: i32) -> String {
    match code {
        2 => "Impossible d'ouvrir le fichier".into(),
        6 => "Format non pris en charge par la sortie".into(),
        8 => "Moteur audio non initialisé".into(),
        23 => "Périphérique introuvable".into(),
        37 => "Action indisponible".into(),
        41 => "Format de fichier non pris en charge".into(),
        44 => "Codec indisponible".into(),
        46 => "Le périphérique est occupé (une autre application l'utilise peut-être en mode exclusif)".into(),
        _ => format!("Erreur BASS {code}"),
    }
}
