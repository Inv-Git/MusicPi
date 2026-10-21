//! Surveillance des dossiers de musique : les albums ajoutés, déplacés ou supprimés
//! (dans l'Explorateur, par un téléchargement…) apparaissent sans « Actualiser ».
//!
//! Principe : on relève la date de modification de chaque dossier ; un dossier dont
//! la date a changé (ou nouveau) est relu, sans ses sous-dossiers. Peu coûteux.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub type Snapshot = HashMap<PathBuf, SystemTime>;

/// Date de modification de tous les dossiers sous les racines.
pub fn snapshot(roots: &[PathBuf]) -> Snapshot {
    let mut out = HashMap::new();
    let mut stack: Vec<PathBuf> = roots.to_vec();
    while let Some(d) = stack.pop() {
        let Ok(m) = std::fs::metadata(&d) else { continue };
        if !m.is_dir() {
            continue;
        }
        out.insert(d.clone(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH));
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    stack.push(e.path());
                }
            }
        }
    }
    out
}

/// Dossiers à relire (nouveaux ou modifiés depuis `since`) et dossiers disparus.
pub fn diff(old: &Snapshot, new: &Snapshot, since: Option<SystemTime>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut changed: Vec<PathBuf> = new
        .iter()
        .filter(|(d, t)| match old.get(*d) {
            Some(o) => o != *t,
            None => since.map(|s| **t > s).unwrap_or(true),
        })
        .map(|(d, _)| d.clone())
        .collect();
    changed.sort();
    let mut gone: Vec<PathBuf> = old.keys().filter(|d| !new.contains_key(*d)).cloned().collect();
    gone.sort();
    (changed, gone)
}

/// Ce qui change dans la bibliothèque : chemins retirés, fichiers à (re)lire.
#[derive(Debug, Default, PartialEq)]
pub struct Delta {
    pub removed: Vec<String>,
    pub added: Vec<PathBuf>,
}

/// Compare le contenu actuel des dossiers avec les pistes connues.
/// `known` : chemin → taille, `wanted` : filtre des fichiers audio.
pub fn delta(changed: &[PathBuf], gone: &[PathBuf], known: &HashMap<String, u64>, wanted: impl Fn(&Path) -> bool) -> Delta {
    let mut d = Delta::default();
    let mut seen: HashSet<String> = HashSet::new();
    for dir in changed {
        let mut here: HashMap<String, u64> = HashMap::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if e.file_type().map(|t| t.is_file()).unwrap_or(false) && wanted(&p) {
                    here.insert(p.to_string_lossy().into_owned(), e.metadata().map(|m| m.len()).unwrap_or(0));
                }
            }
        }
        for (p, size) in &here {
            match known.get(p) {
                Some(s) if s == size => {}
                _ => d.added.push(PathBuf::from(p)),
            }
            seen.insert(p.clone());
        }
        // Pistes connues de ce dossier qui n'y sont plus.
        for p in known.keys() {
            if Path::new(p).parent() == Some(dir.as_path()) && !here.contains_key(p) {
                d.removed.push(p.clone());
            }
        }
    }
    for dir in gone {
        for p in known.keys() {
            if Path::new(p).starts_with(dir) && !seen.contains(p) {
                d.removed.push(p.clone());
            }
        }
    }
    d.removed.sort();
    d.removed.dedup();
    d.added.sort();
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn audio(p: &Path) -> bool {
        p.extension().map(|e| e == "flac").unwrap_or(false)
    }

    #[test]
    fn detection() {
        let root = std::env::temp_dir().join(format!("watch-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let a = root.join("Artiste").join("Album");
        fs::create_dir_all(&a).unwrap();
        fs::write(a.join("01.flac"), b"aaaa").unwrap();
        fs::write(a.join("cover.jpg"), b"x").unwrap();
        let roots = vec![root.clone()];
        let s1 = snapshot(&roots);
        assert_eq!(s1.len(), 3);

        // Rien de connu : tout est nouveau.
        let (ch, gone) = diff(&HashMap::new(), &s1, None);
        let d = delta(&ch, &gone, &HashMap::new(), audio);
        assert_eq!(d.added, vec![a.join("01.flac")]);

        let mut known: HashMap<String, u64> = HashMap::new();
        known.insert(a.join("01.flac").to_string_lossy().into_owned(), 4);

        // Un album ajouté, un titre supprimé.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let b = root.join("Artiste").join("Autre");
        fs::create_dir_all(&b).unwrap();
        fs::write(b.join("01.flac"), b"bb").unwrap();
        fs::remove_file(a.join("01.flac")).unwrap();
        fs::write(a.join("02.flac"), b"cc").unwrap();
        let s2 = snapshot(&roots);
        let (ch, gone) = diff(&s1, &s2, None);
        assert!(ch.contains(&b) && ch.contains(&a), "{ch:?}");
        let d = delta(&ch, &gone, &known, audio);
        assert_eq!(d.removed, vec![a.join("01.flac").to_string_lossy().into_owned()]);
        assert!(d.added.contains(&b.join("01.flac")) && d.added.contains(&a.join("02.flac")));

        // Dossier supprimé en entier.
        let mut known2: HashMap<String, u64> = HashMap::new();
        known2.insert(b.join("01.flac").to_string_lossy().into_owned(), 2);
        fs::remove_dir_all(&b).unwrap();
        let s3 = snapshot(&roots);
        let (ch, gone) = diff(&s2, &s3, None);
        assert_eq!(gone, vec![b.clone()]);
        let d = delta(&ch, &gone, &known2, audio);
        assert_eq!(d.removed, vec![b.join("01.flac").to_string_lossy().into_owned()]);

        // Rien n'a bougé : rien à faire.
        let (ch, gone) = diff(&s3, &snapshot(&roots), None);
        assert!(ch.is_empty() && gone.is_empty());
        let _ = fs::remove_dir_all(&root);
    }
}
