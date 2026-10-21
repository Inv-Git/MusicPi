//! Scan d'un dossier musical et lecture des tags.

use std::path::{Path, PathBuf};

use lofty::prelude::*;
use lofty::picture::PictureType;
use lofty::tag::ItemKey;
use serde::{Deserialize, Serialize};

const EXTENSIONS: &[&str] = &["flac", "mp3", "wav", "aif", "aiff", "ogg", "m4a"];

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Track {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub duration: f64,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u8>,
    pub channels: Option<u8>,
    /// Débit moyen en kbit/s.
    pub bitrate: Option<u32>,
    pub year: Option<u32>,
    pub genre: String,
    /// Taille du fichier en octets.
    pub size: u64,
    pub format: String,
    /// Date d'ajout du fichier (secondes depuis 1970), pour « Ajoutés récemment ».
    pub added: u64,
}

/// Accès au fichier cache : un seul écrivain à la fois (scan, modifications, surveillance).
pub static CACHE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Formats ignorés (extensions en minuscules, sans point).
static SKIP: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

pub fn set_skip(exts: &[String]) {
    *SKIP.lock().unwrap() = exts.iter().map(|e| e.trim_start_matches('.').to_lowercase()).collect();
}

fn skipped(p: &Path) -> bool {
    let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
    SKIP.lock().map(|s| s.contains(&ext)).unwrap_or(false)
}

/// Toutes les racines de la bibliothèque.
pub fn scan_all(roots: &[PathBuf], progress: impl Fn(usize, usize)) -> Vec<Track> {
    let mut files = vec![];
    for r in roots {
        walk(r, &mut files);
    }
    scan_files(files, progress)
}

fn scan_files(files: Vec<PathBuf>, progress: impl Fn(usize, usize)) -> Vec<Track> {
    let total = files.len();
    let mut tracks = Vec::with_capacity(total);
    for (i, p) in files.iter().enumerate() {
        tracks.push(read(p));
        if i % 50 == 0 {
            progress(i, total);
        }
    }
    progress(total, total);
    tracks.sort_by(|a, b| {
        let ka = (a.album_artist.to_lowercase(), a.album.to_lowercase(), a.disc.unwrap_or(1), a.track.unwrap_or(0));
        let kb = (b.album_artist.to_lowercase(), b.album.to_lowercase(), b.disc.unwrap_or(1), b.track.unwrap_or(0));
        ka.cmp(&kb).then_with(|| a.path.cmp(&b.path))
    });
    tracks
}

pub fn is_audio(p: &Path) -> bool {
    // « ._xxx.flac » : fichiers fantômes créés par macOS, pas de la musique.
    if p.file_name().map(|n| n.to_string_lossy().starts_with("._")).unwrap_or(false) {
        return false;
    }
    p.extension().map(|x| EXTENSIONS.contains(&x.to_string_lossy().to_lowercase().as_str())).unwrap_or(false)
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            walk(&p, files);
        } else if is_audio(&p) && !skipped(&p) {
            files.push(p);
        }
    }
}

pub fn read(p: &Path) -> Track {
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let format = p.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default();
    let folder = p
        .parent()
        .and_then(|d| d.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut t = Track {
        path: p.to_string_lossy().into_owned(),
        title: stem,
        artist: String::new(),
        album: folder,
        album_artist: String::new(),
        track: None,
        disc: None,
        duration: 0.0,
        sample_rate: None,
        size: std::fs::metadata(p).map(|m| m.len()).unwrap_or(0),
        added: std::fs::metadata(p)
            .and_then(|m| m.created().or_else(|_| m.modified()))
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0),
        format,
        ..Default::default()
    };
    let Ok(tf) = lofty::read_from_path(p) else { return t };
    let props = tf.properties();
    t.duration = props.duration().as_secs_f64();
    t.sample_rate = props.sample_rate();
    t.bit_depth = props.bit_depth();
    t.channels = props.channels();
    t.bitrate = props.overall_bitrate();
    if let Some(tag) = tf.primary_tag().or_else(|| tf.first_tag()) {
        if let Some(v) = tag.title() {
            t.title = v.into_owned();
        }
        if let Some(v) = tag.artist() {
            t.artist = v.into_owned();
        }
        if let Some(v) = tag.album() {
            t.album = v.into_owned();
        }
        t.track = tag.track();
        t.disc = tag.disk();
        for item in tag.items() {
            let Some(v) = item.value().text() else { continue };
            if *item.key() == ItemKey::AlbumArtist {
                t.album_artist = v.to_string();
            } else if *item.key() == ItemKey::Genre && t.genre.is_empty() {
                t.genre = v.to_string();
            } else if t.year.is_none() {
                // Année : champ « Year » ou date d'enregistrement (« 2018-05-04 »).
                let k = format!("{:?}", item.key());
                if k == "Year" || k == "RecordingDate" || k == "ReleaseDate" || k == "OriginalReleaseDate" {
                    t.year = v.get(0..4).and_then(|y| y.parse().ok());
                }
            }
        }
    }
    if t.album_artist.is_empty() {
        t.album_artist = t.artist.clone();
    }
    t
}

// ---------------------------------------------------------------------------
// Cache de la bibliothèque
// ---------------------------------------------------------------------------

pub fn load_cache(path: &Path) -> Vec<Track> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_cache(path: &Path, tracks: &[Track]) {
    if let Ok(b) = serde_json::to_vec(tracks) {
        // Écriture sûre : fichier temporaire puis remplacement.
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, b).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

/// Le fichier fait-il partie de la bibliothèque (format audio non ignoré) ?
pub fn wanted(p: &Path) -> bool {
    is_audio(p) && !skipped(p)
}

// ---------------------------------------------------------------------------
// Pochettes : image intégrée au fichier, sinon image du dossier
// ---------------------------------------------------------------------------

const COVER_NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart"];

pub fn cover(track: &Path) -> Option<(Vec<u8>, &'static str)> {
    if let Ok(tf) = lofty::read_from_path(track) {
        let mut best: Option<Vec<u8>> = None;
        for tag in tf.tags() {
            for pic in tag.pictures() {
                if pic.pic_type() == PictureType::CoverFront {
                    best = Some(pic.data().to_vec());
                    break;
                }
                if best.is_none() {
                    best = Some(pic.data().to_vec());
                }
            }
        }
        if let Some(b) = best {
            let m = mime(&b);
            return Some((b, m));
        }
    }
    let dir = track.parent()?;
    let mut found: Option<PathBuf> = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
        if !matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp") {
            continue;
        }
        let stem = p.file_stem().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
        if COVER_NAMES.iter().any(|n| stem.starts_with(n)) {
            found = Some(p);
            break;
        }
        if found.is_none() {
            found = Some(p);
        }
    }
    let b = std::fs::read(found?).ok()?;
    let m = mime(&b);
    Some((b, m))
}

fn mime(b: &[u8]) -> &'static str {
    if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/jpeg"
    }
}
