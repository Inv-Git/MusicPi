//! Gestion de la bibliothèque : modifications de tags, pochettes et renommages,
//! toutes journalisées pour pouvoir être annulées.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::library::{self, Track};
use crate::tags;

// ---------------------------------------------------------------------------
// Journal
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct TagChange {
    pub path: String,
    pub before: HashMap<String, String>,
    pub after: HashMap<String, String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CoverChange {
    pub path: String,
    /// Fichier (dans le dossier du journal) contenant l'ancienne pochette ; vide = pas de pochette.
    pub before_blob: String,
    pub after_blob: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Move {
    pub from: String,
    pub to: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Entry {
    pub id: u64,
    /// Secondes depuis 1970.
    pub date: u64,
    pub label: String,
    pub tags: Vec<TagChange>,
    pub covers: Vec<CoverChange>,
    pub moves: Vec<Move>,
    pub undone: bool,
}

/// Résultat renvoyé à l'interface pour mettre la bibliothèque à jour.
#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Changed {
    pub removed: Vec<String>,
    pub tracks: Vec<Track>,
    pub errors: Vec<String>,
    /// Fichiers audio déplacés (ancien chemin, nouveau chemin).
    pub moved: Vec<(String, String)>,
}

#[derive(Clone)]
pub struct Store {
    pub dir: PathBuf,
    pub cache: PathBuf,
}

impl Store {
    fn journal_path(&self) -> PathBuf {
        self.dir.join("journal.json")
    }

    pub fn history(&self) -> Vec<Entry> {
        fs::read(self.journal_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn save_history(&self, h: &[Entry]) {
        let _ = fs::create_dir_all(&self.dir);
        if let Ok(b) = serde_json::to_vec(h) {
            let _ = fs::write(self.journal_path(), b);
        }
    }

    fn push(&self, mut e: Entry) {
        if e.tags.is_empty() && e.covers.is_empty() && e.moves.is_empty() {
            return;
        }
        let mut h = self.history();
        e.id = h.iter().map(|x| x.id).max().unwrap_or(0) + 1;
        e.date = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        h.insert(0, e);
        // On garde les 300 dernières opérations ; les pochettes des plus anciennes sont supprimées.
        while h.len() > 300 {
            if let Some(old) = h.pop() {
                for c in old.covers {
                    for b in [c.before_blob, c.after_blob] {
                        if !b.is_empty() {
                            let _ = fs::remove_file(self.dir.join(b));
                        }
                    }
                }
            }
        }
        self.save_history(&h);
    }

    fn blob(&self, data: &[u8]) -> String {
        let _ = fs::create_dir_all(&self.dir);
        // Nom tiré du contenu : une même pochette n'est stockée qu'une fois.
        let mut h: u64 = 0xcbf29ce484222325;
        for b in data {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        let name = format!("{h:016x}-{}.img", data.len());
        let p = self.dir.join(&name);
        if !p.exists() {
            let _ = fs::write(p, data);
        }
        name
    }

    /// Met à jour le cache de la bibliothèque et prépare la réponse pour l'interface.
    fn finish(&self, removed: Vec<String>, touched: BTreeSet<String>, errors: Vec<String>) -> Changed {
        let tracks: Vec<Track> = touched.iter().filter(|p| Path::new(p).is_file()).map(|p| library::read(Path::new(p))).collect();
        let _lock = library::CACHE_LOCK.lock();
        let mut all = library::load_cache(&self.cache);
        let drop: BTreeSet<&String> = removed.iter().chain(touched.iter()).collect();
        all.retain(|t| !drop.contains(&t.path));
        all.extend(tracks.iter().cloned());
        library::save_cache(&self.cache, &all);
        Changed { removed, tracks, errors, ..Default::default() }
    }

    // -----------------------------------------------------------------------
    // Opérations
    // -----------------------------------------------------------------------

    pub fn edit_tags(&self, label: &str, edits: Vec<(String, HashMap<String, String>)>) -> Changed {
        let mut entry = Entry { label: label.into(), ..Default::default() };
        let mut touched = BTreeSet::new();
        let mut errors = vec![];
        for (path, fields) in edits {
            let p = Path::new(&path);
            let before = match tags::read_fields(p) {
                Ok(b) => b,
                Err(e) => {
                    errors.push(e);
                    continue;
                }
            };
            // On n'écrit que ce qui change vraiment.
            let after: HashMap<String, String> = fields.into_iter().filter(|(k, v)| before.get(k).map(|b| b.trim() != v.trim()).unwrap_or(false)).collect();
            if after.is_empty() {
                continue;
            }
            match tags::write_fields(p, &after) {
                Ok(()) => {
                    let before = after.keys().map(|k| (k.clone(), before.get(k).cloned().unwrap_or_default())).collect();
                    entry.tags.push(TagChange { path: path.clone(), before, after });
                    touched.insert(path);
                }
                Err(e) => errors.push(e),
            }
        }
        self.push(entry);
        self.finish(vec![], touched, errors)
    }

    pub fn set_cover(&self, label: &str, paths: Vec<String>, data: &[u8], folder_file: bool) -> Changed {
        let mut entry = Entry { label: label.into(), ..Default::default() };
        let mut touched = BTreeSet::new();
        let mut errors = vec![];
        let after_blob = self.blob(data);
        let mut dirs = BTreeSet::new();
        for path in paths {
            let p = Path::new(&path);
            let before_blob = tags::read_cover(p).map(|b| self.blob(&b)).unwrap_or_default();
            match tags::write_cover(p, Some(data)) {
                Ok(()) => {
                    entry.covers.push(CoverChange { path: path.clone(), before_blob, after_blob: after_blob.clone() });
                    if let Some(d) = p.parent() {
                        dirs.insert(d.to_path_buf());
                    }
                    touched.insert(path);
                }
                Err(e) => errors.push(e),
            }
        }
        // Copie « cover.jpg » dans le dossier de l'album (l'ancienne est conservée dans le journal).
        if folder_file {
            let ext = if data.starts_with(&[0x89, b'P', b'N', b'G']) { "png" } else { "jpg" };
            for d in dirs {
                let f = d.join(format!("cover.{ext}"));
                let before_blob = fs::read(&f).ok().map(|b| self.blob(&b)).unwrap_or_default();
                match fs::write(&f, data) {
                    Ok(()) => entry.covers.push(CoverChange { path: format!("file:{}", f.display()), before_blob, after_blob: after_blob.clone() }),
                    Err(e) => errors.push(format!("{} : {e}", f.display())),
                }
            }
        }
        self.push(entry);
        self.finish(vec![], touched, errors)
    }

    pub fn move_files(&self, label: &str, moves: Vec<(String, String)>, root: &Path) -> Changed {
        let mut entry = Entry { label: label.into(), ..Default::default() };
        let mut removed = vec![];
        let mut touched = BTreeSet::new();
        let mut errors = vec![];
        let mut emptied: BTreeSet<PathBuf> = BTreeSet::new();
        let mut dest_of_dir: HashMap<PathBuf, PathBuf> = HashMap::new();
        let mut moved = vec![];

        for (from, to) in moves {
            if from == to {
                continue;
            }
            let (f, t) = (PathBuf::from(&from), PathBuf::from(&to));
            if t.exists() && !same_file_ci(&f, &t) {
                errors.push(format!("{} existe déjà", t.display()));
                continue;
            }
            match move_one(&f, &t) {
                Ok(()) => {
                    if let (Some(fd), Some(td)) = (f.parent(), t.parent()) {
                        emptied.insert(fd.to_path_buf());
                        dest_of_dir.entry(fd.to_path_buf()).or_insert_with(|| td.to_path_buf());
                    }
                    entry.moves.push(Move { from: from.clone(), to: to.clone() });
                    moved.push((from.clone(), to.clone()));
                    removed.push(from);
                    touched.insert(to);
                }
                Err(e) => errors.push(format!("{} : {e}", f.display())),
            }
        }

        // Les dossiers vidés de leur musique : on emporte le reste (pochettes, .cue, .log…)
        // vers le nouveau dossier de l'album, puis on supprime les dossiers vides.
        for dir in emptied {
            let Ok(rd) = fs::read_dir(&dir) else { continue };
            let rest: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
            if rest.iter().any(|p| p.is_dir() || library::is_audio(p)) {
                continue;
            }
            if let Some(dest) = dest_of_dir.get(&dir) {
                if dest != &dir {
                    for p in rest {
                        let t = dest.join(p.file_name().unwrap_or_default());
                        if t.exists() {
                            continue;
                        }
                        if move_one(&p, &t).is_ok() {
                            entry.moves.push(Move { from: p.display().to_string(), to: t.display().to_string() });
                        }
                    }
                }
            }
            remove_empty_up(&dir, root);
        }

        self.push(entry);
        let mut ch = self.finish(removed, touched, errors);
        ch.moved = moved;
        ch
    }

    pub fn undo(&self, id: u64, root: Option<&Path>) -> Changed {
        let mut h = self.history();
        let Some(e) = h.iter_mut().find(|e| e.id == id && !e.undone) else {
            return Changed { errors: vec!["Opération introuvable ou déjà annulée".into()], ..Default::default() };
        };
        let mut removed = vec![];
        let mut touched = BTreeSet::new();
        let mut errors = vec![];
        let mut moved = vec![];

        for m in e.moves.iter().rev() {
            let (f, t) = (PathBuf::from(&m.to), PathBuf::from(&m.from));
            match move_one(&f, &t) {
                Ok(()) => {
                    if let Some(d) = f.parent() {
                        if let Some(r) = root {
                            remove_empty_up(d, r);
                        }
                    }
                    if library::is_audio(&t) {
                        removed.push(m.to.clone());
                        touched.insert(m.from.clone());
                        moved.push((m.to.clone(), m.from.clone()));
                    }
                }
                Err(err) => errors.push(format!("{} : {err}", f.display())),
            }
        }
        for c in &e.tags {
            match tags::write_fields(Path::new(&c.path), &c.before) {
                Ok(()) => {
                    touched.insert(c.path.clone());
                }
                Err(err) => errors.push(err),
            }
        }
        for c in &e.covers {
            let data = (!c.before_blob.is_empty()).then(|| fs::read(self.dir.join(&c.before_blob)).ok()).flatten();
            let r = if let Some(f) = c.path.strip_prefix("file:") {
                match &data {
                    Some(d) => fs::write(f, d).map_err(|e| e.to_string()),
                    None => fs::remove_file(f).map_err(|e| e.to_string()),
                }
            } else {
                touched.insert(c.path.clone());
                tags::write_cover(Path::new(&c.path), data.as_deref())
            };
            if let Err(err) = r {
                errors.push(err);
            }
        }
        e.undone = true;
        self.save_history(&h);
        let mut ch = self.finish(removed, touched, errors);
        ch.moved = moved;
        ch
    }
}

fn same_file_ci(a: &Path, b: &Path) -> bool {
    // Windows ignore la casse : renommer « album » en « Album » vise le même fichier.
    a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

fn move_one(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(d) = to.parent() {
        fs::create_dir_all(d)?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    // Autre disque : copie puis suppression.
    fs::copy(from, to)?;
    fs::remove_file(from)
}

/// Supprime `dir` s'il est vide, puis ses parents vides, sans jamais remonter au-dessus de `root`.
fn remove_empty_up(dir: &Path, root: &Path) {
    let mut d = dir.to_path_buf();
    while d.starts_with(root) && d != root {
        if fs::remove_dir(&d).is_err() {
            break;
        }
        match d.parent() {
            Some(p) => d = p.to_path_buf(),
            None => break,
        }
    }
}
