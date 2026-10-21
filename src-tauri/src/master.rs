//! Dernier étage avant la sortie : fondus (pause / reprise), mono et balance.
//! Réglages partagés en atomiques : jamais de verrou dans le thread audio.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

static TARGET: AtomicU32 = AtomicU32::new(0x3F80_0000); // 1.0
static FADE_MS: AtomicU32 = AtomicU32::new(0);
/// Saut immédiat demandé : (numéro, valeur).
static JUMP_SEQ: AtomicU64 = AtomicU64::new(0);
static JUMP_VAL: AtomicU32 = AtomicU32::new(0x3F80_0000);
static MONO: AtomicBool = AtomicBool::new(false);
static BALANCE: AtomicU32 = AtomicU32::new(0); // 0.0

fn f(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Acquire))
}

/// Va vers `gain` en `ms` millisecondes.
pub fn fade_to(gain: f32, ms: u32) {
    FADE_MS.store(ms, Ordering::Release);
    TARGET.store(gain.to_bits(), Ordering::Release);
}

/// Se place tout de suite sur `gain` (et y reste).
pub fn jump(gain: f32) {
    JUMP_VAL.store(gain.to_bits(), Ordering::Release);
    TARGET.store(gain.to_bits(), Ordering::Release);
    JUMP_SEQ.fetch_add(1, Ordering::AcqRel);
}

pub fn set_mono(on: bool) {
    MONO.store(on, Ordering::Release);
}

/// -1 = tout à gauche, 0 = centre, 1 = tout à droite.
pub fn set_balance(b: f32) {
    BALANCE.store(b.clamp(-1.0, 1.0).to_bits(), Ordering::Release);
}

/// Le signal est-il modifié (hors fondu en cours) ?
pub fn active() -> bool {
    MONO.load(Ordering::Acquire) || f(&BALANCE).abs() > 0.001
}

pub struct MasterDsp {
    fs: f32,
    chans: usize,
    gain: f32,
    jump_seq: u64,
}

impl MasterDsp {
    pub fn new(freq: u32, chans: u32) -> Box<Self> {
        Box::new(MasterDsp { fs: freq.max(1) as f32, chans: chans.max(1) as usize, gain: f(&TARGET), jump_seq: JUMP_SEQ.load(Ordering::Acquire) })
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        let seq = JUMP_SEQ.load(Ordering::Acquire);
        if seq != self.jump_seq {
            self.jump_seq = seq;
            self.gain = f(&JUMP_VAL);
        }
        let target = f(&TARGET);
        let ms = FADE_MS.load(Ordering::Acquire);
        let step = if ms == 0 { 1.0 } else { 1000.0 / (ms as f32 * self.fs) };
        let mono = MONO.load(Ordering::Acquire);
        let bal = f(&BALANCE);
        let ch = self.chans;
        let (lg, rg) = if bal < 0.0 { (1.0, 1.0 + bal) } else { (1.0 - bal, 1.0) };
        let shaping = mono || bal.abs() > 0.001;
        if (self.gain - target).abs() < 1e-6 && (self.gain - 1.0).abs() < 1e-6 && !shaping {
            return;
        }
        for frame in buf.chunks_mut(ch) {
            if self.gain != target {
                if self.gain < target {
                    self.gain = (self.gain + step).min(target);
                } else {
                    self.gain = (self.gain - step).max(target);
                }
            }
            if ch >= 2 && shaping {
                if mono {
                    let m = (frame[0] + frame[1]) * 0.5;
                    frame[0] = m;
                    frame[1] = m;
                }
                frame[0] *= lg;
                frame[1] *= rg;
            }
            if self.gain != 1.0 {
                for s in frame.iter_mut() {
                    *s *= self.gain;
                }
            }
        }
    }
}

/// Callback DSP de BASS. `user` pointe sur un `MasterDsp`.
pub unsafe extern "system" fn dsp_proc(_dsp: u32, _channel: u32, buffer: *mut c_void, length: u32, user: *mut c_void) {
    if user.is_null() || buffer.is_null() {
        return;
    }
    let d = &mut *(user as *mut MasterDsp);
    let s = std::slice::from_raw_parts_mut(buffer as *mut f32, length as usize / 4);
    d.process(s);
}

#[cfg(test)]
mod tests {
    use super::*;

    // Les réglages sont globaux : un seul test, enchaîné.
    #[test]
    fn fondus_mono_balance() {
        jump(1.0);
        set_mono(false);
        set_balance(0.0);
        let mut d = MasterDsp::new(1000, 2);
        let mut b = vec![0.5f32; 20];
        d.process(&mut b);
        assert!(b.iter().all(|&x| x == 0.5), "intact à plat");

        // Fondu de 1 s à 1000 Hz : -0,001 par trame.
        fade_to(0.0, 1000);
        let mut b = vec![1.0f32; 2000]; // 1000 trames
        d.process(&mut b);
        assert!((b[0] - 0.999).abs() < 1e-4, "{}", b[0]);
        assert!(b[1998].abs() < 1e-3);
        let mut b = vec![1.0f32; 10];
        d.process(&mut b);
        assert!(b.iter().all(|&x| x == 0.0));

        jump(0.0);
        fade_to(1.0, 0);
        let mut b = vec![1.0f32; 4];
        d.process(&mut b);
        assert_eq!(b, vec![1.0; 4], "fade de 0 ms = immédiat");

        set_mono(true);
        let mut b = vec![1.0f32, 0.0, 0.0, 1.0];
        d.process(&mut b);
        assert_eq!(b, vec![0.5, 0.5, 0.5, 0.5]);
        set_mono(false);

        set_balance(-0.5);
        let mut b = vec![1.0f32, 1.0];
        d.process(&mut b);
        assert_eq!(b, vec![1.0, 0.5]);
        set_balance(0.0);
        assert!(!active());
    }
}
