use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::engine::{AudioOpts, OutputPref};
use crate::eq::EqParams;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub library_folder: Option<String>,
    pub output: OutputPref,
    pub volume: f32,
    pub eq: EqParams,
    /// Raccourcis clavier personnalisés : action → combinaison (« Ctrl+ArrowRight »).
    pub shortcuts: std::collections::HashMap<String, String>,
    /// Apparence : « light », « dark » ou « system ».
    pub theme: String,
    /// Couleur d'accent : « coral » ou « windows ».
    pub accent: String,
    /// Mode ambiance lancé tout seul après `ambient_delay` secondes sans toucher l'ordinateur.
    pub ambient_auto: bool,
    pub ambient_delay: u32,
    /// Lecture et sortie : ReplayGain, fondus, limiteur, tampon…
    pub audio: AudioOpts,
    /// Reprendre la file et la position au démarrage.
    pub resume: bool,
    /// Lecture aléatoire activée.
    pub shuffle: bool,
    /// Dossiers de musique supplémentaires.
    pub extra_folders: Vec<String>,
    /// Surveiller les dossiers (nouveaux albums détectés tout seuls).
    pub watch: bool,
    /// Formats ignorés (« mp3 », « wav »…).
    pub ignore_ext: Vec<String>,
    /// Préférences propres à l'interface (affichage, EQ perso…), gérées par elle.
    pub ui: serde_json::Value,
    /// Fermer la fenêtre la range dans la zone de notification (la musique continue).
    pub tray_close: bool,
    /// Compte Last.fm (clé d'API perso, et session une fois connecté).
    pub lastfm: LastFm,
    /// Jeton personnel Discogs (recherches de pochettes et de fiches).
    pub discogs_token: String,
    /// Modèle de rangement des fichiers.
    pub rename_pattern: String,
    /// Enregistrer aussi la pochette en « cover.jpg » dans le dossier de l'album.
    pub cover_file: bool,
    /// App Android : serveur, appareils appairés.
    pub phone: crate::phone::PhoneSettings,
    /// Paroles : recherche en ligne, enregistrement en .lrc.
    pub lyrics: LyricsPrefs,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LyricsPrefs {
    /// Chercher sur lrclib.net quand le morceau n'a pas de paroles.
    pub online: bool,
    /// Enregistrer les paroles trouvées en ligne dans un .lrc à côté du morceau.
    pub save_lrc: bool,
}

impl Default for LyricsPrefs {
    fn default() -> Self {
        Self { online: true, save_lrc: false }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self { library_folder: None, output: OutputPref::default(), volume: 1.0, eq: EqParams::default(), shortcuts: Default::default(), theme: "light".into(), accent: "coral".into(), ambient_auto: false, ambient_delay: 300, audio: AudioOpts::default(), resume: true, shuffle: false, extra_folders: vec![], watch: true, ignore_ext: vec![], ui: serde_json::json!({}), tray_close: false, lastfm: LastFm::default(), discogs_token: String::new(), rename_pattern: "{albumartist}/{year} - {album}/{disc}{track} - {title}".into(), cover_file: true, phone: Default::default(), lyrics: LyricsPrefs::default() }
    }
}

impl Settings {
    /// Toutes les racines de la bibliothèque (dossier principal d'abord).
    pub fn roots(&self) -> Vec<std::path::PathBuf> {
        self.library_folder.iter().chain(self.extra_folders.iter()).filter(|r| !r.is_empty()).map(std::path::PathBuf::from).collect()
    }

    /// Racine qui contient ce fichier.
    pub fn root_of(&self, p: &Path) -> Option<std::path::PathBuf> {
        self.roots().into_iter().find(|r| p.starts_with(r))
    }

    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, s);
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LastFm {
    pub api_key: String,
    pub api_secret: String,
    pub user: String,
    pub session: String,
    /// Scrobbler les écoutes (sinon : seulement « en cours d'écoute »… ou rien si pas de session).
    pub scrobble: bool,
}
