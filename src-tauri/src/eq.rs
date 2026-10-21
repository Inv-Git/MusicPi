//! Égaliseur graphique 10 bandes (filtres en cloche RBJ), appliqué en DSP sur
//! le mélangeur. Calcul en double précision ; aucun traitement quand il est
//! désactivé ou à plat, pour ne pas toucher au signal.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

pub const BANDS: [f64; 10] = [32.0, 64.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
/// Une octave de largeur par bande.
const Q: f64 = 1.41;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct EqParams {
    pub enabled: bool,
    /// Pré-amplification en dB (négative pour éviter la saturation quand on monte des bandes).
    pub preamp: f32,
    /// Gain de chaque bande en dB, de -12 à +12.
    pub gains: [f32; 10],
    pub preset: String,
}

impl Default for EqParams {
    fn default() -> Self {
        Self { enabled: false, preamp: 0.0, gains: [0.0; 10], preset: "Plat".into() }
    }
}

impl EqParams {
    /// L'égaliseur modifie-t-il réellement le signal ?
    pub fn active(&self) -> bool {
        self.enabled && (self.preamp.abs() > 0.01 || self.gains.iter().any(|g| g.abs() > 0.01))
    }
}

static PARAMS: Mutex<Option<EqParams>> = Mutex::new(None);
static VERSION: AtomicU64 = AtomicU64::new(1);

pub fn set(p: EqParams) {
    *PARAMS.lock().unwrap() = Some(p);
    VERSION.fetch_add(1, Ordering::Release);
}

pub fn get() -> EqParams {
    PARAMS.lock().unwrap().clone().unwrap_or_default()
}

#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Biquad {
    fn peaking(fs: f64, f0: f64, gain_db: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f64::consts::PI * f0 / fs;
        let alpha = w0.sin() / (2.0 * Q);
        let cos = w0.cos();
        let a0 = 1.0 + alpha / a;
        Biquad {
            b0: (1.0 + alpha * a) / a0,
            b1: (-2.0 * cos) / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha / a) / a0,
        }
    }
}

/// État du DSP pour un mélangeur donné (fréquence et canaux fixes).
pub struct EqDsp {
    fs: f64,
    chans: usize,
    version: u64,
    active: bool,
    preamp: f64,
    filters: Vec<Biquad>,
    /// Mémoire des filtres : [canal][bande] = (z1, z2).
    z: Vec<Vec<(f64, f64)>>,
}

impl EqDsp {
    pub fn new(freq: u32, chans: u32) -> Box<Self> {
        let mut d = Box::new(EqDsp { fs: freq as f64, chans: chans.max(1) as usize, version: 0, active: false, preamp: 1.0, filters: vec![], z: vec![] });
        d.refresh(true);
        d
    }

    fn refresh(&mut self, force: bool) {
        let v = VERSION.load(Ordering::Acquire);
        if v == self.version && !force {
            return;
        }
        // Jamais de blocage dans le thread audio : on réessaiera au bloc suivant.
        let Ok(guard) = PARAMS.try_lock() else { return };
        let p = guard.clone().unwrap_or_default();
        drop(guard);
        self.version = v;
        self.active = p.active();
        self.preamp = 10f64.powf(p.preamp as f64 / 20.0);
        let nyquist = self.fs / 2.0;
        let filters: Vec<Biquad> = BANDS
            .iter()
            .zip(p.gains.iter())
            .filter(|(f, g)| g.abs() > 0.01 && **f < nyquist * 0.9)
            .map(|(f, g)| Biquad::peaking(self.fs, *f, *g as f64))
            .collect();
        if filters.len() != self.filters.len() {
            self.z = vec![vec![(0.0, 0.0); filters.len()]; self.chans];
        }
        self.filters = filters;
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        self.refresh(false);
        if !self.active {
            return;
        }
        let ch = self.chans;
        for (i, s) in buf.iter_mut().enumerate() {
            let c = i % ch;
            let mut x = *s as f64 * self.preamp;
            for (f, z) in self.filters.iter().zip(self.z[c].iter_mut()) {
                let y = f.b0 * x + z.0;
                z.0 = f.b1 * x - f.a1 * y + z.1;
                z.1 = f.b2 * x - f.a2 * y;
                x = y;
            }
            *s = x as f32;
        }
    }
}

/// Callback DSP de BASS (DSPPROC). `user` pointe sur un `EqDsp`.
pub unsafe extern "system" fn dsp_proc(_dsp: u32, _channel: u32, buffer: *mut c_void, length: u32, user: *mut c_void) {
    if user.is_null() || buffer.is_null() {
        return;
    }
    let d = &mut *(user as *mut EqDsp);
    let s = std::slice::from_raw_parts_mut(buffer as *mut f32, length as usize / 4);
    d.process(s);
}
