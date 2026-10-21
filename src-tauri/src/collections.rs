//! Titres et albums aimés (le petit cœur) et playlists, partagés avec l'app téléphone.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    /// Chemins des fichiers, dans l'ordre de lecture.
    pub tracks: Vec<String>,
    pub created: u64,
    pub updated: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Collections {
    /// Augmente à chaque modification.
    pub version: u64,
    /// Titres aimés, du plus récent au plus ancien.
    pub liked_tracks: Vec<String>,
    /// Albums aimés : clé « artiste|album » (sans casse, accents ni ponctuation).
    pub liked_albums: Vec<String>,
    pub playlists: Vec<Playlist>,
}

/// Modification demandée par l'interface du PC ou par le téléphone.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Op {
    LikeTrack { path: String, on: bool },
    LikeAlbum { key: String, on: bool },
    Create {
        id: Option<String>,
        name: String,
        #[serde(default)]
        paths: Vec<String>,
    },
    Rename { id: String, name: String },
    Delete { id: String },
    Add { id: String, paths: Vec<String> },
    Remove { id: String, paths: Vec<String> },
    Move { id: String, from: usize, to: usize },
    SetTracks { id: String, paths: Vec<String> },
}

fn set_flag(list: &mut Vec<String>, item: String, on: bool) -> bool {
    let has = list.contains(&item);
    if on && !has {
        list.insert(0, item);
        true
    } else if !on && has {
        list.retain(|x| x != &item);
        true
    } else {
        false
    }
}

impl Collections {
    fn playlist(&mut self, id: &str) -> Result<&mut Playlist, String> {
        self.playlists.iter_mut().find(|p| p.id == id).ok_or_else(|| "Playlist introuvable (supprimée ?)".to_string())
    }

    /// Applique une modification ; renvoie l'identifiant de la playlist concernée.
    pub fn apply(&mut self, op: Op, now: u64, new_id: impl FnOnce() -> String) -> Result<Option<String>, String> {
        let mut out = None;
        let changed = match op {
            Op::LikeTrack { path, on } => set_flag(&mut self.liked_tracks, path, on),
            Op::LikeAlbum { key, on } => set_flag(&mut self.liked_albums, key, on),
            Op::Create { id, name, paths } => {
                let id = id.filter(|s| !s.is_empty()).unwrap_or_else(new_id);
                if self.playlists.iter().any(|p| p.id == id) {
                    // Déjà créée (requête renvoyée par le téléphone après une coupure).
                    out = Some(id);
                    false
                } else {
                    let name = name.trim();
                    let name = if name.is_empty() { "Nouvelle playlist".to_string() } else { name.to_string() };
                    self.playlists.insert(0, Playlist { id: id.clone(), name, tracks: paths, created: now, updated: now });
                    out = Some(id);
                    true
                }
            }
            Op::Rename { id, name } => {
                let name = name.trim().to_string();
                if name.is_empty() {
                    return Err("Nom vide".into());
                }
                let p = self.playlist(&id)?;
                p.name = name;
                p.updated = now;
                out = Some(id);
                true
            }
            Op::Delete { id } => {
                let n = self.playlists.len();
                self.playlists.retain(|p| p.id != id);
                n != self.playlists.len()
            }
            Op::Add { id, paths } => {
                let p = self.playlist(&id)?;
                p.tracks.extend(paths);
                p.updated = now;
                out = Some(id);
                true
            }
            Op::Remove { id, paths } => {
                let p = self.playlist(&id)?;
                let gone: HashSet<&String> = paths.iter().collect();
                p.tracks.retain(|t| !gone.contains(t));
                p.updated = now;
                out = Some(id);
                true
            }
            Op::Move { id, from, to } => {
                let p = self.playlist(&id)?;
                if from >= p.tracks.len() {
                    return Err("Position invalide".into());
                }
                let t = p.tracks.remove(from);
                p.tracks.insert(to.min(p.tracks.len()), t);
                p.updated = now;
                out = Some(id);
                true
            }
            Op::SetTracks { id, paths } => {
                let p = self.playlist(&id)?;
                p.tracks = paths;
                p.updated = now;
                out = Some(id);
                true
            }
        };
        if changed {
            self.version += 1;
        }
        Ok(out)
    }

    /// Fichiers renommés ou déplacés : on suit les nouveaux chemins.
    pub fn remap(&mut self, moves: &[(String, String)]) -> bool {
        if moves.is_empty() {
            return false;
        }
        let m: HashMap<&str, &str> = moves.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let mut changed = false;
        let mut fix = |v: &mut Vec<String>| {
            for p in v.iter_mut() {
                if let Some(n) = m.get(p.as_str()) {
                    *p = n.to_string();
                    changed = true;
                }
            }
        };
        fix(&mut self.liked_tracks);
        for p in &mut self.playlists {
            fix(&mut p.tracks);
        }
        if changed {
            self.version += 1;
        }
        changed
    }
}

// ---------------------------------------------------------------------------------------
// Enregistrement sur le disque
// ---------------------------------------------------------------------------------------

pub type Notify = Box<dyn Fn(&Collections) + Send + Sync>;

pub struct Store {
    path: PathBuf,
    data: Mutex<Collections>,
    notify: Mutex<Option<Notify>>,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Store {
    pub fn load(path: PathBuf) -> Store {
        let data = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Store { path, data: Mutex::new(data), notify: Mutex::new(None) }
    }

    /// Appelé après chaque modification (interface du PC et téléphone).
    pub fn on_change(&self, f: Notify) {
        *self.notify.lock().unwrap() = Some(f);
    }

    pub fn get(&self) -> Collections {
        self.data.lock().unwrap().clone()
    }

    fn save_and_notify(&self, c: &Collections) {
        if let Ok(b) = serde_json::to_vec_pretty(c) {
            // Écriture sûre : fichier temporaire puis remplacement.
            let tmp = self.path.with_extension("json.tmp");
            if std::fs::write(&tmp, b).is_ok() {
                let _ = std::fs::rename(&tmp, &self.path);
            }
        }
        if let Some(f) = self.notify.lock().unwrap().as_ref() {
            f(c);
        }
    }

    pub fn apply(&self, op: Op) -> Result<(Collections, Option<String>), String> {
        let (c, id, changed) = {
            let mut d = self.data.lock().unwrap();
            let v = d.version;
            let id = d.apply(op, now(), || crate::remote::random_hex(8))?;
            (d.clone(), id, d.version != v)
        };
        if changed {
            self.save_and_notify(&c);
        }
        Ok((c, id))
    }

    /// Restauration d'une sauvegarde : tout est remplacé.
    pub fn replace(&self, mut c: Collections) {
        let c = {
            let mut d = self.data.lock().unwrap();
            c.version = d.version.max(c.version) + 1;
            *d = c;
            d.clone()
        };
        self.save_and_notify(&c);
    }

    pub fn remap(&self, moves: &[(String, String)]) {
        let c = {
            let mut d = self.data.lock().unwrap();
            if !d.remap(moves) {
                return;
            }
            d.clone()
        };
        self.save_and_notify(&c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> impl FnMut() -> String {
        let mut n = 0;
        move || {
            n += 1;
            format!("p{n}")
        }
    }

    #[test]
    fn coeurs() {
        let mut c = Collections::default();
        c.apply(Op::LikeTrack { path: "a".into(), on: true }, 1, || "x".into()).unwrap();
        c.apply(Op::LikeTrack { path: "b".into(), on: true }, 1, || "x".into()).unwrap();
        c.apply(Op::LikeTrack { path: "a".into(), on: true }, 1, || "x".into()).unwrap();
        assert_eq!(c.liked_tracks, vec!["b", "a"]);
        assert_eq!(c.version, 2);
        c.apply(Op::LikeTrack { path: "a".into(), on: false }, 1, || "x".into()).unwrap();
        assert_eq!(c.liked_tracks, vec!["b"]);
        c.apply(Op::LikeAlbum { key: "k".into(), on: true }, 1, || "x".into()).unwrap();
        assert_eq!(c.liked_albums, vec!["k"]);
    }

    #[test]
    fn playlists() {
        let mut c = Collections::default();
        let mut g = ids();
        let id = c.apply(Op::Create { id: None, name: "  ".into(), paths: vec!["a".into()] }, 5, &mut g).unwrap().unwrap();
        assert_eq!(c.playlists[0].name, "Nouvelle playlist");
        c.apply(Op::Add { id: id.clone(), paths: vec!["b".into(), "c".into()] }, 6, &mut g).unwrap();
        c.apply(Op::Move { id: id.clone(), from: 2, to: 0 }, 6, &mut g).unwrap();
        assert_eq!(c.playlists[0].tracks, vec!["c", "a", "b"]);
        c.apply(Op::Remove { id: id.clone(), paths: vec!["a".into()] }, 6, &mut g).unwrap();
        assert_eq!(c.playlists[0].tracks, vec!["c", "b"]);
        c.apply(Op::Rename { id: id.clone(), name: "Basse".into() }, 7, &mut g).unwrap();
        assert_eq!(c.playlists[0].name, "Basse");
        assert!(c.apply(Op::Rename { id: id.clone(), name: " ".into() }, 7, &mut g).is_err());
        // Création renvoyée deux fois par le téléphone : une seule playlist.
        c.apply(Op::Create { id: Some("tel1".into()), name: "T".into(), paths: vec![] }, 8, &mut g).unwrap();
        c.apply(Op::Create { id: Some("tel1".into()), name: "T".into(), paths: vec![] }, 8, &mut g).unwrap();
        assert_eq!(c.playlists.len(), 2);
        c.apply(Op::Delete { id: id.clone() }, 9, &mut g).unwrap();
        assert_eq!(c.playlists.len(), 1);
        assert!(c.apply(Op::Add { id, paths: vec![] }, 9, &mut g).is_err());
    }

    #[test]
    fn renommages() {
        let mut c = Collections::default();
        c.liked_tracks = vec!["old".into(), "x".into()];
        c.playlists.push(Playlist { id: "p".into(), tracks: vec!["x".into(), "old".into()], ..Default::default() });
        assert!(c.remap(&[("old".into(), "new".into())]));
        assert_eq!(c.liked_tracks, vec!["new", "x"]);
        assert_eq!(c.playlists[0].tracks, vec!["x", "new"]);
        assert!(!c.remap(&[("zzz".into(), "y".into())]));
    }
}
