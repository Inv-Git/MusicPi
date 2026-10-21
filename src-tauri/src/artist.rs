//! Photos d'artistes : image du dossier de l'artiste (artist.jpg…), sinon photo
//! récupérée en ligne (Deezer, puis Discogs) et gardée en cache sur le disque.
//! Sans photo, l'interface affiche la pochette d'un album.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use crate::online;

/// Une seule recherche en ligne à la fois, pour rester poli avec les services.
static ONLINE: Mutex<()> = Mutex::new(());

const LOCAL_NAMES: [&str; 5] = ["artist.jpg", "artist.jpeg", "artist.png", "folder.jpg", "folder.png"];
/// Délai avant de retenter un artiste pour qui aucune photo n'a été trouvée.
const RETRY_AFTER: Duration = Duration::from_secs(30 * 24 * 3600);

/// Dossier de l'artiste : parent du dossier de l'album, s'il est sous la racine
/// de la bibliothèque (et n'est pas la racine elle-même).
pub fn artist_dir(track: &Path, root: Option<&Path>) -> Option<PathBuf> {
    let album = track.parent()?;
    let dir = album.parent()?;
    if let Some(r) = root {
        if dir == r || !dir.starts_with(r) {
            return None;
        }
    }
    Some(dir.to_path_buf())
}

pub fn normalize(name: &str) -> String {
    name.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

fn cache_base(cache_dir: &Path, name: &str) -> PathBuf {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in normalize(name).bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    cache_dir.join("artistes").join(format!("{h:016x}"))
}

pub fn image(name: &str, track: &Path, root: Option<&Path>, cache_dir: &Path, token: &str) -> Option<Vec<u8>> {
    if let Some(d) = artist_dir(track, root) {
        for n in LOCAL_NAMES {
            if let Ok(b) = fs::read(d.join(n)) {
                return Some(b);
            }
        }
    }
    let base = cache_base(cache_dir, name);
    let img = base.with_extension("img");
    let none = base.with_extension("none");
    if let Ok(b) = fs::read(&img) {
        return Some(b);
    }
    let recent = |p: &Path| {
        fs::metadata(p)
            .and_then(|m| m.modified())
            .map(|t| SystemTime::now().duration_since(t).unwrap_or_default() < RETRY_AFTER)
            .unwrap_or(false)
    };
    if recent(&none) {
        return None;
    }
    let _guard = ONLINE.lock().unwrap_or_else(|e| e.into_inner());
    // Un autre appel a peut-être trouvé l'image pendant l'attente.
    if let Ok(b) = fs::read(&img) {
        return Some(b);
    }
    let _ = fs::create_dir_all(base.parent().unwrap());
    let wanted = normalize(name);
    let found = online::artist_hits(name, token)
        .0
        .into_iter()
        .find(|h| normalize(&h.artist) == wanted)
        .and_then(|h| online::download(&h.url).ok());
    match found {
        Some(b) => {
            let _ = fs::write(&img, &b);
            let _ = fs::remove_file(&none);
            Some(b)
        }
        None => {
            let _ = fs::write(&none, b"");
            None
        }
    }
}

/// Enregistre la photo choisie : artist.jpg dans le dossier de l'artiste quand il
/// existe, et dans le cache dans tous les cas.
pub fn save(name: &str, track: &Path, root: Option<&Path>, cache_dir: &Path, data: &[u8]) -> Result<(), String> {
    let base = cache_base(cache_dir, name);
    fs::create_dir_all(base.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(base.with_extension("img"), data).map_err(|e| e.to_string())?;
    let _ = fs::remove_file(base.with_extension("none"));
    if let Some(d) = artist_dir(track, root) {
        let png = data.starts_with(&[0x89, b'P', b'N', b'G']);
        for n in LOCAL_NAMES.iter().filter(|n| n.starts_with("artist")) {
            let _ = fs::remove_file(d.join(n));
        }
        fs::write(d.join(if png { "artist.png" } else { "artist.jpg" }), data).map_err(|e| e.to_string())?;
    }
    Ok(())
}
