//! Paroles : fichier `.lrc` à côté du morceau, tags du fichier, puis LRCLIB (lrclib.net) en ligne.
//! Les paroles synchronisées (une heure par ligne) permettent d'allumer la ligne en cours.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Lyrics {
    /// Chaque ligne a une heure de début.
    pub synced: bool,
    pub lines: Vec<Line>,
    /// « lrc » (fichier à côté), « tags » (dans le fichier audio) ou « lrclib ».
    pub source: String,
    /// Morceau instrumental (pas de paroles).
    pub instrumental: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Line {
    /// Début de la ligne en secondes (paroles synchronisées).
    pub t: Option<f64>,
    pub text: String,
}

/// Réponse envoyée à l'interface et au téléphone.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsOut {
    pub lyrics: Option<Lyrics>,
    /// Décalage choisi pour ce morceau, en millisecondes (positif : les paroles arrivent plus tard).
    pub offset: i32,
}

// ---------------------------------------------------------------------------
// Lecture du format LRC
// ---------------------------------------------------------------------------

/// Lit « mm:ss.xx » (ou « mm:ss », « mm:ss:xx ») en secondes.
fn parse_time(s: &str) -> Option<f64> {
    let (m, rest) = s.split_once(':')?;
    let m: f64 = m.trim().parse().ok()?;
    let rest = rest.trim().replacen(':', ".", 1);
    let sec: f64 = rest.parse().ok()?;
    (m >= 0.0 && sec >= 0.0 && sec < 60.0).then_some(m * 60.0 + sec)
}

/// Découpe un texte de paroles. Synchronisé s'il contient des heures « [mm:ss.xx] ».
pub fn parse(text: &str) -> (bool, Vec<Line>) {
    let text = text.trim_start_matches('\u{feff}');
    let mut timed: Vec<Line> = Vec::new();
    let mut plain: Vec<Line> = Vec::new();
    let mut offset = 0.0;
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut times = Vec::new();
        let mut meta = false;
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(end) = stripped.find(']') else { break };
            let inner = &stripped[..end];
            if let Some(t) = parse_time(inner) {
                times.push(t);
            } else if let Some((k, v)) = inner.split_once(':') {
                // Étiquettes [ar:…], [ti:…], [offset:+250]…
                if k.trim().eq_ignore_ascii_case("offset") {
                    offset = v.trim().parse::<f64>().unwrap_or(0.0) / 1000.0;
                }
                meta = true;
            } else {
                break;
            }
            rest = stripped[end + 1..].trim_start();
        }
        if !times.is_empty() {
            for t in times {
                timed.push(Line { t: Some(t), text: rest.trim().to_string() });
            }
        } else if !meta {
            plain.push(Line { t: None, text: raw.trim().to_string() });
        }
    }
    if !timed.is_empty() {
        // [offset:+x] : les paroles arrivent x ms plus tôt.
        for l in &mut timed {
            l.t = l.t.map(|t| (t - offset).max(0.0));
        }
        timed.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
        return (true, timed);
    }
    // Texte simple : on retire les lignes vides au début et à la fin, on garde les blancs entre couplets.
    while plain.first().is_some_and(|l| l.text.is_empty()) {
        plain.remove(0);
    }
    while plain.last().is_some_and(|l| l.text.is_empty()) {
        plain.pop();
    }
    (false, plain)
}

/// Écrit des paroles au format LRC (synchronisées) ou en texte simple.
pub fn to_lrc(l: &Lyrics) -> String {
    let mut out = String::new();
    for line in &l.lines {
        match line.t {
            Some(t) if l.synced => {
                let m = (t / 60.0).floor() as u32;
                let s = t - m as f64 * 60.0;
                out.push_str(&format!("[{m:02}:{s:05.2}]{}\n", line.text));
            }
            _ => {
                out.push_str(&line.text);
                out.push('\n');
            }
        }
    }
    out
}

fn from_text(text: &str, source: &str) -> Option<Lyrics> {
    let (synced, lines) = parse(text);
    lines.iter().any(|l| !l.text.is_empty()).then(|| Lyrics { synced, lines, source: source.into(), instrumental: false })
}

// ---------------------------------------------------------------------------
// Sur le disque
// ---------------------------------------------------------------------------

fn sidecar(path: &Path) -> PathBuf {
    path.with_extension("lrc")
}

/// Paroles disponibles sans réseau : fichier `.lrc` puis tags du fichier audio.
pub fn local(path: &Path) -> Option<Lyrics> {
    for p in [sidecar(path), path.with_extension("LRC")] {
        if let Ok(bytes) = std::fs::read(&p) {
            if let Some(l) = from_text(&String::from_utf8_lossy(&bytes), "lrc") {
                return Some(l);
            }
        }
    }
    use lofty::prelude::*;
    use lofty::tag::ItemKey;
    let tf = lofty::read_from_path(path).ok()?;
    let mut best: Option<Lyrics> = None;
    for tag in tf.tags() {
        for item in tag.items() {
            let name = match item.key() {
                ItemKey::Lyrics => "LYRICS".to_string(),
                ItemKey::Unknown(k) => k.to_ascii_uppercase(),
                _ => continue,
            };
            if !matches!(name.as_str(), "LYRICS" | "UNSYNCEDLYRICS" | "SYNCEDLYRICS" | "UNSYNCED LYRICS") {
                continue;
            }
            if let Some(l) = item.value().text().and_then(|t| from_text(t, "tags")) {
                // On préfère une version synchronisée si les tags en ont une.
                if best.as_ref().map(|b| !b.synced && l.synced).unwrap_or(true) {
                    best = Some(l);
                }
            }
        }
    }
    best
}

/// Enregistre les paroles en `.lrc` à côté du morceau (sans écraser un fichier existant).
pub fn save_sidecar(path: &Path, l: &Lyrics) -> Result<(), String> {
    let p = sidecar(path);
    if p.exists() {
        return Ok(());
    }
    std::fs::write(&p, to_lrc(l)).map_err(|e| format!("{} : {e}", p.display()))
}

// ---------------------------------------------------------------------------
// LRCLIB
// ---------------------------------------------------------------------------

fn from_lrclib(v: &Value) -> Option<Lyrics> {
    if v["instrumental"].as_bool() == Some(true) {
        return Some(Lyrics { synced: false, lines: vec![], source: "lrclib".into(), instrumental: true });
    }
    if let Some(l) = v["syncedLyrics"].as_str().and_then(|t| from_text(t, "lrclib")) {
        return Some(l);
    }
    v["plainLyrics"].as_str().and_then(|t| from_text(t, "lrclib"))
}

/// Cherche sur lrclib.net. `Ok(None)` : rien trouvé.
pub fn lrclib(artist: &str, title: &str, album: &str, duration: f64, exact_only: bool) -> Result<Option<Lyrics>, String> {
    if title.trim().is_empty() {
        return Ok(None);
    }
    let agent = crate::online::agent();
    let mut req = agent.get("https://lrclib.net/api/get").query("artist_name", artist).query("track_name", title);
    if !album.is_empty() {
        req = req.query("album_name", album);
    }
    if duration > 0.0 {
        req = req.query("duration", &format!("{}", duration.round() as u64));
    }
    match req.call() {
        Ok(resp) => {
            let v: Value = resp.into_json().map_err(|e| e.to_string())?;
            if let Some(l) = from_lrclib(&v) {
                return Ok(Some(l));
            }
        }
        Err(ureq::Error::Status(404, _)) => {}
        Err(e) => return Err(e.to_string()),
    }
    if exact_only {
        return Ok(None);
    }
    // Recherche plus souple : même titre et même artiste, durée la plus proche (3 s d'écart au plus).
    let resp = agent
        .get("https://lrclib.net/api/search")
        .query("track_name", title)
        .query("artist_name", artist)
        .call()
        .map_err(|e| e.to_string())?;
    let list: Vec<Value> = resp.into_json().map_err(|e| e.to_string())?;
    let mut best: Option<(f64, bool, &Value)> = None;
    for v in &list {
        let d = v["duration"].as_f64().unwrap_or(0.0);
        let diff = if duration > 0.0 && d > 0.0 { (d - duration).abs() } else { 0.0 };
        if diff > 3.0 {
            continue;
        }
        let synced = v["syncedLyrics"].as_str().is_some_and(|s| !s.is_empty());
        let better = match best {
            None => true,
            Some((bd, bs, _)) => (synced && !bs) || (synced == bs && diff < bd),
        };
        if better {
            best = Some((diff, synced, v));
        }
    }
    Ok(best.and_then(|(_, _, v)| from_lrclib(v)))
}

// ---------------------------------------------------------------------------
// Cache (paroles trouvées en ligne, recherches sans résultat, décalages)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub lyrics: Option<Lyrics>,
    /// Dernière recherche en ligne (secondes depuis 1970).
    pub checked: u64,
    pub offset: i32,
}

pub struct Store {
    path: PathBuf,
    map: Mutex<HashMap<String, Entry>>,
}

/// Une recherche sans résultat n'est refaite qu'au bout de 7 jours.
pub const RETRY_SECS: u64 = 7 * 24 * 3600;

impl Store {
    pub fn load(path: PathBuf) -> Self {
        let map = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Self { path, map: Mutex::new(map) }
    }

    pub fn get(&self, key: &str) -> Entry {
        self.map.lock().unwrap().get(key).cloned().unwrap_or_default()
    }

    pub fn update(&self, key: &str, f: impl FnOnce(&mut Entry)) {
        let mut map = self.map.lock().unwrap();
        f(map.entry(key.to_string()).or_default());
        if let Ok(s) = serde_json::to_string(&*map) {
            let tmp = self.path.with_extension("tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(&tmp, &self.path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_synchronise() {
        let (s, l) = parse("\u{feff}[ar:Moi]\n[offset:+500]\n[00:12.50]Un\n[00:05.00][01:02.25]Deux\n\n[00:20.00]");
        assert!(s);
        assert_eq!(l.len(), 4);
        assert_eq!(l[0], Line { t: Some(4.5), text: "Deux".into() });
        assert_eq!(l[1].text, "Un");
        assert!((l[1].t.unwrap() - 12.0).abs() < 1e-9);
        assert_eq!(l[2].text, "");
        assert!((l[3].t.unwrap() - 61.75).abs() < 1e-9);
    }

    #[test]
    fn texte_simple() {
        let (s, l) = parse("\n\nCouplet un\nsuite\n\nRefrain\n\n");
        assert!(!s);
        assert_eq!(l.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), vec!["Couplet un", "suite", "", "Refrain"]);
    }

    #[test]
    fn aller_retour_lrc() {
        let l = Lyrics { synced: true, lines: vec![Line { t: Some(84.3), text: "Reviens".into() }], source: "lrclib".into(), instrumental: false };
        assert_eq!(to_lrc(&l), "[01:24.30]Reviens\n");
        let (s, back) = parse(&to_lrc(&l));
        assert!(s);
        assert!((back[0].t.unwrap() - 84.3).abs() < 1e-6);
    }

    #[test]
    fn heures() {
        assert_eq!(parse_time("01:02.5"), Some(62.5));
        assert_eq!(parse_time("1:02:50"), Some(62.5));
        assert_eq!(parse_time("ar"), None);
    }
}
