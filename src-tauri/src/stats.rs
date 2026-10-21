//! Écoutes : nombre par titre, dernière écoute, historique.
//! Un titre compte quand on en a écouté la moitié (ou 4 minutes), comme Last.fm.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

const HISTORY_MAX: usize = 20_000;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Stats {
    /// Chemin → nombre d'écoutes.
    pub plays: HashMap<String, u32>,
    /// Chemin → dernière écoute (secondes depuis 1970).
    pub last: HashMap<String, u64>,
    /// (chemin, date), du plus ancien au plus récent.
    pub history: Vec<(String, u64)>,
}

impl Stats {
    pub fn record(&mut self, path: &str, at: u64) {
        *self.plays.entry(path.to_string()).or_insert(0) += 1;
        self.last.insert(path.to_string(), at);
        self.history.push((path.to_string(), at));
        if self.history.len() > HISTORY_MAX {
            let cut = self.history.len() - HISTORY_MAX;
            self.history.drain(..cut);
        }
    }

    /// Fichiers renommés ou déplacés : les compteurs suivent.
    pub fn remap(&mut self, moves: &[(String, String)]) -> bool {
        let mut changed = false;
        for (a, b) in moves {
            if let Some(n) = self.plays.remove(a) {
                *self.plays.entry(b.clone()).or_insert(0) += n;
                changed = true;
            }
            if let Some(t) = self.last.remove(a) {
                self.last.insert(b.clone(), t);
            }
        }
        if changed {
            let m: HashMap<&str, &str> = moves.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            for (p, _) in self.history.iter_mut() {
                if let Some(n) = m.get(p.as_str()) {
                    *p = n.to_string();
                }
            }
        }
        changed
    }

    /// Fusion d'une sauvegarde : on additionne et on garde les dates les plus récentes.
    pub fn merge(&mut self, other: Stats) {
        for (p, n) in other.plays {
            *self.plays.entry(p).or_insert(0) += n;
        }
        for (p, t) in other.last {
            let e = self.last.entry(p).or_insert(0);
            *e = (*e).max(t);
        }
        self.history.extend(other.history);
        self.history.sort_by_key(|(_, t)| *t);
        self.history.dedup();
    }
}

pub struct Store {
    path: PathBuf,
    data: Mutex<Stats>,
}

impl Store {
    pub fn load(path: PathBuf) -> Store {
        let data = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Store { path, data: Mutex::new(data) }
    }

    fn save(&self, s: &Stats) {
        if let Ok(b) = serde_json::to_vec(s) {
            let tmp = self.path.with_extension("json.tmp");
            if std::fs::write(&tmp, b).is_ok() {
                let _ = std::fs::rename(&tmp, &self.path);
            }
        }
    }

    pub fn get(&self) -> Stats {
        self.data.lock().unwrap().clone()
    }

    pub fn record(&self, path: &str, at: u64) -> u32 {
        let (s, n) = {
            let mut d = self.data.lock().unwrap();
            d.record(path, at);
            let n = d.plays.get(path).copied().unwrap_or(0);
            (d.clone(), n)
        };
        self.save(&s);
        n
    }

    pub fn remap(&self, moves: &[(String, String)]) {
        let s = {
            let mut d = self.data.lock().unwrap();
            if !d.remap(moves) {
                return;
            }
            d.clone()
        };
        self.save(&s);
    }

    pub fn merge(&self, other: Stats) {
        let s = {
            let mut d = self.data.lock().unwrap();
            d.merge(other);
            d.clone()
        };
        self.save(&s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecoutes() {
        let mut s = Stats::default();
        s.record("a", 10);
        s.record("a", 20);
        s.record("b", 30);
        assert_eq!(s.plays["a"], 2);
        assert_eq!(s.last["a"], 20);
        assert_eq!(s.history.len(), 3);
        assert!(s.remap(&[("a".into(), "c".into())]));
        assert_eq!(s.plays.get("a"), None);
        assert_eq!(s.plays["c"], 2);
        assert_eq!(s.history[0].0, "c");
        let mut o = Stats::default();
        o.record("c", 5);
        s.merge(o);
        assert_eq!(s.plays["c"], 3);
        assert_eq!(s.last["c"], 20);
        assert_eq!(s.history[0], ("c".to_string(), 5));
    }
}
