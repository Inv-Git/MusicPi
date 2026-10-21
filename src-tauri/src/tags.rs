//! Lecture et écriture des tags (titre, artiste, album…) et de la pochette intégrée.

use std::collections::HashMap;
use std::path::Path;

use lofty::config::WriteOptions;
use lofty::file::TaggedFile;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;
use lofty::tag::{ItemKey, Tag};

/// Champs modifiables, avec leurs noms côté interface.
pub const FIELDS: [&str; 8] = ["title", "artist", "album", "albumArtist", "year", "genre", "track", "disc"];

fn key_of(field: &str) -> Option<ItemKey> {
    Some(match field {
        "title" => ItemKey::TrackTitle,
        "artist" => ItemKey::TrackArtist,
        "album" => ItemKey::AlbumTitle,
        "albumArtist" => ItemKey::AlbumArtist,
        "year" => ItemKey::RecordingDate,
        "genre" => ItemKey::Genre,
        "track" => ItemKey::TrackNumber,
        "disc" => ItemKey::DiscNumber,
        _ => return None,
    })
}

fn open(p: &Path) -> Result<TaggedFile, String> {
    lofty::read_from_path(p).map_err(|e| format!("{} : {e}", p.display()))
}

fn tag_mut(tf: &mut TaggedFile) -> &mut Tag {
    if tf.primary_tag().is_none() {
        let tt = tf.primary_tag_type();
        tf.insert_tag(Tag::new(tt));
    }
    tf.primary_tag_mut().expect("tag")
}

/// Écriture sûre : on modifie une copie, on vérifie qu'elle se relit correctement,
/// puis on remplace l'original d'un coup. Si quoi que ce soit échoue (ou si l'app est
/// fermée en plein milieu), l'original reste intact.
fn save(tf: &TaggedFile, p: &Path) -> Result<(), String> {
    let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let tmp = p.with_file_name(format!(".{name}.music-tmp"));
    let fail = |e: String| {
        let _ = std::fs::remove_file(&tmp);
        format!("{name} : {e}")
    };
    let orig_len = std::fs::metadata(p).map(|m| m.len()).map_err(|e| fail(e.to_string()))?;
    std::fs::copy(p, &tmp).map_err(|e| fail(e.to_string()))?;
    tf.save_to_path(&tmp, WriteOptions::default()).map_err(|e| fail(e.to_string()))?;
    // Contrôles : le fichier modifié se relit, et n'a pas perdu son contenu audio.
    let new_len = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
    if new_len < orig_len / 2 {
        return Err(fail("fichier modifié anormalement petit, modification annulée".into()));
    }
    lofty::read_from_path(&tmp).map_err(|e| fail(format!("relecture impossible ({e})")))?;
    std::fs::rename(&tmp, p).map_err(|e| fail(format!("{e} (fichier en cours de lecture ?)")))
}

/// Valeurs actuelles des champs modifiables.
pub fn read_fields(p: &Path) -> Result<HashMap<String, String>, String> {
    let tf = open(p)?;
    let mut out: HashMap<String, String> = FIELDS.iter().map(|f| (f.to_string(), String::new())).collect();
    if let Some(tag) = tf.primary_tag().or_else(|| tf.first_tag()) {
        for item in tag.items() {
            let Some(v) = item.value().text() else { continue };
            for f in FIELDS {
                if key_of(f).as_ref() == Some(item.key()) && out[f].is_empty() {
                    out.insert(f.to_string(), v.to_string());
                }
            }
            // Année rangée dans un autre champ (TYER, DATE…).
            let k = format!("{:?}", item.key());
            if out["year"].is_empty() && (k == "Year" || k == "ReleaseDate" || k == "OriginalReleaseDate") {
                out.insert("year".into(), v.to_string());
            }
        }
    }
    Ok(out)
}

/// Écrit les champs donnés ; une valeur vide efface le champ.
pub fn write_fields(p: &Path, fields: &HashMap<String, String>) -> Result<(), String> {
    let mut tf = open(p)?;
    let tag = tag_mut(&mut tf);
    for (f, v) in fields {
        let Some(key) = key_of(f) else { continue };
        let v = v.trim();
        if v.is_empty() {
            tag.remove_key(&key);
        } else {
            tag.insert_text(key, v.to_string());
        }
    }
    save(&tf, p)
}

/// Pochette « face avant » intégrée au fichier.
pub fn read_cover(p: &Path) -> Option<Vec<u8>> {
    let tf = open(p).ok()?;
    let mut any = None;
    for tag in tf.tags() {
        for pic in tag.pictures() {
            if pic.pic_type() == PictureType::CoverFront {
                return Some(pic.data().to_vec());
            }
            any.get_or_insert_with(|| pic.data().to_vec());
        }
    }
    any
}

/// Remplace (ou retire, avec `None`) la pochette intégrée.
pub fn write_cover(p: &Path, data: Option<&[u8]>) -> Result<(), String> {
    let mut tf = open(p)?;
    let tag = tag_mut(&mut tf);
    tag.remove_picture_type(PictureType::CoverFront);
    if let Some(d) = data {
        let mime = if d.starts_with(&[0x89, b'P', b'N', b'G']) { MimeType::Png } else { MimeType::Jpeg };
        tag.push_picture(Picture::new_unchecked(PictureType::CoverFront, Some(mime), None, d.to_vec()));
    }
    save(&tf, p)
}

/// Valeurs ReplayGain d'un fichier (gain en dB, crête en linéaire).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReplayGain {
    pub track_gain: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_gain: Option<f32>,
    pub album_peak: Option<f32>,
}

/// « -7.89 dB » → -7.89
fn rg_number(s: &str) -> Option<f32> {
    let n: String = s.trim().chars().take_while(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | ',')).collect();
    n.replace(',', ".").parse().ok()
}

pub fn replaygain(p: &Path) -> ReplayGain {
    let mut rg = ReplayGain::default();
    let Ok(tf) = lofty::read_from_path(p) else { return rg };
    for tag in tf.tags() {
        for item in tag.items() {
            let Some(v) = item.value().text().and_then(rg_number) else { continue };
            match item.key() {
                ItemKey::ReplayGainTrackGain => rg.track_gain = rg.track_gain.or(Some(v)),
                ItemKey::ReplayGainTrackPeak => rg.track_peak = rg.track_peak.or(Some(v)),
                ItemKey::ReplayGainAlbumGain => rg.album_gain = rg.album_gain.or(Some(v)),
                ItemKey::ReplayGainAlbumPeak => rg.album_peak = rg.album_peak.or(Some(v)),
                _ => {}
            }
        }
    }
    rg
}

impl ReplayGain {
    /// Facteur de volume à appliquer : `mode` = « track » ou « album » ; `preamp` en dB ;
    /// `clip` = ne jamais dépasser la crête (pas de saturation).
    pub fn factor(&self, mode: &str, preamp: f32, clip: bool) -> Option<f32> {
        let (g, p) = if mode == "album" {
            (self.album_gain.or(self.track_gain), self.album_peak.or(self.track_peak))
        } else {
            (self.track_gain.or(self.album_gain), self.track_peak.or(self.album_peak))
        };
        let g = g?;
        let mut f = 10f32.powf((g + preamp) / 20.0);
        if clip {
            if let Some(p) = p.filter(|p| *p > 0.0) {
                f = f.min(1.0 / p);
            }
        }
        Some(f)
    }
}

#[cfg(test)]
mod rg_tests {
    use super::*;
    #[test]
    fn calcul() {
        assert_eq!(rg_number("-7.89 dB"), Some(-7.89));
        assert_eq!(rg_number("+2,5 dB"), Some(2.5));
        assert_eq!(rg_number("0.987654"), Some(0.987654));
        let rg = ReplayGain { track_gain: Some(-6.0), track_peak: Some(0.9), album_gain: Some(3.0), album_peak: Some(0.95) };
        assert!((rg.factor("track", 0.0, true).unwrap() - 0.501187).abs() < 1e-4);
        // +3 dB = 1,41 mais crête 0,95 → plafonné à 1/0,95.
        assert!((rg.factor("album", 0.0, true).unwrap() - 1.0 / 0.95).abs() < 1e-4);
        assert!((rg.factor("album", 0.0, false).unwrap() - 1.41254).abs() < 1e-3);
        assert_eq!(ReplayGain::default().factor("track", 0.0, true), None);
    }
}
