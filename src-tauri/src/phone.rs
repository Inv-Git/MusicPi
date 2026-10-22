//! Liaison avec l'app Android : appairage, bibliothèque, pochettes, fichiers audio,
//! état du lecteur en direct et télécommande.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::engine::Cmd;
use crate::remote::{self, Request, Response};
use crate::{artist, library, AppState};

pub const DEFAULT_PORT: u16 = 47800;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PhoneSettings {
    pub enabled: bool,
    pub port: u16,
    pub devices: Vec<PhoneDevice>,
}

impl Default for PhoneSettings {
    fn default() -> Self {
        Self { enabled: false, port: DEFAULT_PORT, devices: vec![] }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PhoneDevice {
    pub name: String,
    pub token: String,
    /// Date d'appairage (secondes depuis 1970).
    pub added: u64,
}

impl Default for PhoneDevice {
    fn default() -> Self {
        Self { name: String::new(), token: String::new(), added: 0 }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneStatus {
    pub enabled: bool,
    pub port: u16,
    pub running: bool,
    pub error: Option<String>,
    pub addresses: Vec<String>,
    pub code: String,
    pub devices: Vec<DeviceInfo>,
}

#[derive(Serialize)]
pub struct DeviceInfo {
    pub name: String,
    pub added: u64,
}

struct Pairing {
    code: String,
    fails: u32,
}

/// Nombre d'essais de code faux avant de bloquer l'appairage (jusqu'à un nouveau code).
const MAX_FAILS: u32 = 5;

pub struct Phone {
    app: AppHandle,
    server: Mutex<Option<remote::Server>>,
    clients: Mutex<Vec<Sender<String>>>,
    last_state: Mutex<String>,
    pairing: Mutex<Pairing>,
    error: Mutex<Option<String>>,
    thumbs: PathBuf,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Réduit une image en JPEG de `size` px au plus (pochettes légères pour la 4G).
fn thumbnail(bytes: &[u8], size: u32) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width() > size || img.height() > size { img.thumbnail(size, size) } else { img };
    let rgb = img.to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85).encode_image(&rgb).ok()?;
    Some(out)
}

fn mime_of(p: &Path) -> &'static str {
    match p.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref() {
        Some("flac") => "audio/flac",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("aif") | Some("aiff") => "audio/aiff",
        Some("ogg") => "audio/ogg",
        Some("m4a") => "audio/mp4",
        _ => "application/octet-stream",
    }
}

impl Phone {
    pub fn new(app: AppHandle, cache_dir: &Path) -> Arc<Phone> {
        Arc::new(Phone {
            app,
            server: Mutex::new(None),
            clients: Mutex::new(vec![]),
            last_state: Mutex::new("{}".into()),
            pairing: Mutex::new(Pairing { code: remote::random_code(), fails: 0 }),
            error: Mutex::new(None),
            thumbs: cache_dir.join("miniatures"),
        })
    }

    /// Appelé à chaque mise à jour de l'état du lecteur.
    pub fn broadcast(&self, state_json: String) {
        *self.last_state.lock().unwrap() = state_json.clone();
        let mut c = self.clients.lock().unwrap();
        if !c.is_empty() {
            c.retain(|tx| tx.send(state_json.clone()).is_ok());
        }
    }

    /// Événement nommé envoyé aux téléphones connectés (ex. cœurs et playlists modifiés).
    pub fn event(&self, name: &str, json: &str) {
        let msg = format!("event: {name}\n{json}");
        let mut c = self.clients.lock().unwrap();
        if !c.is_empty() {
            c.retain(|tx| tx.send(msg.clone()).is_ok());
        }
    }

    /// Démarre ou arrête le serveur selon les réglages.
    pub fn apply(self: &Arc<Self>) {
        let (enabled, port) = {
            let st = self.app.state::<AppState>();
            let s = st.settings.lock().unwrap();
            (s.phone.enabled, s.phone.port)
        };
        let mut srv = self.server.lock().unwrap();
        let same = srv.as_ref().map(|s| s.port() == port).unwrap_or(false);
        if !enabled {
            *srv = None;
            self.clients.lock().unwrap().clear();
            *self.error.lock().unwrap() = None;
            return;
        }
        if same {
            return;
        }
        *srv = None;
        match remote::Server::start(port, Arc::new(Routes(self.clone()))) {
            Ok(s) => {
                *srv = Some(s);
                *self.error.lock().unwrap() = None;
            }
            Err(e) => *self.error.lock().unwrap() = Some(format!("Port {port} indisponible ({e})")),
        }
    }

    pub fn status(&self) -> PhoneStatus {
        let st = self.app.state::<AppState>();
        let s = st.settings.lock().unwrap();
        PhoneStatus {
            enabled: s.phone.enabled,
            port: s.phone.port,
            running: self.server.lock().unwrap().is_some(),
            error: self.error.lock().unwrap().clone(),
            addresses: remote::local_addresses(),
            code: self.pairing.lock().unwrap().code.clone(),
            devices: s.phone.devices.iter().map(|d| DeviceInfo { name: d.name.clone(), added: d.added }).collect(),
        }
    }

    pub fn new_code(&self) {
        *self.pairing.lock().unwrap() = Pairing { code: remote::random_code(), fails: 0 };
    }

    fn library_roots(&self) -> Vec<PathBuf> {
        let st = self.app.state::<AppState>();
        let roots = st.settings.lock().unwrap().roots();
        roots.into_iter().filter_map(|r| std::fs::canonicalize(r).ok()).collect()
    }

    /// Le chemin demandé doit être un fichier de la bibliothèque, rien d'autre.
    fn allowed(&self, p: &str) -> Option<PathBuf> {
        let c = std::fs::canonicalize(p).ok()?;
        (c.is_file() && self.library_roots().iter().any(|r| c.starts_with(r))).then(|| PathBuf::from(p))
    }

    /// Fichier Opus 160 kbit/s gardé en cache (créé à la première demande).
    fn light_version(&self, p: &Path) -> Option<PathBuf> {
        if crate::engine::api().map(|a| a.enc_opus.is_none()).unwrap_or(true) {
            return None;
        }
        let dir = self.thumbs.parent()?.join("opus");
        std::fs::create_dir_all(&dir).ok()?;
        let mtime = std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        let out = dir.join(format!("{:016x}.opus", fnv(&format!("{}|{mtime}", p.display()))));
        if out.is_file() {
            return Some(out);
        }
        // Une conversion à la fois (le téléphone peut demander plusieurs titres d'un coup).
        static LOCK: Mutex<()> = Mutex::new(());
        let _g = LOCK.lock();
        if out.is_file() {
            return Some(out);
        }
        crate::engine::transcode_opus(p, &out, 160).ok()?;
        Some(out)
    }

    fn authorized(&self, req: &Request) -> bool {
        let token = req
            .header("authorization")
            .and_then(|h| h.strip_prefix("Bearer "))
            .map(|s| s.trim().to_string())
            .or_else(|| req.query.get("t").cloned())
            .unwrap_or_default();
        if token.len() < 16 {
            return false;
        }
        let st = self.app.state::<AppState>();
        let s = st.settings.lock().unwrap();
        s.phone.devices.iter().any(|d| d.token == token)
    }

    fn pair(&self, req: &Request) -> Response {
        let v: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
        let code = v["code"].as_str().unwrap_or("").trim().to_string();
        let name = v["device"].as_str().unwrap_or("Téléphone").chars().take(60).collect::<String>();
        {
            let mut p = self.pairing.lock().unwrap();
            if p.fails >= MAX_FAILS {
                return Response::error(429, "Trop d'essais : génère un nouveau code sur le PC.");
            }
            if code != p.code {
                p.fails += 1;
                return Response::error(403, "Code incorrect.");
            }
            // Code à usage unique.
            *p = Pairing { code: remote::random_code(), fails: 0 };
        }
        let token = remote::random_hex(24);
        let st = self.app.state::<AppState>();
        st.update(|s| s.phone.devices.push(PhoneDevice { name: name.clone(), token: token.clone(), added: now() }));
        let _ = self.app.emit("phone-changed", ());
        Response::json(json!({ "token": token, "name": computer_name() }).to_string())
    }

    fn image(&self, bytes: Option<Vec<u8>>, key: &str, size: u32) -> Response {
        let Some(b) = bytes else { return Response::error(404, "pas d'image") };
        let _ = std::fs::create_dir_all(&self.thumbs);
        let cached = self.thumbs.join(format!("{:016x}-{size}.jpg", fnv(key)));
        let out = std::fs::read(&cached).ok().or_else(|| {
            let t = thumbnail(&b, size)?;
            let _ = std::fs::write(&cached, &t);
            Some(t)
        });
        let resp = match out {
            Some(t) => Response::bytes(200, "image/jpeg", t),
            None => {
                let mime = if b.starts_with(&[0x89, b'P', b'N', b'G']) { "image/png" } else { "image/jpeg" };
                Response::bytes(200, mime, b)
            }
        };
        resp.with_header("Cache-Control", "private, max-age=604800")
    }

    fn command(&self, req: &Request) -> Response {
        let v: Value = match serde_json::from_slice(&req.body) {
            Ok(v) => v,
            Err(_) => return Response::error(400, "JSON invalide"),
        };
        let st = self.app.state::<AppState>();
        let f = |k: &str| v[k].as_f64();
        let cmd = match v["cmd"].as_str().unwrap_or("") {
            "play" => {
                let queue: Vec<PathBuf> = v["paths"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|p| p.as_str()).filter_map(|p| self.allowed(p)).collect())
                    .unwrap_or_default();
                if queue.is_empty() {
                    return Response::error(400, "Aucune piste");
                }
                let index = v["index"].as_u64().unwrap_or(0) as usize;
                Cmd::Play { index: index.min(queue.len() - 1), queue }
            }
            "toggle" => Cmd::Toggle,
            "next" => Cmd::Next,
            "prev" => Cmd::Prev,
            "stop" => Cmd::Stop,
            "seek" => Cmd::Seek(f("position").unwrap_or(0.0)),
            "volume" => {
                let vol = f("volume").unwrap_or(1.0).clamp(0.0, 1.0) as f32;
                st.update(|s| s.volume = vol);
                Cmd::SetVolume(vol)
            }
            "practice" => Cmd::SetPractice(v["on"].as_bool().unwrap_or(false)),
            "tempo" => Cmd::SetTempo(f("tempo").unwrap_or(0.0) as f32),
            "loop" => Cmd::SetLoop(match (f("start"), f("end")) {
                (Some(a), Some(b)) if b > a => Some((a, b)),
                _ => None,
            }),
            other => return Response::error(400, &format!("Commande inconnue : {other}")),
        };
        st.engine.send(cmd);
        Response::bytes(204, "text/plain", vec![])
    }
}

fn computer_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into())
}

struct Routes(Arc<Phone>);

impl remote::Handler for Routes {
    fn handle(&self, req: &Request) -> Response {
        let ph = &self.0;
        let get = req.method == "GET" || req.method == "HEAD";
        match (get, req.path.as_str()) {
            (true, "/api/info") => {
                let port = ph.server.lock().unwrap().as_ref().map(|s| s.port()).unwrap_or(0);
                return Response::json(
                    json!({
                        "app": "MusicPi",
                        "version": env!("CARGO_PKG_VERSION"),
                        "name": computer_name(),
                        "addresses": remote::local_addresses(),
                        "port": port,
                    })
                    .to_string(),
                );
            }
            (false, "/api/pair") if req.method == "POST" => return ph.pair(req),
            _ => {}
        }
        if !ph.authorized(req) {
            return Response::error(401, "Appareil non appairé");
        }
        let q = |k: &str| req.query.get(k).cloned().unwrap_or_default();
        match (get, req.path.as_str()) {
            (true, "/api/library") => {
                let st = ph.app.state::<AppState>();
                let p = st.library_path.clone();
                let Ok(m) = std::fs::metadata(&p) else { return Response::json("[]".into()) };
                let mtime = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
                let etag = format!("\"{}-{}\"", m.len(), mtime);
                if req.header("if-none-match") == Some(etag.as_str()) {
                    return Response::bytes(304, "application/json", vec![]).with_header("ETag", &etag);
                }
                Response::file(p, "application/json; charset=utf-8").with_header("ETag", &etag)
            }
            (true, "/api/cover") => {
                let Some(p) = ph.allowed(&q("p")) else { return Response::error(404, "piste inconnue") };
                let size = q("s").parse().unwrap_or(400u32).clamp(64, 1200);
                let mtime = std::fs::metadata(&p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
                let key = format!("cover|{}|{mtime}", p.display());
                ph.image(library::cover(&p).map(|(b, _)| b), &key, size)
            }
            (true, "/api/artist") => {
                let name = q("n");
                let track = ph.allowed(&q("t")).unwrap_or_default();
                let size = q("s").parse().unwrap_or(400u32).clamp(64, 1200);
                let st = ph.app.state::<AppState>();
                let (root, token) = {
                    let s = st.settings.lock().unwrap();
                    (s.root_of(&track), s.discogs_token.clone())
                };
                let cache = ph.app.path().app_cache_dir().unwrap_or_default();
                let img = artist::image(&name, &track, root.as_deref(), &cache, &token);
                ph.image(img, &format!("artist|{}", artist::normalize(&name)), size)
            }
            (true, "/api/file") => match ph.allowed(&q("p")) {
                Some(p) if library::is_audio(&p) => {
                    // Version allégée (Opus) demandée par le téléphone en données mobiles.
                    if q("q") == "low" {
                        if let Some(o) = ph.light_version(&p) {
                            return Response::file(o, "audio/ogg").with_header("X-Music-Quality", "opus");
                        }
                    }
                    let mime = mime_of(&p);
                    Response::file(p, mime)
                }
                _ => Response::error(404, "fichier inconnu"),
            },
            (true, "/api/caps") => Response::json(json!({ "opus": crate::engine::api().map(|a| a.enc_opus.is_some()).unwrap_or(false) }).to_string()),
            (true, "/api/state") => Response::json(ph.last_state.lock().unwrap().clone()),
            (true, "/api/events") => {
                let (tx, rx) = mpsc::channel();
                let _ = tx.send(ph.last_state.lock().unwrap().clone());
                let coll = ph.app.state::<AppState>().collections.get();
                if let Ok(j) = serde_json::to_string(&coll) {
                    let _ = tx.send(format!("event: collections\n{j}"));
                }
                ph.clients.lock().unwrap().push(tx);
                Response::events(rx)
            }
            (false, "/api/cmd") => ph.command(req),
            // Écoute terminée sur le téléphone : compteurs et Last.fm.
            (false, "/api/played") => {
                let v: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
                let Some(p) = v["path"].as_str().and_then(|p| ph.allowed(p)) else { return Response::error(404, "piste inconnue") };
                let t = library::read(&p);
                crate::on_played(&ph.app, &t.path, t.artist, t.title, t.album, t.duration);
                Response::bytes(204, "text/plain", vec![])
            }
            (true, "/api/lyrics") => match ph.allowed(&q("p")) {
                Some(p) => {
                    let out = crate::lyrics_for(&ph.app, &p.to_string_lossy(), false);
                    Response::json(serde_json::to_string(&out).unwrap_or_default())
                }
                None => Response::error(404, "piste inconnue"),
            },
            (true, "/api/collections") => {
                let c = ph.app.state::<AppState>().collections.get();
                Response::json(serde_json::to_string(&c).unwrap_or_default())
            }
            (false, "/api/collections") => {
                let op: crate::collections::Op = match serde_json::from_slice(&req.body) {
                    Ok(o) => o,
                    Err(e) => return Response::error(400, &format!("Opération invalide : {e}")),
                };
                match ph.app.state::<AppState>().collections.apply(op) {
                    Ok((c, id)) => Response::json(json!({ "collections": c, "id": id }).to_string()),
                    Err(e) => Response::error(409, &e),
                }
            }
            _ => Response::error(404, "inconnu"),
        }
    }
}
