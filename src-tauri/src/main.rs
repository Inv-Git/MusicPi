#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accent;
mod artist;
mod bass;
mod collections;
mod engine;
mod eq;
mod lastfm;
mod library;
mod lyrics;
mod manage;
mod master;
mod online;
mod phone;
mod power;
mod remote;
mod settings;
mod smtc;
mod stats;
mod system;
mod tags;
mod watch;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use engine::{Cmd, DeviceDesc, EngineHandle, OutputPref};
use settings::Settings;

struct AppState {
    engine: EngineHandle,
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    library_path: PathBuf,
    store: manage::Store,
    phone: Arc<phone::Phone>,
    collections: Arc<collections::Store>,
    stats: Arc<stats::Store>,
    lyrics: lyrics::Store,
    data_dir: PathBuf,
}

/// Fermeture demandée depuis la zone de notification : on quitte vraiment.
static QUITTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Contrôles multimédia de Windows actifs (les touches multimédia passent par eux).
static SMTC_OK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Écoutes, Last.fm
// ---------------------------------------------------------------------------

type PendingScrobble = (String, String, String, u32, u64);

/// Envoie un scrobble (et ceux restés en attente, faute de réseau).
fn scrobble_async(app: &AppHandle, s: lastfm::Scrobble) {
    let Some(st) = app.try_state::<AppState>() else { return };
    let cfg = st.settings.lock().unwrap().lastfm.clone();
    if cfg.session.is_empty() || !cfg.scrobble {
        return;
    }
    let queue_path = st.data_dir.join("scrobbles-en-attente.json");
    std::thread::spawn(move || {
        static LOCK: Mutex<()> = Mutex::new(());
        let _g = LOCK.lock();
        let acc = lastfm::Account { api_key: cfg.api_key, secret: cfg.api_secret, session: cfg.session };
        let mut queue: Vec<PendingScrobble> = std::fs::read(&queue_path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        queue.push((s.artist, s.title, s.album, s.duration, s.timestamp));
        let mut left = vec![];
        for (artist, title, album, duration, timestamp) in queue {
            let sc = lastfm::Scrobble { artist, title, album, duration, timestamp };
            if lastfm::scrobble(&acc, &sc).is_err() {
                left.push((sc.artist, sc.title, sc.album, sc.duration, sc.timestamp));
            }
        }
        left.truncate(2000);
        let _ = std::fs::write(&queue_path, serde_json::to_vec(&left).unwrap_or_default());
    });
}

/// Un titre a été écouté (sur le PC ou le téléphone).
fn on_played(app: &AppHandle, path: &str, artist: String, title: String, album: String, duration: f64) -> u32 {
    let Some(st) = app.try_state::<AppState>() else { return 0 };
    let at = now_secs();
    let n = st.stats.record(path, at);
    let _ = app.emit("stats-changed", (path.to_string(), n, at));
    if !artist.is_empty() && !title.is_empty() {
        scrobble_async(app, lastfm::Scrobble { artist, title, album, duration: duration as u32, timestamp: at.saturating_sub(duration as u64 / 2) });
    }
    n
}

#[tauri::command]
fn record_play(app: AppHandle, path: String, artist: String, title: String, album: String, duration: f64) -> u32 {
    on_played(&app, &path, artist, title, album, duration)
}

#[tauri::command]
fn get_stats(state: State<AppState>) -> stats::Stats {
    state.stats.get()
}

/// Morceau en cours : contrôles multimédia de Windows et « en cours d'écoute » Last.fm.
#[tauri::command]
fn media_update(app: AppHandle, path: String, title: String, artist: String, album: String, duration: f64, position: f64, playing: bool, changed: bool) {
    // Pochette pour Windows : fichier dans le cache.
    let cover = if changed {
        app.path().app_cache_dir().ok().and_then(|d| {
            let f = d.join("smtc-pochette.jpg");
            let (b, _) = library::cover(std::path::Path::new(&path))?;
            std::fs::write(&f, b).ok()?;
            Some(format!("file:///{}", f.to_string_lossy().replace('\\', "/")))
        })
    } else {
        None
    };
    {
        static LAST_COVER: Mutex<Option<String>> = Mutex::new(None);
        let mut lc = LAST_COVER.lock().unwrap();
        if changed {
            *lc = cover;
        }
        let cover = lc.clone();
        let (t, a, al) = (title.clone(), artist.clone(), album.clone());
        let _ = app.run_on_main_thread(move || smtc::update(&t, &a, &al, cover.as_deref(), duration, playing, position));
    }
    if changed && playing {
        if let Some(st) = app.try_state::<AppState>() {
            let cfg = st.settings.lock().unwrap().lastfm.clone();
            if !cfg.session.is_empty() && cfg.scrobble {
                std::thread::spawn(move || {
                    let acc = lastfm::Account { api_key: cfg.api_key, secret: cfg.api_secret, session: cfg.session };
                    let _ = lastfm::now_playing(&acc, &lastfm::Scrobble { artist, title, album, duration: duration as u32, timestamp: 0 });
                });
            }
        }
    }
}

#[tauri::command]
fn media_stopped(app: AppHandle) {
    let _ = app.run_on_main_thread(smtc::stopped);
}

#[tauri::command]
async fn lastfm_login(state: State<'_, AppState>, api_key: String, api_secret: String, user: String, password: String) -> Result<String, String> {
    let (k, sec, u) = (api_key.trim().to_string(), api_secret.trim().to_string(), user.trim().to_string());
    let (k2, s2, u2) = (k.clone(), sec.clone(), u.clone());
    let session = blocking(move || lastfm::login(&k2, &s2, &u2, &password)).await??;
    state.update(|s| {
        s.lastfm = settings::LastFm { api_key: k, api_secret: sec, user: u.clone(), session, scrobble: true };
    });
    Ok(u)
}

#[tauri::command]
fn lastfm_set(state: State<AppState>, scrobble: bool, logout: bool) {
    state.update(|s| {
        s.lastfm.scrobble = scrobble;
        if logout {
            s.lastfm.session.clear();
            s.lastfm.user.clear();
        }
    });
}

// ---------------------------------------------------------------------------
// Paroles
// ---------------------------------------------------------------------------

/// Paroles d'un morceau : `.lrc` ou tags d'abord, puis cache, puis LRCLIB (si permis).
/// `force` : refait la recherche en ligne même si elle n'avait rien donné récemment.
pub fn lyrics_for(app: &AppHandle, path: &str, force: bool) -> lyrics::LyricsOut {
    let st = app.state::<AppState>();
    let p = PathBuf::from(path);
    let entry = st.lyrics.get(path);
    let out = |l: Option<lyrics::Lyrics>| lyrics::LyricsOut { lyrics: l, offset: entry.offset };
    if let Some(l) = lyrics::local(&p) {
        return out(Some(l));
    }
    if entry.lyrics.is_some() && !force {
        return out(entry.lyrics.clone());
    }
    let prefs = st.settings.lock().unwrap().lyrics.clone();
    if !prefs.online || (!force && now_secs().saturating_sub(entry.checked) < lyrics::RETRY_SECS) {
        return out(entry.lyrics.clone());
    }
    let t = library::read(&p);
    match lyrics::lrclib(&t.artist, &t.title, &t.album, t.duration, false) {
        Ok(found) => {
            if let (true, Some(l)) = (prefs.save_lrc, found.as_ref()) {
                if !l.instrumental {
                    let _ = lyrics::save_sidecar(&p, l);
                }
            }
            st.lyrics.update(path, |e| {
                e.checked = now_secs();
                e.lyrics = found.clone();
            });
            out(found)
        }
        // Pas de réseau : on réessaiera la prochaine fois.
        Err(_) => out(entry.lyrics.clone()),
    }
}

#[tauri::command]
async fn get_lyrics(app: AppHandle, path: String, force: Option<bool>) -> Result<lyrics::LyricsOut, String> {
    blocking(move || lyrics_for(&app, &path, force.unwrap_or(false))).await
}

#[tauri::command]
fn set_lyrics_offset(state: State<AppState>, path: String, offset: i32) {
    state.lyrics.update(&path, |e| e.offset = offset.clamp(-30_000, 30_000));
}

#[tauri::command]
fn set_lyrics_prefs(state: State<AppState>, online: bool, save_lrc: bool) {
    state.update(|s| {
        s.lyrics.online = online;
        s.lyrics.save_lrc = save_lrc;
    });
}

#[derive(serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct LyricsTest {
    tested: usize,
    synced: usize,
    plain: usize,
    instrumental: usize,
    none: usize,
    errors: usize,
}

/// Essaie `count` morceaux pris au hasard dans la bibliothèque (fichiers, puis LRCLIB).
#[tauri::command]
async fn lyrics_test(app: AppHandle, state: State<'_, AppState>, count: usize) -> Result<LyricsTest, String> {
    let path = state.library_path.clone();
    blocking(move || {
        let tracks = library::load_cache(&path);
        let mut seed = now_secs() ^ 0x9e37_79b9_7f4a_7c15;
        let mut pick: Vec<&library::Track> = Vec::new();
        let n = count.min(tracks.len());
        let mut used = std::collections::HashSet::new();
        while pick.len() < n {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let i = (seed % tracks.len() as u64) as usize;
            if used.insert(i) {
                pick.push(&tracks[i]);
            }
        }
        let mut r = LyricsTest::default();
        for (k, t) in pick.iter().enumerate() {
            let found = match lyrics::local(Path::new(&t.path)) {
                Some(l) => Ok(Some(l)),
                None => lyrics::lrclib(&t.artist, &t.title, &t.album, t.duration, true),
            };
            match found {
                Ok(Some(l)) if l.instrumental => r.instrumental += 1,
                Ok(Some(l)) if l.synced => r.synced += 1,
                Ok(Some(_)) => r.plain += 1,
                Ok(None) => r.none += 1,
                Err(_) => r.errors += 1,
            }
            r.tested += 1;
            let _ = app.emit("lyrics-test", (k + 1, n));
        }
        r
    })
    .await
}

// ---------------------------------------------------------------------------
// Système : démarrage de Windows, zone de notification, sauvegarde
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemPrefs {
    autostart: bool,
    tray_close: bool,
    smtc: bool,
}

#[tauri::command]
fn get_system(state: State<AppState>) -> SystemPrefs {
    SystemPrefs { autostart: system::autostart_enabled(), tray_close: state.settings.lock().unwrap().tray_close, smtc: SMTC_OK.load(std::sync::atomic::Ordering::Acquire) }
}

#[tauri::command]
fn set_system(state: State<AppState>, autostart: bool, tray_close: bool) -> Result<(), String> {
    state.update(|s| s.tray_close = tray_close);
    if autostart != system::autostart_enabled() {
        system::set_autostart(autostart)?;
    }
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Backup {
    app: String,
    version: String,
    date: u64,
    settings: serde_json::Value,
    collections: collections::Collections,
    stats: stats::Stats,
}

#[tauri::command]
async fn export_backup(app: AppHandle, state: State<'_, AppState>) -> Result<Option<String>, String> {
    let mut settings = serde_json::to_value(state.settings.lock().unwrap().clone()).map_err(|e| e.to_string())?;
    // Les appareils appairés ne voyagent pas (leurs clés d'accès restent sur ce PC).
    if let Some(p) = settings.get_mut("phone") {
        p["devices"] = serde_json::json!([]);
    }
    let b = Backup { app: "MusicPi".into(), version: env!("CARGO_PKG_VERSION").into(), date: now_secs(), settings, collections: state.collections.get(), stats: state.stats.get() };
    let json = serde_json::to_vec_pretty(&b).map_err(|e| e.to_string())?;
    let path = blocking(move || {
        app.dialog()
            .file()
            .set_file_name("sauvegarde-musicpi.json")
            .add_filter("Sauvegarde MusicPi", &["json"])
            .blocking_save_file()
            .and_then(|p| p.into_path().ok())
    })
    .await?;
    let Some(path) = path else { return Ok(None) };
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[tauri::command]
async fn import_backup(app: AppHandle, state: State<'_, AppState>) -> Result<bool, String> {
    let path = blocking(move || app.dialog().file().add_filter("Sauvegarde MusicPi", &["json"]).blocking_pick_file().and_then(|p| p.into_path().ok())).await?;
    let Some(path) = path else { return Ok(false) };
    let b: Backup = serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?).map_err(|_| "Ce fichier n'est pas une sauvegarde de MusicPi".to_string())?;
    // Réglages : on garde les appareils appairés de ce PC.
    let mut incoming: Settings = serde_json::from_value(b.settings).map_err(|e| e.to_string())?;
    {
        let cur = state.settings.lock().unwrap();
        incoming.phone.devices = cur.phone.devices.clone();
    }
    state.update(|s| *s = incoming.clone());
    eq::set(incoming.eq.clone());
    state.engine.send(Cmd::SetAudio(incoming.audio.clone()));
    state.collections.replace(b.collections);
    state.stats.merge(b.stats);
    Ok(true)
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

impl AppState {
    fn update(&self, f: impl FnOnce(&mut Settings)) {
        let mut s = self.settings.lock().unwrap();
        f(&mut s);
        s.save(&self.settings_path);
    }
}

#[tauri::command]
fn play(state: State<AppState>, paths: Vec<String>, index: usize) {
    let queue = paths.into_iter().map(PathBuf::from).collect();
    state.engine.send(Cmd::Play { queue, index });
}

#[tauri::command]
fn toggle(state: State<AppState>) {
    state.engine.send(Cmd::Toggle);
}

#[tauri::command]
fn next(state: State<AppState>) {
    state.engine.send(Cmd::Next);
}

#[tauri::command]
fn prev(state: State<AppState>) {
    state.engine.send(Cmd::Prev);
}

#[tauri::command]
fn seek(state: State<AppState>, position: f64) {
    state.engine.send(Cmd::Seek(position));
}

#[tauri::command]
fn set_volume(state: State<AppState>, volume: f32) {
    state.engine.send(Cmd::SetVolume(volume));
    state.update(|s| s.volume = volume);
}

#[tauri::command]
fn set_practice(state: State<AppState>, on: bool) {
    state.engine.send(Cmd::SetPractice(on));
}

#[tauri::command]
fn set_tempo(state: State<AppState>, tempo: f32) {
    state.engine.send(Cmd::SetTempo(tempo));
}

#[tauri::command]
fn set_loop(state: State<AppState>, start: Option<f64>, end: Option<f64>) {
    let r = match (start, end) {
        (Some(a), Some(b)) => Some((a, b)),
        _ => None,
    };
    state.engine.send(Cmd::SetLoop(r));
}

#[tauri::command]
fn set_output(state: State<AppState>, output: OutputPref) {
    state.engine.send(Cmd::SetOutput(output.clone()));
    state.update(|s| s.output = output);
}

#[tauri::command]
fn set_eq(state: State<AppState>, eq: eq::EqParams) {
    eq::set(eq.clone());
    state.update(|s| s.eq = eq);
}

#[tauri::command]
fn set_shortcuts(state: State<AppState>, shortcuts: std::collections::HashMap<String, String>) {
    state.update(|s| s.shortcuts = shortcuts);
}

#[tauri::command]
fn set_theme(state: State<AppState>, theme: String) {
    state.update(|s| s.theme = theme);
}

/// « coral » (couleur de l'app) ou « windows » (couleur d'accent de Windows).
#[tauri::command]
fn set_accent(state: State<AppState>, accent: String) {
    state.update(|s| s.accent = accent);
}

#[tauri::command]
fn windows_accent() -> accent::WindowsAccent {
    accent::get()
}

/// Commande synchrone : exécutée sur le fil principal, toujours le même (voir power::keep_awake).
#[tauri::command]
fn keep_awake(on: bool) {
    power::keep_awake(on);
}

#[tauri::command]
fn idle_seconds() -> u32 {
    power::idle_seconds()
}

#[tauri::command]
fn set_audio(state: State<AppState>, audio: engine::AudioOpts) {
    state.engine.send(Cmd::SetAudio(audio.clone()));
    state.update(|s| {
        s.audio = audio;
        s.volume = s.volume.min(s.audio.max_volume);
    });
}

#[tauri::command]
fn set_play_prefs(state: State<AppState>, resume: bool, shuffle: bool) {
    state.update(|s| {
        s.resume = resume;
        s.shuffle = shuffle;
    });
}

/// File de lecture et position, gardées pour la reprise au démarrage.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Session {
    paths: Vec<String>,
    index: usize,
    position: f64,
}

fn session_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("session.json"))
}

#[tauri::command]
fn save_session(app: AppHandle, paths: Vec<String>, index: usize, position: f64) {
    if let Some(p) = session_path(&app) {
        if let Ok(b) = serde_json::to_vec(&Session { paths, index, position }) {
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, b).is_ok() {
                let _ = std::fs::rename(&tmp, &p);
            }
        }
    }
}

#[tauri::command]
fn restore_session(app: AppHandle, state: State<AppState>) -> Option<Session> {
    let p = session_path(&app)?;
    let s: Session = serde_json::from_slice(&std::fs::read(p).ok()?).ok()?;
    let queue: Vec<PathBuf> = s.paths.iter().map(PathBuf::from).collect();
    if s.index < queue.len() && queue[s.index].exists() {
        state.engine.send(Cmd::Restore { queue, index: s.index, position: s.position });
        Some(s)
    } else {
        None
    }
}

#[tauri::command]
fn set_ambient(state: State<AppState>, auto: bool, delay: u32) {
    state.update(|s| {
        s.ambient_auto = auto;
        s.ambient_delay = delay.clamp(60, 3600);
    });
}

#[tauri::command]
async fn list_devices(state: State<'_, AppState>) -> Result<Vec<DeviceDesc>, String> {
    Ok(state.engine.devices())
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
async fn pick_folder(app: AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .blocking_pick_folder()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
async fn load_library(state: State<'_, AppState>) -> Result<Vec<library::Track>, String> {
    let path = state.library_path.clone();
    tauri::async_runtime::spawn_blocking(move || library::load_cache(&path))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn scan_library(app: AppHandle, state: State<'_, AppState>, folder: Option<String>) -> Result<Vec<library::Track>, String> {
    if let Some(f) = folder {
        state.update(|s| s.library_folder = Some(f));
    }
    let roots = state.settings.lock().unwrap().roots();
    let emitter = app.clone();
    let cache = state.library_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        SCANNING.store(true, std::sync::atomic::Ordering::Release);
        let tracks = library::scan_all(&roots, |done, total| {
            let _ = emitter.emit("scan-progress", (done, total));
        });
        {
            let _lock = library::CACHE_LOCK.lock();
            library::save_cache(&cache, &tracks);
        }
        SCANNING.store(false, std::sync::atomic::Ordering::Release);
        tracks
    })
    .await
    .map_err(|e| e.to_string())
}

/// Scan complet en cours : la surveillance des dossiers attend.
static SCANNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[tauri::command]
fn add_library_folder(state: State<AppState>, folder: String) {
    state.update(|s| {
        if s.library_folder.is_none() {
            s.library_folder = Some(folder);
        } else if !s.extra_folders.contains(&folder) && s.library_folder.as_deref() != Some(folder.as_str()) {
            s.extra_folders.push(folder);
        }
    });
}

#[tauri::command]
fn remove_library_folder(state: State<AppState>, folder: String) {
    state.update(|s| {
        s.extra_folders.retain(|f| f != &folder);
        if s.library_folder.as_deref() == Some(folder.as_str()) {
            s.library_folder = if s.extra_folders.is_empty() { None } else { Some(s.extra_folders.remove(0)) };
        }
    });
}

#[tauri::command]
fn set_library_prefs(state: State<AppState>, watch: bool, ignore_ext: Vec<String>) {
    library::set_skip(&ignore_ext);
    state.update(|s| {
        s.watch = watch;
        s.ignore_ext = ignore_ext;
    });
}

/// Menu de la zone de notification, en français ou en anglais.
fn tray_menu(app: &tauri::AppHandle, en: bool) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    let l = |fr: &'static str, e: &'static str| if en { e } else { fr };
    let show = MenuItem::with_id(app, "show", l("Afficher MusicPi", "Show MusicPi"), true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", l("Lecture / pause", "Play / pause"), true, None::<&str>)?;
    let prev = MenuItem::with_id(app, "prev", l("Précédent", "Previous"), true, None::<&str>)?;
    let next = MenuItem::with_id(app, "next", l("Suivant", "Next"), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", l("Quitter", "Quit"), true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    Menu::with_items(app, &[&show, &sep1, &toggle, &prev, &next, &sep2, &quit])
}

#[tauri::command]
fn set_tray_lang(app: tauri::AppHandle, en: bool) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id("main"), tray_menu(&app, en)) {
        let _ = tray.set_menu(Some(menu));
    }
}

#[tauri::command]
fn set_ui(state: State<AppState>, ui: serde_json::Value) {
    state.update(|s| s.ui = ui);
}

/// Surveillance des dossiers : toutes les 30 s, les dossiers modifiés sont relus.
fn start_watch(app: AppHandle) {
    std::thread::Builder::new()
        .name("surveillance-dossiers".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(8));
            let mut prev: Option<(Vec<PathBuf>, watch::Snapshot)> = None;
            loop {
                if let Some(st) = app.try_state::<AppState>() {
                    let (roots, on) = {
                        let s = st.settings.lock().unwrap();
                        (s.roots(), s.watch)
                    };
                    if on && !roots.is_empty() && !SCANNING.load(std::sync::atomic::Ordering::Acquire) {
                        let snap = watch::snapshot(&roots);
                        let cache = st.library_path.clone();
                        let known: std::collections::HashMap<String, u64> = {
                            let _lock = library::CACHE_LOCK.lock();
                            library::load_cache(&cache).into_iter().map(|t| (t.path, t.size)).collect()
                        };
                        let (changed, gone) = match &prev {
                            // Même configuration : on compare avec le relevé précédent.
                            Some((r, old)) if r == &roots => watch::diff(old, &snap, None),
                            // Premier passage : ce qui a changé depuis la dernière mise à jour du cache,
                            // et les dossiers connus qui ont disparu.
                            _ => {
                                let since = std::fs::metadata(&cache).and_then(|m| m.modified()).ok();
                                let (changed, _) = watch::diff(&Default::default(), &snap, since);
                                let mut gone: Vec<PathBuf> = known
                                    .keys()
                                    .filter_map(|p| std::path::Path::new(p).parent().map(|d| d.to_path_buf()))
                                    .filter(|d| !snap.contains_key(d))
                                    .collect();
                                gone.sort();
                                gone.dedup();
                                (changed, gone)
                            }
                        };
                        let d = watch::delta(&changed, &gone, &known, library::wanted);
                        if !d.removed.is_empty() || !d.added.is_empty() {
                            let tracks: Vec<library::Track> = d.added.iter().filter(|p| p.is_file()).map(|p| library::read(p)).collect();
                            {
                                let _lock = library::CACHE_LOCK.lock();
                                let mut all = library::load_cache(&cache);
                                let drop: std::collections::HashSet<&str> =
                                    d.removed.iter().map(|s| s.as_str()).chain(tracks.iter().map(|t| t.path.as_str())).collect();
                                all.retain(|t| !drop.contains(t.path.as_str()));
                                all.extend(tracks.iter().cloned());
                                library::save_cache(&cache, &all);
                            }
                            let ch = manage::Changed { removed: d.removed, tracks, ..Default::default() };
                            let _ = app.emit("library-changed", ch);
                        }
                        prev = Some((roots, snap));
                    }
                }
                std::thread::sleep(std::time::Duration::from_secs(30));
            }
        })
        .ok();
}

/// Décode une URL du type http://cover.localhost/<chemin encodé>.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn image_response(img: Option<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    match img {
        Some(b) => {
            let mime = if b.starts_with(&[0x89, b'P', b'N', b'G']) { "image/png" } else { "image/jpeg" };
            tauri::http::Response::builder().header("Content-Type", mime).header("Cache-Control", "max-age=31536000").body(b).unwrap()
        }
        None => tauri::http::Response::builder().status(404).body(Vec::new()).unwrap(),
    }
}

fn cover_response(uri_path: &str) -> tauri::http::Response<Vec<u8>> {
    let path = percent_decode(uri_path.trim_start_matches('/'));
    match library::cover(std::path::Path::new(&path)) {
        Some((bytes, mime)) => tauri::http::Response::builder()
            .header("Content-Type", mime)
            .header("Cache-Control", "max-age=31536000")
            .header("Access-Control-Allow-Origin", "*")
            .body(bytes)
            .unwrap(),
        None => tauri::http::Response::builder().status(404).body(Vec::new()).unwrap(),
    }
}

// ---------------------------------------------------------------------------
// Gestion de la bibliothèque
// ---------------------------------------------------------------------------

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
struct TagEditIn {
    path: String,
    fields: std::collections::HashMap<String, String>,
}

#[tauri::command]
async fn edit_tags(state: State<'_, AppState>, label: String, edits: Vec<TagEditIn>) -> Result<manage::Changed, String> {
    let store = state.store.clone();
    blocking(move || store.edit_tags(&label, edits.into_iter().map(|e| (e.path, e.fields)).collect())).await
}

/// `source` : adresse web d'une image, ou chemin d'un fichier image local.
#[tauri::command]
async fn set_cover(state: State<'_, AppState>, label: String, paths: Vec<String>, source: String) -> Result<manage::Changed, String> {
    let store = state.store.clone();
    let folder_file = state.settings.lock().unwrap().cover_file;
    blocking(move || {
        let data = if source.starts_with("http") { online::download(&source)? } else { std::fs::read(&source).map_err(|e| e.to_string())? };
        Ok::<_, String>(store.set_cover(&label, paths, &data, folder_file))
    })
    .await?
}

#[tauri::command]
async fn pick_image(app: AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .add_filter("Images", &["jpg", "jpeg", "png", "webp"])
        .blocking_pick_file()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
}

#[derive(serde::Serialize)]
struct CoverResults {
    hits: Vec<online::CoverHit>,
    errors: Vec<String>,
}

#[tauri::command]
async fn search_covers(state: State<'_, AppState>, artist: String, album: String) -> Result<CoverResults, String> {
    let token = state.settings.lock().unwrap().discogs_token.clone();
    blocking(move || {
        let (hits, errors) = online::search_covers(&artist, &album, &token);
        CoverResults { hits, errors }
    })
    .await
}

#[tauri::command]
async fn search_releases(state: State<'_, AppState>, source: String, artist: String, album: String) -> Result<Vec<online::ReleaseHit>, String> {
    let token = state.settings.lock().unwrap().discogs_token.clone();
    blocking(move || online::search_releases(&source, &artist, &album, &token)).await?
}

#[tauri::command]
async fn release_details(state: State<'_, AppState>, source: String, id: String) -> Result<online::ReleaseInfo, String> {
    let token = state.settings.lock().unwrap().discogs_token.clone();
    blocking(move || online::release_details(&source, &id, &token)).await?
}

#[tauri::command]
async fn move_files(state: State<'_, AppState>, label: String, moves: Vec<(String, String)>) -> Result<manage::Changed, String> {
    let store = state.store.clone();
    let root = {
        let s = state.settings.lock().unwrap();
        let first = moves.first().map(|(f, _)| PathBuf::from(f));
        first.and_then(|f| s.root_of(&f)).or_else(|| s.library_folder.as_ref().map(PathBuf::from)).ok_or("Aucun dossier de bibliothèque")?
    };
    let coll = state.collections.clone();
    let st2 = state.stats.clone();
    blocking(move || {
        let ch = store.move_files(&label, moves, &root);
        coll.remap(&ch.moved);
        st2.remap(&ch.moved);
        ch
    })
    .await
}

#[tauri::command]
async fn history(state: State<'_, AppState>) -> Result<Vec<manage::Entry>, String> {
    let store = state.store.clone();
    blocking(move || store.history()).await
}

#[tauri::command]
async fn undo(state: State<'_, AppState>, id: u64) -> Result<manage::Changed, String> {
    let store = state.store.clone();
    let root = state.settings.lock().unwrap().library_folder.clone();
    let coll = state.collections.clone();
    let st2 = state.stats.clone();
    blocking(move || {
        let ch = store.undo(id, root.as_deref().map(std::path::Path::new));
        coll.remap(&ch.moved);
        st2.remap(&ch.moved);
        ch
    })
    .await
}

// ---------------------------------------------------------------------------
// Cœurs et playlists
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
struct CollectionsOut {
    collections: collections::Collections,
    id: Option<String>,
}

#[tauri::command]
fn get_collections(state: State<AppState>) -> collections::Collections {
    state.collections.get()
}

#[tauri::command]
fn collection_op(state: State<AppState>, op: collections::Op) -> Result<CollectionsOut, String> {
    let (collections, id) = state.collections.apply(op)?;
    Ok(CollectionsOut { collections, id })
}

#[tauri::command]
async fn search_artist_images(state: State<'_, AppState>, name: String) -> Result<CoverResults, String> {
    let token = state.settings.lock().unwrap().discogs_token.clone();
    blocking(move || {
        let (hits, errors) = online::artist_hits(&name, &token);
        CoverResults { hits, errors }
    })
    .await
}

/// `source` : adresse web d'une image ou chemin d'un fichier local.
#[tauri::command]
async fn set_artist_image(app: AppHandle, state: State<'_, AppState>, name: String, track: String, source: String) -> Result<(), String> {
    let root = state.settings.lock().unwrap().root_of(std::path::Path::new(&track)).map(|p| p.to_string_lossy().into_owned());
    let cache = app.path().app_cache_dir().map_err(|e| e.to_string())?;
    blocking(move || {
        let data = if source.starts_with("http") { online::download(&source)? } else { std::fs::read(&source).map_err(|e| e.to_string())? };
        artist::save(&name, std::path::Path::new(&track), root.as_deref().map(std::path::Path::new), &cache, &data)
    })
    .await?
}

#[tauri::command]
async fn phone_status(state: State<'_, AppState>) -> Result<phone::PhoneStatus, String> {
    let ph = state.phone.clone();
    blocking(move || ph.status()).await
}

#[tauri::command]
async fn set_phone(state: State<'_, AppState>, enabled: bool, port: u16) -> Result<phone::PhoneStatus, String> {
    state.update(|s| {
        s.phone.enabled = enabled;
        s.phone.port = if port == 0 { phone::DEFAULT_PORT } else { port };
    });
    let ph = state.phone.clone();
    blocking(move || {
        ph.apply();
        ph.status()
    })
    .await
}

#[tauri::command]
fn phone_new_code(state: State<AppState>) -> String {
    state.phone.new_code();
    state.phone.status().code
}

#[tauri::command]
fn phone_revoke(state: State<AppState>, index: usize) {
    state.update(|s| {
        if index < s.phone.devices.len() {
            s.phone.devices.remove(index);
        }
    });
}

#[tauri::command]
fn set_tools(state: State<AppState>, discogs_token: String, rename_pattern: String, cover_file: bool) {
    state.update(|s| {
        s.discogs_token = discogs_token.trim().to_string();
        s.rename_pattern = rename_pattern;
        s.cover_file = cover_file;
    });
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("cover", |_ctx, request, responder| {
            let uri_path = request.uri().path().to_string();
            std::thread::spawn(move || responder.respond(cover_response(&uri_path)));
        })
        // http://artist.localhost/<nom de l'artiste>?t=<chemin d'une de ses pistes>
        .register_asynchronous_uri_scheme_protocol("artist", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let name = percent_decode(request.uri().path().trim_start_matches('/'));
            let track = request
                .uri()
                .query()
                .unwrap_or("")
                .split('&')
                .find_map(|kv| kv.strip_prefix("t="))
                .map(percent_decode)
                .unwrap_or_default();
            std::thread::spawn(move || {
                let (root, token) = match app.try_state::<AppState>() {
                    Some(st) => {
                        let s = st.settings.lock().unwrap();
                        (s.root_of(std::path::Path::new(&track)).map(|p| p.to_string_lossy().into_owned()), s.discogs_token.clone())
                    }
                    None => (None, String::new()),
                };
                let cache = app.path().app_cache_dir().unwrap_or_default();
                let img = artist::image(&name, std::path::Path::new(&track), root.as_deref().map(std::path::Path::new), &cache, &token);
                responder.respond(image_response(img));
            });
        })
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&config_dir)?;
            let settings_path = config_dir.join("settings.json");
            let settings = Settings::load(&settings_path);
            library::set_skip(&settings.ignore_ext);
            eq::set(settings.eq.clone());
            let cache_dir = app.path().app_cache_dir()?;
            std::fs::create_dir_all(&cache_dir)?;
            let library_path = cache_dir.join("library-v2.json");

            // Dossiers où chercher les DLL BASS, du plus spécifique au plus général.
            let mut dirs = vec![];
            if let Ok(r) = app.path().resource_dir() {
                dirs.push(r.join("lib"));
            }
            if let Some(d) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
                dirs.push(d.join("lib"));
                dirs.push(d);
            }
            dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lib"));

            let handle = app.handle().clone();
            let phone = phone::Phone::new(app.handle().clone(), &cache_dir);
            let ph = phone.clone();
            let engine = engine::start(
                dirs,
                settings.output.clone(),
                settings.volume,
                settings.audio.clone(),
                Box::new(move |s| {
                    let _ = handle.emit("player-state", s.clone());
                    if let Ok(j) = serde_json::to_string(s) {
                        ph.broadcast(j);
                    }
                }),
            );
            let store = manage::Store { dir: app.path().app_data_dir()?.join("historique"), cache: library_path.clone() };
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let coll = Arc::new(collections::Store::load(data_dir.join("collections.json")));
            {
                let handle = app.handle().clone();
                let ph = phone.clone();
                coll.on_change(Box::new(move |c| {
                    let _ = handle.emit("collections", c.clone());
                    if let Ok(j) = serde_json::to_string(c) {
                        ph.event("collections", &j);
                    }
                }));
            }
            let stats_store = Arc::new(stats::Store::load(data_dir.join("ecoutes.json")));
            app.manage(AppState {
                engine,
                settings: Mutex::new(settings),
                settings_path,
                library_path,
                store,
                phone: phone.clone(),
                collections: coll,
                stats: stats_store,
                lyrics: lyrics::Store::load(data_dir.join("paroles.json")),
                data_dir: data_dir.clone(),
            });

            // Contrôles multimédia de Windows (sur le fil principal, celui de la fenêtre).
            if let Some(w) = app.get_webview_window("main") {
                use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                if let Ok(h) = w.window_handle() {
                    if let RawWindowHandle::Win32(h) = h.as_raw() {
                        let ah = app.handle().clone();
                        let ok = smtc::init(h.hwnd.get() as *mut std::ffi::c_void, move |e| {
                            let Some(st) = ah.try_state::<AppState>() else { return };
                            let cmd = match e {
                                smtc::Event::Toggle | smtc::Event::Play | smtc::Event::Pause => Cmd::Toggle,
                                smtc::Event::Next => Cmd::Next,
                                smtc::Event::Prev => Cmd::Prev,
                                smtc::Event::Stop => Cmd::Stop,
                                smtc::Event::Seek(p) => Cmd::Seek(p),
                            };
                            st.engine.send(cmd);
                        });
                        SMTC_OK.store(ok, std::sync::atomic::Ordering::Release);
                    }
                }
            }

            // Zone de notification.
            {
                use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
                let menu = tray_menu(app.handle(), false)?;
                let mut tb = TrayIconBuilder::with_id("main").tooltip("MusicPi").menu(&menu).show_menu_on_left_click(false);
                if let Some(icon) = app.default_window_icon() {
                    tb = tb.icon(icon.clone());
                }
                tb.on_menu_event(|app, event| {
                    let cmd = match event.id().as_ref() {
                        "show" => return show_main(app),
                        "quit" => {
                            QUITTING.store(true, std::sync::atomic::Ordering::Release);
                            return app.exit(0);
                        }
                        "toggle" => Cmd::Toggle,
                        "prev" => Cmd::Prev,
                        "next" => Cmd::Next,
                        _ => return,
                    };
                    if let Some(st) = app.try_state::<AppState>() {
                        st.engine.send(cmd);
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;
            }

            // Lancé au démarrage de Windows : on reste dans la zone de notification.
            if std::env::args().any(|a| a == "--minimized") {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            start_watch(app.handle().clone());
            // Serveur pour l'app téléphone, s'il est activé.
            std::thread::spawn(move || phone.apply());

            // Touches multimédia du clavier, actives même quand l'app est en arrière-plan.
            {
                use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Shortcut, ShortcutState};
                app.handle().plugin(
                    tauri_plugin_global_shortcut::Builder::new()
                        .with_handler(|app, shortcut, event| {
                            if event.state() != ShortcutState::Pressed {
                                return;
                            }
                            let cmd = match shortcut.key {
                                Code::MediaPlayPause => Cmd::Toggle,
                                Code::MediaTrackNext => Cmd::Next,
                                Code::MediaTrackPrevious => Cmd::Prev,
                                Code::MediaStop => Cmd::Stop,
                                _ => return,
                            };
                            if let Some(st) = app.try_state::<AppState>() {
                                st.engine.send(cmd);
                            }
                        })
                        .build(),
                )?;
                // Avec les contrôles multimédia de Windows, les touches passent déjà par eux.
                let keys = if SMTC_OK.load(std::sync::atomic::Ordering::Acquire) { vec![] } else { vec![Code::MediaPlayPause, Code::MediaTrackNext, Code::MediaTrackPrevious, Code::MediaStop] };
                for code in keys {
                    // Échoue si une autre application (Spotify…) a déjà pris la touche : on continue sans.
                    if let Err(e) = app.global_shortcut().register(Shortcut::new(None, code)) {
                        eprintln!("Touche multimédia {code:?} indisponible : {e}");
                    }
                }
            }
            Ok(())
        })
        // Fermer = ranger dans la zone de notification (si demandé).
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let tray = window.app_handle().try_state::<AppState>().map(|st| st.settings.lock().unwrap().tray_close).unwrap_or(false);
                if tray && !QUITTING.load(std::sync::atomic::Ordering::Acquire) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_lyrics,
            set_lyrics_offset,
            set_lyrics_prefs,
            lyrics_test,
            set_tray_lang,
            play,
            toggle,
            next,
            prev,
            seek,
            set_volume,
            set_practice,
            set_tempo,
            set_loop,
            set_output,
            set_eq,
            set_shortcuts,
            set_theme,
            list_devices,
            get_settings,
            pick_folder,
            load_library,
            scan_library,
            edit_tags,
            set_cover,
            pick_image,
            search_covers,
            search_releases,
            release_details,
            move_files,
            history,
            undo,
            set_tools,
            search_artist_images,
            set_artist_image,
            add_library_folder,
            remove_library_folder,
            set_library_prefs,
            set_ui,
            phone_status,
            set_phone,
            phone_new_code,
            phone_revoke,
            get_collections,
            collection_op,
            record_play,
            get_stats,
            media_update,
            media_stopped,
            lastfm_login,
            lastfm_set,
            get_system,
            set_system,
            export_backup,
            import_backup,
            set_accent,
            windows_accent,
            keep_awake,
            idle_seconds,
            set_ambient,
            set_audio,
            set_play_prefs,
            save_session,
            restore_session
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de l'application");
}
