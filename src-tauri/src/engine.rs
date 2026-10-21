//! Moteur de lecture.
//!
//! Chaîne : fichier → flux de décodage BASS (flottant 32 bits) → mélangeur BASSmix
//! → sortie BASSWASAPI (exclusive par défaut).
//!
//! En mode exclusif, la sortie s'ouvre à la fréquence du fichier quand le
//! périphérique l'accepte : avec le volume à 100 %, les échantillons arrivent
//! intacts au DAC (un 24 bits tient exactement dans un flottant 32 bits).
//! Sinon, BASSmix rééchantillonne avec son meilleur filtre.
//!
//! Gapless : la piste suivante est préparée à l'avance. Si elle a le même format
//! de sortie, elle est branchée sur le mélangeur au moment exact où la piste en
//! cours se termine (synchro « mixtime »). Sinon, la sortie est rouverte au bon
//! format une fois le tampon vidé.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::bass::{self, consts::*, Apis, ChannelInfo, MixerNode, WasapiDeviceInfo, WasapiInfo};
use crate::{eq, master, tags};

static API: OnceLock<Apis> = OnceLock::new();
/// Accès aux fonctions BASS hors du moteur (conversion pour le téléphone).
pub fn api() -> Option<&'static Apis> {
    API.get()
}

/// Convertit un fichier en Opus (`kbps` kbit/s) : version légère pour le streaming en 4G.
pub fn transcode_opus(src: &Path, dest: &Path, kbps: u32) -> Result<(), String> {
    let a = API.get().ok_or("Moteur audio indisponible")?;
    let enc = a.enc_opus.as_ref().ok_or("Module BASSenc_OPUS absent")?;
    let tmp = dest.with_extension("part");
    unsafe {
        let w = bass::wide(src.as_os_str());
        let h = (a.bass.BASS_StreamCreateFile)(BASS_FILE_NAME, w.as_ptr() as *const c_void, 0, 0, BASS_STREAM_DECODE | BASS_SAMPLE_FLOAT | BASS_UNICODE);
        if h == 0 {
            return Err(bass::error_text((a.bass.BASS_ErrorGetCode)()));
        }
        let opts = bass::wide(std::ffi::OsStr::new(&format!("--bitrate {kbps}")));
        let out = bass::wide(tmp.as_os_str());
        let e = (enc.BASS_Encode_OPUS_StartFile)(h, opts.as_ptr() as *const c_void, BASS_ENCODE_AUTOFREE | BASS_UNICODE, out.as_ptr() as *const c_void);
        if e == 0 {
            let err = bass::error_text((a.bass.BASS_ErrorGetCode)());
            (a.bass.BASS_StreamFree)(h);
            return Err(err);
        }
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = (a.bass.BASS_ChannelGetData)(h, buf.as_mut_ptr() as *mut c_void, buf.len() as u32);
            if n == u32::MAX || n == 0 {
                break;
            }
        }
        // Libérer le flux ferme l'encodeur (AUTOFREE) et finalise le fichier.
        (a.bass.BASS_StreamFree)(h);
    }
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())
}

/// Diagnostic du fondu enchaîné : nombre de déclenchements, et résultat des enveloppes.
static XFADE_FIRED: AtomicU32 = AtomicU32::new(0);
static XFADE_ENV_OK: AtomicU32 = AtomicU32::new(0);
static SHARED: OnceLock<Shared> = OnceLock::new();

struct Shared {
    tx: Sender<Msg>,
    mixer: AtomicU32,
    /// Piste en cours dans le mélangeur (celle qui « mène »).
    cur: AtomicU32,
    next: Mutex<Option<Pending>>,
}

#[derive(Clone, Copy)]
struct Pending {
    handle: u32,
    index: usize,
    gen: u64,
    /// Piste après laquelle celle-ci doit venir.
    after: u32,
    /// Durée du fondu enchaîné, en octets du mélangeur (0 = enchaînement sans blanc).
    xfade: u64,
    /// Facteur ReplayGain appliqué.
    rg: Option<f32>,
}

/// Réglages de lecture et de sortie (Paramètres › Lecture et › Sortie).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AudioOpts {
    /// « off », « track » ou « album ».
    pub replaygain: String,
    /// Pré-amplification ReplayGain, en dB.
    pub rg_preamp: f32,
    /// Ne jamais dépasser la crête du fichier (pas de saturation).
    pub rg_clip: bool,
    /// Fondu enchaîné entre deux titres, en secondes (0 = enchaînement sans blanc).
    pub crossfade: f32,
    /// Fondre aussi entre deux titres d'un même album (sinon : sans blanc, comme le disque).
    pub crossfade_albums: bool,
    /// Fondu à la pause et à la reprise, en millisecondes.
    pub fade_ms: u32,
    /// Volume maximum autorisé (0..1).
    pub max_volume: f32,
    pub mono: bool,
    /// -1 (gauche) à 1 (droite).
    pub balance: f32,
    /// Tampon de sortie, en millisecondes.
    pub buffer_ms: u32,
    /// Fréquence imposée en mode exclusif (0 = celle du fichier).
    pub exclusive_freq: u32,
    /// Fin de la file : « stop », « repeat » ou « radio ».
    pub end_mode: String,
}

impl Default for AudioOpts {
    fn default() -> Self {
        AudioOpts {
            replaygain: "off".into(),
            rg_preamp: 0.0,
            rg_clip: true,
            crossfade: 0.0,
            crossfade_albums: false,
            fade_ms: 150,
            max_volume: 1.0,
            mono: false,
            balance: 0.0,
            buffer_ms: 300,
            exclusive_freq: 0,
            end_mode: "stop".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Types publics
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct OutputPref {
    /// Identifiant WASAPI du périphérique ; `None` = périphérique par défaut.
    pub device_id: Option<String>,
    pub exclusive: bool,
}

impl Default for OutputPref {
    fn default() -> Self {
        Self { device_id: None, exclusive: false }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDesc {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub mix_freq: u32,
    #[serde(skip)]
    pub mix_chans: u32,
    #[serde(skip)]
    pub index: i32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalPath {
    pub source: String,
    pub output: String,
    pub device: String,
    pub shared: bool,
    pub bit_perfect: bool,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub ready: bool,
    pub index: Option<usize>,
    pub path: Option<String>,
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
    pub volume: f32,
    pub signal: Option<SignalPath>,
    pub error: Option<String>,
    /// Mode travail : vitesse réglable et boucle.
    pub practice: bool,
    /// Changement de vitesse en % (−25 = 75 % de la vitesse d'origine).
    pub tempo: f32,
    pub tempo_available: bool,
    /// Boucle [début, fin] en secondes.
    pub loop_range: Option<(f64, f64)>,
    /// La file est terminée (pour enchaîner sur d'autres titres si demandé).
    pub finished: bool,
}

pub enum Cmd {
    Play { queue: Vec<PathBuf>, index: usize },
    Toggle,
    Stop,
    Next,
    Prev,
    Seek(f64),
    SetVolume(f32),
    SetOutput(OutputPref),
    Devices(Sender<Vec<DeviceDesc>>),
    SetPractice(bool),
    SetTempo(f32),
    SetLoop(Option<(f64, f64)>),
    SetAudio(AudioOpts),
    /// Reprise au démarrage : file et position, sans lancer la lecture.
    Restore { queue: Vec<PathBuf>, index: usize, position: f64 },
}

enum Msg {
    Cmd(Cmd),
    Advanced(Pending),
    Ended(u64),
    /// Changement côté Windows : périphérique par défaut, débranché, en échec…
    DeviceChange(u32, u32),
}

#[derive(Clone)]
pub struct EngineHandle {
    tx: Sender<Msg>,
}

impl EngineHandle {
    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(Msg::Cmd(cmd));
    }

    pub fn devices(&self) -> Vec<DeviceDesc> {
        let (t, r) = mpsc::channel();
        self.send(Cmd::Devices(t));
        r.recv_timeout(Duration::from_secs(3)).unwrap_or_default()
    }
}

pub type Emit = Box<dyn Fn(&PlayerState) + Send>;

pub fn start(lib_dirs: Vec<PathBuf>, pref: OutputPref, volume: f32, opts: AudioOpts, emit: Emit) -> EngineHandle {
    let (tx, rx) = mpsc::channel::<Msg>();
    let _ = SHARED.set(Shared { tx: tx.clone(), mixer: AtomicU32::new(0), cur: AtomicU32::new(0), next: Mutex::new(None) });
    master::set_mono(opts.mono);
    master::set_balance(opts.balance);

    std::thread::Builder::new()
        .name("moteur-audio".into())
        .spawn(move || {
            let load_error = match bass::load(&lib_dirs) {
                Ok(a) => {
                    let _ = API.set(a);
                    if let Some(a) = API.get() {
                        unsafe { (a.wasapi.BASS_WASAPI_SetNotify)(wasapi_notify, std::ptr::null_mut()) };
                    }
                    None
                }
                Err(e) => Some(e),
            };
            let mut eng = Engine::new(pref, volume, opts, emit, load_error);
            eng.emit();
            loop {
                let wait = if eng.playing { Duration::from_millis(200) } else { Duration::from_millis(1000) };
                match rx.recv_timeout(wait) {
                    Ok(m) => eng.handle(m),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
                eng.tick();
                eng.emit();
            }
        })
        .expect("thread audio");

    EngineHandle { tx }
}

// ---------------------------------------------------------------------------
// Callbacks appelés par BASS (thread audio)
// ---------------------------------------------------------------------------

unsafe extern "system" fn wasapi_proc(buffer: *mut c_void, length: u32, _user: *mut c_void) -> u32 {
    let (Some(a), Some(sh)) = (API.get(), SHARED.get()) else {
        std::ptr::write_bytes(buffer as *mut u8, 0, length as usize);
        return length;
    };
    let mixer = sh.mixer.load(Ordering::Acquire);
    let got = if mixer != 0 { (a.bass.BASS_ChannelGetData)(mixer, buffer, length) } else { u32::MAX };
    let got = if got == u32::MAX { 0 } else { got.min(length) };
    if got < length {
        std::ptr::write_bytes((buffer as *mut u8).add(got as usize), 0, (length - got) as usize);
    }
    length
}

/// Fin de boucle atteinte (au moment du mixage) : retour au début de la boucle.
unsafe extern "system" fn on_loop_end(_sync: u32, channel: u32, _data: u32, user: *mut c_void) {
    if let Some(a) = API.get() {
        (a.mix.BASS_Mixer_ChannelSetPosition)(channel, user as usize as u64, BASS_POS_BYTE);
    }
}

/// Notifications de BASSWASAPI (thread interne) : relayées au moteur.
unsafe extern "system" fn wasapi_notify(notify: u32, device: u32, _user: *mut c_void) {
    if let Some(sh) = SHARED.get() {
        let _ = sh.tx.send(Msg::DeviceChange(notify, device));
    }
}

/// Fin de piste, appelée au moment du mixage : branche la suivante sans blanc.
unsafe extern "system" fn on_end(_sync: u32, channel: u32, _data: u32, user: *mut c_void) {
    let gen = user as usize as u64;
    let (Some(a), Some(sh)) = (API.get(), SHARED.get()) else { return };
    // Piste qui finit son fondu enchaîné : la suivante a déjà pris le relais.
    if sh.cur.load(Ordering::Acquire) != channel {
        return;
    }
    let pending = sh.next.lock().ok().and_then(|mut g| g.take());
    if let Some(p) = pending {
        let mixer = sh.mixer.load(Ordering::Acquire);
        if p.gen == gen && mixer != 0 && (a.mix.BASS_Mixer_StreamAddChannel)(mixer, p.handle, source_flags()) != 0 {
            (a.mix.BASS_Mixer_ChannelSetSync)(p.handle, BASS_SYNC_END | BASS_SYNC_MIXTIME, 0, on_end, p.gen as usize as *mut c_void);
            sh.cur.store(p.handle, Ordering::Release);
            let _ = sh.tx.send(Msg::Advanced(p));
            return;
        }
        (a.bass.BASS_StreamFree)(p.handle);
    }
    let _ = sh.tx.send(Msg::Ended(gen));
}

/// Début du fondu enchaîné (au moment du mixage) : la suivante monte, celle-ci descend.
unsafe extern "system" fn on_xfade(_sync: u32, channel: u32, _data: u32, _user: *mut c_void) {
    let (Some(a), Some(sh)) = (API.get(), SHARED.get()) else { return };
    if sh.cur.load(Ordering::Acquire) != channel {
        return;
    }
    let p = {
        let Ok(mut g) = sh.next.lock() else { return };
        match *g {
            Some(p) if p.after == channel && p.xfade > 0 => g.take().unwrap(),
            _ => return,
        }
    };
    let mixer = sh.mixer.load(Ordering::Acquire);
    if mixer == 0 || (a.mix.BASS_Mixer_StreamAddChannel)(mixer, p.handle, source_flags()) == 0 {
        (a.bass.BASS_StreamFree)(p.handle);
        return;
    }
    let up = [MixerNode { pos: 0, value: 0.0 }, MixerNode { pos: p.xfade, value: 1.0 }];
    let down = [MixerNode { pos: 0, value: 1.0 }, MixerNode { pos: p.xfade, value: 0.0 }];
    let ok1 = (a.mix.BASS_Mixer_ChannelSetEnvelope)(p.handle, BASS_MIXER_ENV_VOL, up.as_ptr(), 2);
    (a.mix.BASS_Mixer_ChannelSetEnvelopePos)(p.handle, BASS_MIXER_ENV_VOL, 0);
    let ok2 = (a.mix.BASS_Mixer_ChannelSetEnvelope)(channel, BASS_MIXER_ENV_VOL, down.as_ptr(), 2);
    (a.mix.BASS_Mixer_ChannelSetEnvelopePos)(channel, BASS_MIXER_ENV_VOL, 0);
    XFADE_FIRED.fetch_add(1, Ordering::AcqRel);
    XFADE_ENV_OK.store(((ok1 != 0) as u32) | (((ok2 != 0) as u32) << 1), Ordering::Release);
    (a.mix.BASS_Mixer_ChannelSetSync)(p.handle, BASS_SYNC_END | BASS_SYNC_MIXTIME, 0, on_end, p.gen as usize as *mut c_void);
    sh.cur.store(p.handle, Ordering::Release);
    let _ = sh.tx.send(Msg::Advanced(p));
}

fn source_flags() -> u32 {
    BASS_STREAM_AUTOFREE | BASS_MIXER_CHAN_NORAMPIN
}

// ---------------------------------------------------------------------------
// Moteur
// ---------------------------------------------------------------------------

struct Source {
    handle: u32,
    /// Flux de décodage d'origine (sous le module de vitesse éventuel).
    base: u32,
    /// Facteur ReplayGain appliqué.
    rg: Option<f32>,
    /// Le flux passe par le module de vitesse (BASS_FX).
    tempo: bool,
    freq: u32,
    chans: u32,
    bits: u32,
    float: bool,
    duration: f64,
    format: String,
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct OutCfg {
    device: i32,
    freq: u32,
    chans: u32,
    exclusive: bool,
}

#[derive(Clone, Copy)]
struct ActiveOut {
    cfg: OutCfg,
    freq: u32,
    chans: u32,
    format: u32,
}

struct Engine {
    emit_fn: Emit,
    load_error: Option<String>,
    pref: OutputPref,
    volume: f32,
    queue: Vec<PathBuf>,
    index: Option<usize>,
    cur: Option<Source>,
    mixer: u32,
    /// DSP égaliseur attaché au mélangeur (Box<eq::EqDsp> sous forme de pointeur).
    eq_dsp: usize,
    out: Option<ActiveOut>,
    device_name: String,
    gen: u64,
    playing: bool,
    error: Option<String>,
    /// Fin de piste détectée au décodage : on attend que le tampon se vide.
    drain_until: Option<Instant>,
    /// Réouverture de la sortie programmée (changement de périphérique Windows).
    reopen_at: Option<Instant>,
    practice: bool,
    tempo: f32,
    loop_range: Option<(f64, f64)>,
    loop_sync: u32,
    /// Durée de la dernière piste, pour l'affichage pendant la vidange.
    last_duration: f64,
    opts: AudioOpts,
    /// DSP de sortie (fondus, mono, balance) : Box<master::MasterDsp> en pointeur.
    master_dsp: usize,
    /// Synchro du fondu enchaîné posée sur la piste en cours.
    xfade_sync: u32,
    finished: bool,
    /// Explication affichée dans le détail du signal (fondu prévu, ou pourquoi pas).
    xfade_note: Option<String>,
}

fn khz(f: u32) -> String {
    if f % 1000 == 0 {
        format!("{} kHz", f / 1000)
    } else {
        format!("{:.1} kHz", f as f64 / 1000.0).replace('.', ",")
    }
}

impl Engine {
    fn new(pref: OutputPref, volume: f32, opts: AudioOpts, emit_fn: Emit, load_error: Option<String>) -> Self {
        Engine {
            emit_fn,
            load_error,
            pref,
            volume: volume.clamp(0.0, opts.max_volume.clamp(0.0, 1.0)),
            queue: vec![],
            index: None,
            cur: None,
            mixer: 0,
            eq_dsp: 0,
            out: None,
            device_name: String::new(),
            gen: 1,
            playing: false,
            error: None,
            drain_until: None,
            reopen_at: None,
            practice: false,
            tempo: 0.0,
            loop_range: None,
            loop_sync: 0,
            last_duration: 0.0,
            opts,
            master_dsp: 0,
            xfade_sync: 0,
            finished: false,
            xfade_note: None,
        }
    }

    fn api(&self) -> Option<&'static Apis> {
        API.get()
    }

    fn last_error(&self) -> String {
        match self.api() {
            Some(a) => bass::error_text(unsafe { (a.bass.BASS_ErrorGetCode)() }),
            None => "Moteur audio indisponible".into(),
        }
    }

    // --- messages ---------------------------------------------------------

    fn handle(&mut self, m: Msg) {
        if self.api().is_none() {
            if let Msg::Cmd(Cmd::Devices(t)) = m {
                let _ = t.send(vec![]);
            }
            return;
        }
        match m {
            Msg::Cmd(c) => self.command(c),
            Msg::Advanced(p) => self.advanced(p),
            Msg::Ended(gen) => self.ended(gen),
            Msg::DeviceChange(n, dev) => self.device_change(n, dev),
        }
    }

    fn command(&mut self, c: Cmd) {
        match c {
            Cmd::Play { queue, index } => {
                self.queue = queue;
                self.loop_range = None;
                if index < self.queue.len() {
                    self.load(index, true, 0.0);
                }
            }
            Cmd::Toggle => self.toggle(),
            Cmd::Stop => self.stop(),
            Cmd::Next => match self.index {
                Some(i) if i + 1 < self.queue.len() => self.load(i + 1, true, 0.0),
                _ => self.stop(),
            },
            Cmd::Prev => match self.index {
                Some(i) if self.position() > 3.0 || i == 0 => self.load(i, self.playing || self.cur.is_none(), 0.0),
                Some(i) => self.load(i - 1, true, 0.0),
                None => {}
            },
            Cmd::Seek(s) => self.seek(s),
            Cmd::SetVolume(v) => {
                self.volume = v.clamp(0.0, self.opts.max_volume.clamp(0.0, 1.0));
                if self.mixer != 0 {
                    let a = self.api().unwrap();
                    unsafe { (a.bass.BASS_ChannelSetAttribute)(self.mixer, BASS_ATTRIB_VOLDSP, self.volume) };
                }
            }
            Cmd::SetOutput(p) => self.set_output(p),
            Cmd::Devices(t) => {
                let _ = t.send(self.devices());
            }
            Cmd::SetPractice(on) => {
                if on != self.practice {
                    self.practice = on;
                    // La chaîne change (ajout ou retrait du module de vitesse) : on recharge la piste.
                    if self.api().map(|a| a.fx.is_some()).unwrap_or(false) {
                        self.reload_here();
                    }
                }
            }
            Cmd::SetTempo(t) => {
                self.tempo = t.clamp(-50.0, 50.0);
                if let (Some(a), Some(c)) = (self.api(), &self.cur) {
                    if self.practice && c.tempo {
                        unsafe { (a.bass.BASS_ChannelSetAttribute)(c.handle, BASS_ATTRIB_TEMPO, self.tempo) };
                    }
                }
            }
            Cmd::SetLoop(r) => {
                self.loop_range = r.filter(|(a, b)| b > a);
                self.apply_loop(true);
                // Pas de fondu enchaîné pendant une boucle.
                if self.cur.is_some() {
                    self.prepare_next();
                }
            }
            Cmd::SetAudio(o) => self.set_audio(o),
            Cmd::Restore { queue, index, position } => {
                self.queue = queue;
                if index < self.queue.len() {
                    self.load(index, false, position.max(0.0));
                }
            }
        }
    }

    fn advanced(&mut self, p: Pending) {
        let a = self.api().unwrap();
        if p.gen != self.gen {
            unsafe { (a.bass.BASS_StreamFree)(p.handle) };
            return;
        }
        if let Some(mut src) = self.describe(p.handle, &self.queue[p.index].clone()) {
            src.rg = p.rg;
            self.cur = Some(src);
        }
        self.xfade_sync = 0;
        self.index = Some(p.index);
        self.loop_range = None;
        self.loop_sync = 0;
        self.prepare_next();
    }

    fn ended(&mut self, gen: u64) {
        if gen != self.gen {
            return;
        }
        // Le décodage est fini mais le tampon de sortie joue encore : on attend.
        let wait = self.buffered_secs() + 0.05;
        if let Some(c) = &self.cur {
            self.last_duration = c.duration;
        }
        self.drain_until = Some(Instant::now() + Duration::from_secs_f64(wait));
    }

    fn device_change(&mut self, notify: u32, device: u32) {
        let ours = self.out.map(|o| o.cfg.device == device as i32).unwrap_or(false);
        let follow_default = self.pref.device_id.is_none();
        let relevant = match notify {
            // Le périphérique par défaut de Windows a changé : on le suit si on n'en a pas choisi un.
            BASS_WASAPI_NOTIFY_DEFOUTPUT => follow_default,
            // Notre périphérique a été débranché ou est en échec.
            BASS_WASAPI_NOTIFY_DISABLED | BASS_WASAPI_NOTIFY_FAIL => ours,
            _ => false,
        };
        if relevant {
            // Windows envoie souvent plusieurs notifications d'affilée : on attend un peu.
            self.reopen_at = Some(Instant::now() + Duration::from_millis(400));
        }
    }

    /// Rouvre la sortie (sur le nouveau périphérique) en reprenant la lecture là où elle en était.
    fn reopen(&mut self) {
        let pos = self.position();
        let playing = self.playing;
        let had_track = self.cur.is_some();
        self.teardown();
        self.close_output();
        if let (true, Some(i)) = (had_track, self.index) {
            self.load(i, playing, pos);
        }
    }

    fn tick(&mut self) {
        if let Some(t) = self.reopen_at {
            if Instant::now() >= t {
                self.reopen_at = None;
                self.reopen();
            }
        }
        if let Some(t) = self.drain_until {
            if Instant::now() >= t {
                self.drain_until = None;
                match self.index {
                    Some(i) if i + 1 < self.queue.len() => self.load(i + 1, true, 0.0),
                    _ if self.opts.end_mode == "repeat" && !self.queue.is_empty() => self.load(0, true, 0.0),
                    _ => {
                        self.stop();
                        self.finished = true;
                    }
                }
            }
        }
    }

    // --- sortie -------------------------------------------------------------

    fn devices(&self) -> Vec<DeviceDesc> {
        let Some(a) = self.api() else { return vec![] };
        let mut v = vec![];
        for i in 0..256u32 {
            let mut info: WasapiDeviceInfo = unsafe { std::mem::zeroed() };
            if unsafe { (a.wasapi.BASS_WASAPI_GetDeviceInfo)(i, &mut info) } == 0 {
                break;
            }
            let f = info.flags;
            if f & (BASS_DEVICE_INPUT | BASS_DEVICE_LOOPBACK) != 0 || f & BASS_DEVICE_ENABLED == 0 {
                continue;
            }
            v.push(DeviceDesc {
                id: bass::cstr(info.id),
                name: bass::cstr(info.name),
                is_default: f & BASS_DEVICE_DEFAULT != 0,
                mix_freq: info.mixfreq,
                mix_chans: info.mixchans,
                index: i as i32,
            });
        }
        v
    }

    fn resolve_device(&self) -> Option<DeviceDesc> {
        let list = self.devices();
        if let Some(id) = &self.pref.device_id {
            if let Some(d) = list.iter().find(|d| &d.id == id) {
                return Some(d.clone());
            }
        }
        list.iter().find(|d| d.is_default).or(list.first()).cloned()
    }

    fn supports(&self, dev: i32, freq: u32, chans: u32) -> bool {
        let a = self.api().unwrap();
        unsafe { (a.wasapi.BASS_WASAPI_CheckFormat)(dev, freq, chans, BASS_WASAPI_EXCLUSIVE) != u32::MAX }
    }

    /// Choisit le format de sortie pour une source donnée.
    fn plan(&mut self, src: &Source) -> Result<OutCfg, String> {
        let dev = self.resolve_device().ok_or("Aucun périphérique de sortie")?;
        self.device_name = dev.name.clone();
        if !self.pref.exclusive {
            return Ok(OutCfg { device: dev.index, freq: dev.mix_freq, chans: dev.mix_chans.max(1), exclusive: false });
        }
        let mut chans_opts = vec![src.chans];
        if src.chans != 2 {
            chans_opts.push(2);
        }
        // 0. Fréquence imposée dans les réglages.
        if self.opts.exclusive_freq > 0 {
            for &ch in &chans_opts {
                if self.supports(dev.index, self.opts.exclusive_freq, ch) {
                    return Ok(OutCfg { device: dev.index, freq: self.opts.exclusive_freq, chans: ch, exclusive: true });
                }
            }
        }
        // 1. Fréquence d'origine : lecture bit-perfect.
        for &ch in &chans_opts {
            if self.supports(dev.index, src.freq, ch) {
                return Ok(OutCfg { device: dev.index, freq: src.freq, chans: ch, exclusive: true });
            }
        }
        // 2. Repli : même famille (44,1 / 48) en dessous, puis l'autre, puis au-dessus.
        let fam44 = src.freq % 11025 == 0;
        let (same, other): (&[u32], &[u32]) = if fam44 {
            (&[352800, 176400, 88200, 44100], &[384000, 192000, 96000, 48000])
        } else {
            (&[384000, 192000, 96000, 48000], &[352800, 176400, 88200, 44100])
        };
        let mut cands: Vec<u32> = vec![];
        cands.extend(same.iter().filter(|&&f| f < src.freq));
        cands.extend(other.iter().filter(|&&f| f < src.freq));
        cands.extend(same.iter().rev().filter(|&&f| f > src.freq));
        cands.extend(other.iter().rev().filter(|&&f| f > src.freq));
        for &ch in &chans_opts {
            for &f in &cands {
                if self.supports(dev.index, f, ch) {
                    return Ok(OutCfg { device: dev.index, freq: f, chans: ch, exclusive: true });
                }
            }
        }
        Err(format!("{} n'accepte aucun format en mode exclusif", dev.name))
    }

    fn ensure_output(&mut self, cfg: OutCfg) -> Result<ActiveOut, String> {
        if let Some(o) = self.out {
            if o.cfg == cfg {
                return Ok(o);
            }
        }
        self.close_output();
        let a = self.api().unwrap();
        let flags = if cfg.exclusive { BASS_WASAPI_EXCLUSIVE } else { BASS_WASAPI_AUTOFORMAT };
        let ok = unsafe {
            (a.wasapi.BASS_WASAPI_Init)(cfg.device, cfg.freq, cfg.chans, flags, self.opts.buffer_ms.clamp(50, 2000) as f32 / 1000.0, 0.0, wasapi_proc, std::ptr::null_mut())
        };
        if ok == 0 {
            return Err(self.last_error());
        }
        let mut info: WasapiInfo = unsafe { std::mem::zeroed() };
        unsafe { (a.wasapi.BASS_WASAPI_GetInfo)(&mut info) };
        let o = ActiveOut { cfg, freq: info.freq.max(1), chans: info.chans.max(1), format: info.format };
        self.out = Some(o);
        Ok(o)
    }

    fn close_output(&mut self) {
        if self.out.take().is_some() {
            let a = self.api().unwrap();
            unsafe {
                (a.wasapi.BASS_WASAPI_Stop)(1);
                (a.wasapi.BASS_WASAPI_Free)();
            }
        }
    }

    fn buffered_secs(&self) -> f64 {
        let (Some(a), Some(o)) = (self.api(), self.out) else { return 0.0 };
        let bytes = unsafe { (a.wasapi.BASS_WASAPI_GetData)(std::ptr::null_mut(), BASS_DATA_AVAILABLE) };
        if bytes == u32::MAX {
            return 0.0;
        }
        bytes as f64 / (4.0 * o.chans as f64 * o.freq as f64)
    }

    // --- pistes -------------------------------------------------------------

    fn open(&self, path: &Path) -> Result<Source, String> {
        let a = self.api().unwrap();
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let mut flags = BASS_STREAM_DECODE | BASS_SAMPLE_FLOAT | BASS_UNICODE;
        if ext == "mp3" {
            flags |= BASS_STREAM_PRESCAN;
        }
        let w = bass::wide(path.as_os_str());
        let h = unsafe { (a.bass.BASS_StreamCreateFile)(BASS_FILE_NAME, w.as_ptr() as *const c_void, 0, 0, flags) };
        if h == 0 {
            return Err(format!("{} : {}", path.file_name().unwrap_or_default().to_string_lossy(), self.last_error()));
        }
        let mut src = self.describe(h, path).ok_or_else(|| self.last_error())?;
        src.rg = self.apply_rg(h, path);
        Ok(src)
    }

    /// Volume ReplayGain du fichier, appliqué sur son flux de décodage.
    fn apply_rg(&self, h: u32, path: &Path) -> Option<f32> {
        let a = self.api().unwrap();
        let f = if self.opts.replaygain == "off" {
            None
        } else {
            tags::replaygain(path).factor(&self.opts.replaygain, self.opts.rg_preamp, self.opts.rg_clip)
        };
        unsafe { (a.bass.BASS_ChannelSetAttribute)(h, BASS_ATTRIB_VOLDSP, f.unwrap_or(1.0)) };
        f
    }

    fn describe(&self, h: u32, path: &Path) -> Option<Source> {
        let a = self.api().unwrap();
        let mut info: ChannelInfo = unsafe { std::mem::zeroed() };
        if unsafe { (a.bass.BASS_ChannelGetInfo)(h, &mut info) } == 0 {
            return None;
        }
        let len = unsafe { (a.bass.BASS_ChannelGetLength)(h, BASS_POS_BYTE) };
        let duration = if len == u64::MAX { 0.0 } else { unsafe { (a.bass.BASS_ChannelBytes2Seconds)(h, len) } };
        let format = path.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_else(|| "AUDIO".into());
        Some(Source {
            handle: h,
            base: h,
            rg: None,
            tempo: false,
            freq: info.freq,
            chans: info.chans,
            bits: info.origres & 0xFFFF,
            float: info.origres & BASS_ORIGRES_FLOAT != 0,
            duration,
            format,
        })
    }

    fn free_pending(&mut self) {
        let a = self.api().unwrap();
        if self.xfade_sync != 0 {
            if let Some(c) = &self.cur {
                unsafe { (a.mix.BASS_Mixer_ChannelRemoveSync)(c.handle, self.xfade_sync) };
            }
            self.xfade_sync = 0;
        }
        let p = SHARED.get().and_then(|s| s.next.lock().ok().and_then(|mut g| g.take()));
        if let Some(p) = p {
            unsafe { (a.bass.BASS_StreamFree)(p.handle) };
        }
    }

    fn teardown(&mut self) {
        let a = self.api().unwrap();
        self.gen += 1;
        self.drain_until = None;
        if self.out.is_some() {
            unsafe { (a.wasapi.BASS_WASAPI_Stop)(1) };
        }
        self.free_pending();
        if let Some(sh) = SHARED.get() {
            sh.mixer.store(0, Ordering::Release);
            sh.cur.store(0, Ordering::Release);
        }
        if let Some(c) = self.cur.take() {
            unsafe { (a.bass.BASS_StreamFree)(c.handle) };
        }
        self.loop_sync = 0;
        if self.mixer != 0 {
            unsafe { (a.bass.BASS_StreamFree)(self.mixer) };
            self.mixer = 0;
        }
        // Le mélangeur est libéré : son DSP ne sera plus appelé.
        if self.eq_dsp != 0 {
            drop(unsafe { Box::from_raw(self.eq_dsp as *mut eq::EqDsp) });
            self.eq_dsp = 0;
        }
        if self.master_dsp != 0 {
            drop(unsafe { Box::from_raw(self.master_dsp as *mut master::MasterDsp) });
            self.master_dsp = 0;
        }
    }

    fn fail(&mut self, index: usize, e: String) {
        self.error = Some(e);
        self.index = Some(index);
        self.playing = false;
    }

    fn load(&mut self, index: usize, play: bool, start_at: f64) {
        let a = self.api().unwrap();
        // Nouvelle piste : la boucle de la précédente n'a plus de sens.
        if self.index != Some(index) {
            self.loop_range = None;
        }
        self.teardown();
        self.finished = false;
        master::jump(1.0);
        let path = self.queue[index].clone();
        let mut src = match self.open(&path) {
            Ok(s) => s,
            Err(e) => return self.fail(index, e),
        };
        // Mode travail : on insère le module de vitesse (sans changer la hauteur).
        if self.practice {
            if let Some(fx) = &a.fx {
                let t = unsafe { (fx.BASS_FX_TempoCreate)(src.handle, BASS_STREAM_DECODE | BASS_FX_FREESOURCE) };
                if t != 0 {
                    src.handle = t;
                    src.tempo = true;
                    unsafe { (a.bass.BASS_ChannelSetAttribute)(t, BASS_ATTRIB_TEMPO, self.tempo) };
                }
            }
        }
        let out = match self.plan(&src).and_then(|cfg| self.ensure_output(cfg)) {
            Ok(o) => o,
            Err(e) => {
                unsafe { (a.bass.BASS_StreamFree)(src.handle) };
                return self.fail(index, e);
            }
        };
        unsafe {
            let mixer = (a.mix.BASS_Mixer_StreamCreate)(out.freq, out.chans, BASS_SAMPLE_FLOAT | BASS_STREAM_DECODE | BASS_MIXER_NONSTOP);
            if mixer == 0 {
                let e = self.last_error();
                (a.bass.BASS_StreamFree)(src.handle);
                return self.fail(index, e);
            }
            (a.bass.BASS_ChannelSetAttribute)(mixer, BASS_ATTRIB_VOLDSP, self.volume);
            let dsp = Box::into_raw(eq::EqDsp::new(out.freq, out.chans));
            (a.bass.BASS_ChannelSetDSP)(mixer, eq::dsp_proc, dsp as *mut c_void, 0);
            self.eq_dsp = dsp as usize;
            // Après l'égaliseur (priorité plus basse) : fondus, mono, balance.
            let md = Box::into_raw(master::MasterDsp::new(out.freq, out.chans));
            (a.bass.BASS_ChannelSetDSP)(mixer, master::dsp_proc, md as *mut c_void, -1);
            self.master_dsp = md as usize;
            if !self.attach(mixer, src.handle) {
                let e = self.last_error();
                (a.bass.BASS_StreamFree)(src.handle);
                (a.bass.BASS_StreamFree)(mixer);
                return self.fail(index, e);
            }
            if start_at > 0.0 {
                let b = (a.bass.BASS_ChannelSeconds2Bytes)(src.handle, start_at);
                (a.mix.BASS_Mixer_ChannelSetPosition)(src.handle, b, BASS_POS_BYTE | BASS_POS_MIXER_RESET);
            }
            self.mixer = mixer;
            SHARED.get().unwrap().mixer.store(mixer, Ordering::Release);
            SHARED.get().unwrap().cur.store(src.handle, Ordering::Release);
        }
        self.cur = Some(src);
        self.index = Some(index);
        self.error = None;
        self.playing = play;
        if play {
            unsafe { (a.wasapi.BASS_WASAPI_Start)() };
        }
        self.apply_loop(false);
        self.prepare_next();
    }

    /// Boucle : une synchro « mixtime » sur la fin ramène la lecture au début, sans blanc.
    fn apply_loop(&mut self, jump: bool) {
        let a = self.api().unwrap();
        let Some(c) = &self.cur else { return };
        let h = c.handle;
        unsafe {
            if self.loop_sync != 0 {
                (a.mix.BASS_Mixer_ChannelRemoveSync)(h, self.loop_sync);
                self.loop_sync = 0;
            }
            let Some((start, end)) = self.loop_range else { return };
            let sb = (a.bass.BASS_ChannelSeconds2Bytes)(h, start);
            let eb = (a.bass.BASS_ChannelSeconds2Bytes)(h, end);
            self.loop_sync = (a.mix.BASS_Mixer_ChannelSetSync)(h, BASS_SYNC_POS | BASS_SYNC_MIXTIME, eb, on_loop_end, sb as usize as *mut c_void);
        }
        // Si on est déjà après la fin de la boucle, on revient à son début.
        if jump {
            if let Some((start, end)) = self.loop_range {
                if self.position() >= end - 0.02 {
                    self.seek(start);
                }
            }
        }
    }

    /// Recharge la piste en cours au même endroit (changement de chaîne audio).
    fn reload_here(&mut self) {
        let pos = self.position();
        let playing = self.playing;
        if let (true, Some(i)) = (self.cur.is_some(), self.index) {
            self.load(i, playing, pos);
        }
    }

    unsafe fn attach(&self, mixer: u32, h: u32) -> bool {
        let a = self.api().unwrap();
        if (a.mix.BASS_Mixer_StreamAddChannel)(mixer, h, source_flags()) == 0 {
            return false;
        }
        // Meilleure qualité de rééchantillonnage disponible (sans effet si inutile).
        if (a.bass.BASS_ChannelSetAttribute)(h, BASS_ATTRIB_SRC, 4.0) == 0 {
            (a.bass.BASS_ChannelSetAttribute)(h, BASS_ATTRIB_SRC, 3.0);
        }
        (a.mix.BASS_Mixer_ChannelSetSync)(h, BASS_SYNC_END | BASS_SYNC_MIXTIME, 0, on_end, self.gen as usize as *mut c_void);
        true
    }

    /// Ouvre la piste suivante à l'avance si elle peut s'enchaîner sans blanc.
    fn prepare_next(&mut self) {
        let a = self.api().unwrap();
        self.free_pending();
        self.xfade_note = None;
        if self.practice {
            return;
        }
        let Some(i) = self.index else { return };
        let mut n = i + 1;
        if n >= self.queue.len() {
            // Répéter la file : la première piste s'enchaîne aussi sans blanc.
            if self.opts.end_mode == "repeat" && !self.queue.is_empty() {
                n = 0;
            } else {
                return;
            }
        }
        let path = self.queue[n].clone();
        let Ok(src) = self.open(&path) else { return };
        let same = match (self.plan(&src), self.out) {
            (Ok(cfg), Some(o)) => cfg == o.cfg,
            _ => false,
        };
        if same {
            unsafe {
                if (a.bass.BASS_ChannelSetAttribute)(src.handle, BASS_ATTRIB_SRC, 4.0) == 0 {
                    (a.bass.BASS_ChannelSetAttribute)(src.handle, BASS_ATTRIB_SRC, 3.0);
                }
            }
            let (after, xfade) = self.plan_crossfade(&path);
            if let Some(sh) = SHARED.get() {
                *sh.next.lock().unwrap() = Some(Pending { handle: src.handle, index: n, gen: self.gen, after, xfade, rg: src.rg });
            }
        } else {
            unsafe { (a.bass.BASS_StreamFree)(src.handle) };
        }
    }

    /// Fondu enchaîné vers `next` : pose la synchro sur la piste en cours.
    /// Renvoie (piste en cours, durée du fondu en octets du mélangeur).
    fn plan_crossfade(&mut self, next: &Path) -> (u32, u64) {
        let (after, xfade, note) = self.plan_crossfade_inner(next);
        self.xfade_note = note;
        (after, xfade)
    }

    fn plan_crossfade_inner(&mut self, next: &Path) -> (u32, u64, Option<String>) {
        let a = self.api().unwrap();
        let Some(c) = &self.cur else { return (0, 0, None) };
        let after = c.handle;
        let secs = self.opts.crossfade as f64;
        if secs < 0.1 {
            return (after, 0, None);
        }
        if self.practice || self.loop_range.is_some() {
            return (after, 0, Some("Fondu enchaîné suspendu (mode travail ou boucle)".into()));
        }
        if self.mixer == 0 {
            return (after, 0, None);
        }
        // Même album (même dossier) : enchaînement sans blanc, sauf si demandé.
        let cur_path = self.index.and_then(|i| self.queue.get(i));
        let same_album = cur_path.and_then(|p| p.parent()) == next.parent();
        if same_album && !self.opts.crossfade_albums {
            return (after, 0, Some("Pas de fondu vers le titre suivant : même album (enchaînement sans blanc)".into()));
        }
        let secs = secs.min(c.duration / 3.0);
        if secs < 0.1 {
            return (after, 0, Some("Pas de fondu : morceau trop court".into()));
        }
        unsafe {
            let len = (a.bass.BASS_ChannelGetLength)(c.handle, BASS_POS_BYTE);
            if len == u64::MAX {
                return (after, 0, Some("Pas de fondu : durée inconnue".into()));
            }
            let fb = (a.bass.BASS_ChannelSeconds2Bytes)(c.handle, secs);
            let sync = (a.mix.BASS_Mixer_ChannelSetSync)(c.handle, BASS_SYNC_POS | BASS_SYNC_MIXTIME, len.saturating_sub(fb), on_xfade, std::ptr::null_mut());
            if sync == 0 {
                return (after, 0, Some(format!("Fondu impossible : {}", self.last_error())));
            }
            self.xfade_sync = sync;
            let mb = (a.bass.BASS_ChannelSeconds2Bytes)(self.mixer, secs);
            let fired = XFADE_FIRED.load(Ordering::Acquire);
            let env = XFADE_ENV_OK.load(Ordering::Acquire);
            (
                after,
                mb,
                Some(format!(
                    "Fondu enchaîné de {:.1} s prévu vers le titre suivant (déjà fait {} fois{})",
                    secs,
                    fired,
                    if fired > 0 { format!(", enveloppes {}", match env { 3 => "OK", 0 => "refusées", _ => "en partie refusées" }) } else { String::new() }
                )
                .replace('.', ",")),
            )
        }
    }

    fn set_audio(&mut self, o: AudioOpts) {
        let old = std::mem::replace(&mut self.opts, o);
        master::set_mono(self.opts.mono);
        master::set_balance(self.opts.balance);
        let max = self.opts.max_volume.clamp(0.0, 1.0);
        if self.volume > max {
            self.volume = max;
            if self.mixer != 0 {
                let a = self.api().unwrap();
                unsafe { (a.bass.BASS_ChannelSetAttribute)(self.mixer, BASS_ATTRIB_VOLDSP, self.volume) };
            }
        }
        if self.cur.is_none() {
            return;
        }
        if (old.buffer_ms != self.opts.buffer_ms || old.exclusive_freq != self.opts.exclusive_freq) && self.out.is_some() {
            self.reopen();
            return;
        }
        let rg_changed = old.replaygain != self.opts.replaygain || old.rg_preamp != self.opts.rg_preamp || old.rg_clip != self.opts.rg_clip;
        if rg_changed {
            if let (Some(i), Some(c)) = (self.index, &self.cur) {
                let (base, path) = (c.base, self.queue[i].clone());
                let f = self.apply_rg(base, &path);
                if let Some(c) = &mut self.cur {
                    c.rg = f;
                }
            }
        }
        if rg_changed || old.crossfade != self.opts.crossfade || old.crossfade_albums != self.opts.crossfade_albums || old.end_mode != self.opts.end_mode {
            self.prepare_next();
        }
    }

    fn stop(&mut self) {
        self.teardown();
        self.playing = false;
    }

    fn toggle(&mut self) {
        let a = self.api().unwrap();
        if self.cur.is_none() {
            let i = self.index.unwrap_or(0);
            if i < self.queue.len() {
                self.load(i, true, 0.0);
            }
            return;
        }
        let ms = self.opts.fade_ms.min(1000);
        unsafe {
            if self.playing {
                if ms > 0 && self.out.is_some() {
                    // Fondu, puis arrêt une fois le fondu entendu ; on revient ensuite à
                    // l'endroit exact où il s'est terminé (le tampon d'avance est jeté).
                    let speed = match &self.cur {
                        Some(c) if c.tempo => 1.0 + self.tempo as f64 / 100.0,
                        _ => 1.0,
                    };
                    let p0 = self.position();
                    master::fade_to(0.0, ms);
                    std::thread::sleep(Duration::from_secs_f64(self.buffered_secs() + ms as f64 / 1000.0 + 0.03));
                    (a.wasapi.BASS_WASAPI_Stop)(1);
                    if let Some(c) = &self.cur {
                        let b = (a.bass.BASS_ChannelSeconds2Bytes)(c.handle, p0 + ms as f64 / 1000.0 * speed);
                        (a.mix.BASS_Mixer_ChannelSetPosition)(c.handle, b, BASS_POS_BYTE | BASS_POS_MIXER_RESET);
                    }
                    master::jump(0.0);
                } else {
                    (a.wasapi.BASS_WASAPI_Stop)(0);
                }
            } else if ms > 0 {
                master::jump(0.0);
                (a.wasapi.BASS_WASAPI_Start)();
                master::fade_to(1.0, ms);
            } else {
                master::jump(1.0);
                (a.wasapi.BASS_WASAPI_Start)();
            }
        }
        self.playing = !self.playing;
    }

    fn seek(&mut self, secs: f64) {
        let a = self.api().unwrap();
        let Some(c) = &self.cur else { return };
        let h = c.handle;
        self.drain_until = None;
        unsafe {
            let b = (a.bass.BASS_ChannelSeconds2Bytes)(h, secs.max(0.0));
            (a.wasapi.BASS_WASAPI_Stop)(1);
            (a.mix.BASS_Mixer_ChannelSetPosition)(h, b, BASS_POS_BYTE | BASS_POS_MIXER_RESET);
            if self.playing {
                master::jump(1.0);
                (a.wasapi.BASS_WASAPI_Start)();
            }
        }
    }

    fn set_output(&mut self, p: OutputPref) {
        if p == self.pref && self.out.is_some() {
            return;
        }
        let pos = self.position();
        let playing = self.playing;
        let had_track = self.cur.is_some();
        self.pref = p;
        self.teardown();
        self.close_output();
        if let (true, Some(i)) = (had_track, self.index) {
            self.load(i, playing, pos);
        }
    }

    fn position(&self) -> f64 {
        let (Some(a), Some(c)) = (self.api(), &self.cur) else { return 0.0 };
        if self.drain_until.is_some() {
            return (self.last_duration - self.buffered_secs()).max(0.0);
        }
        let b = unsafe { (a.mix.BASS_Mixer_ChannelGetPosition)(c.handle, BASS_POS_BYTE) };
        if b == u64::MAX {
            return c.duration;
        }
        let s = unsafe { (a.bass.BASS_ChannelBytes2Seconds)(c.handle, b) };
        let speed = if c.tempo { 1.0 + self.tempo as f64 / 100.0 } else { 1.0 };
        (s - self.buffered_secs() * speed).clamp(0.0, c.duration.max(0.0))
    }

    // --- état ---------------------------------------------------------------

    fn signal(&self) -> Option<SignalPath> {
        let c = self.cur.as_ref()?;
        let o = self.out?;
        let src_bits = if c.float {
            format!("{} bits flottant", c.bits)
        } else if c.bits > 0 {
            format!("{} bits", c.bits)
        } else {
            "compressé".into()
        };
        let source = format!("{} {} / {}", c.format, src_bits, khz(c.freq));
        let (out_bits_label, out_bits) = match o.format {
            BASS_WASAPI_FORMAT_FLOAT => ("32 bits flottant", 24),
            BASS_WASAPI_FORMAT_8BIT => ("8 bits", 8),
            BASS_WASAPI_FORMAT_16BIT => ("16 bits", 16),
            BASS_WASAPI_FORMAT_24BIT => ("24 bits", 24),
            BASS_WASAPI_FORMAT_32BIT => ("32 bits", 32),
            _ => ("?", 0),
        };
        let mode = if o.cfg.exclusive { "WASAPI exclusif" } else { "WASAPI partagé" };
        let output = format!("{mode} · {out_bits_label} / {}", khz(o.freq));

        let mut notes = vec![];
        let mut ok = true;
        if !o.cfg.exclusive {
            ok = false;
            notes.push("Mode partagé : le son passe par le mixeur de Windows (volume et format réglés dans Windows)".to_string());
        }
        if o.freq != c.freq {
            ok = false;
            notes.push(format!("Rééchantillonné de {} vers {}", khz(c.freq), khz(o.freq)));
        }
        if c.bits > 0 && out_bits < c.bits && !c.float {
            ok = false;
            notes.push(format!("Résolution réduite de {} à {} bits", c.bits, out_bits));
        }
        if c.tempo && self.tempo.abs() > 0.01 {
            ok = false;
            notes.push(format!("Mode travail : vitesse à {} %", (100.0 + self.tempo).round()));
        }
        if let Some(f) = c.rg.filter(|f| (f - 1.0).abs() > 0.001) {
            ok = false;
            notes.push(format!("ReplayGain : {:+.1} dB", 20.0 * f.log10()).replace('.', ","));
        }
        if let Some(n) = &self.xfade_note {
            notes.push(n.clone());
        }
        if master::active() {
            ok = false;
            notes.push("Mono ou balance actifs".to_string());
        }
        if eq::get().active() {
            ok = false;
            notes.push("Égaliseur actif".to_string());
        }
        if self.volume < 0.999 {
            ok = false;
            notes.push("Volume logiciel actif : règle le volume sur le DAC ou l'ampli pour rester bit-perfect".to_string());
        }
        if o.chans != c.chans {
            notes.push(format!("{} canal(aux) source envoyé(s) sur {}", c.chans, o.chans));
        }
        if c.bits == 0 {
            notes.push("Source compressée avec pertes : bit-perfect par rapport au fichier décodé".to_string());
        }
        Some(SignalPath { source, output, device: self.device_name.clone(), shared: !o.cfg.exclusive, bit_perfect: ok, notes })
    }

    fn emit(&self) {
        let st = PlayerState {
            ready: self.api().is_some(),
            index: self.index,
            path: self.index.and_then(|i| self.queue.get(i)).map(|p| p.to_string_lossy().into_owned()),
            playing: self.playing,
            position: self.position(),
            duration: self.cur.as_ref().map(|c| c.duration).unwrap_or(0.0),
            volume: self.volume,
            signal: self.signal(),
            error: self.load_error.clone().or_else(|| self.error.clone()),
            practice: self.practice,
            tempo: self.tempo,
            tempo_available: self.api().map(|a| a.fx.is_some()).unwrap_or(false),
            loop_range: self.loop_range,
            finished: self.finished,
        };
        (self.emit_fn)(&st);
    }
}
