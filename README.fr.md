<p align="center">
  <img src="src-tauri/icons/128x128@2x.png" width="128" alt="Logo MusicPi">
</p>

<h1 align="center">MusicPi</h1>

<p align="center">
  Un lecteur FLAC pour Windows, bit-perfect et pensé hors ligne, avec une app Android compagnon.<br>
  <a href="README.md">English version</a>
</p>

---

## Pourquoi MusicPi

MusicPi est l'héritier de [MusicBee](https://getmusicbee.com/), le lecteur de musique Windows que beaucoup utilisent depuis des années. Son nom est un clin d'œil à ce projet. MusicPi en garde l'esprit (tes fichiers, tes dossiers, pas de compte de streaming) et repart sur une base moderne : une interface légère, un moteur audio bit-perfect et une app téléphone synchronisée.

## Fonctions

**Lecture**
- FLAC (et MP3, AAC/M4A, OGG, WAV, AIFF), sans blanc dans un album
- Sortie WASAPI partagée ou exclusive (bit-perfect), réglage de la fréquence et du tampon
- Fondu enchaîné, fondu à la pause, ReplayGain (par titre ou par album), volume maximum, mono, balance
- Égaliseur 10 bandes avec préréglages, un réglage par sortie, import AutoEQ
- Mode travail : ralentir sans changer la hauteur, boucle A/B
- Aléatoire qui évite d'enchaîner le même artiste, répétition, mode radio en fin de file
- Reprise là où tu t'étais arrêté

**Bibliothèque**
- Plusieurs dossiers de musique, détection automatique des albums ajoutés, déplacés ou supprimés
- Recherche rapide et classée, compilations, classement sans les articles
- Éditeur de tags, recherche de pochettes, correction avec MusicBrainz / Discogs, renommage des fichiers selon un modèle, historique annulable
- Cœurs sur les titres et les albums, playlists, listes automatiques (les plus écoutés, ajoutés récemment…)

**Interface**
- Thème clair, sombre ou comme Windows, couleur d'accent de Windows
- Mode ambiance : plein écran avec la pochette et ses couleurs, qui se lance tout seul quand tu t'éloignes
- Contrôles multimédia de Windows (écran verrouillé, panneau du volume), zone de notification, lancement au démarrage
- Raccourcis clavier modifiables, français et anglais

**App Android**
- Parcourir et écouter ta bibliothèque depuis le téléphone, chez toi ou dehors (via Tailscale)
- Télécommande du PC, avec les boutons de volume du téléphone
- Albums et playlists téléchargés pour l'écoute hors ligne (fichiers FLAC d'origine)
- Flux Opus allégé en données mobiles
- Cœurs et playlists synchronisés avec le PC, appairage une seule fois avec un code à 6 chiffres

## Compiler

### Prérequis

- Windows 10 ou 11 (WebView2 y est déjà)
- [Rust](https://rustup.rs/) (stable)
- [Node.js](https://nodejs.org/) 18 ou plus récent
- Les bibliothèques audio BASS (voir plus bas)

### Bibliothèques BASS

Le moteur audio de MusicPi utilise [BASS](https://www.un4seen.com/) d'un4seen. Sa licence ne permet pas de la redistribuer ici : télécharge toi-même les versions **Windows x64** sur un4seen.com et copie les DLL (dossier `x64` de chaque zip) dans `src-tauri/lib/` :

| Fichier | Paquet | Indispensable |
|---|---|---|
| `bass.dll` | BASS | oui |
| `bassmix.dll` | BASSmix | oui |
| `basswasapi.dll` | BASSWASAPI | oui |
| `bassflac.dll` | BASSFLAC | oui |
| `bass_fx.dll` | BASS_FX | pour le mode travail (vitesse) |
| `bassenc.dll` | BASSenc | pour le flux Opus allégé |
| `bassenc_opus.dll` | BASSenc_OPUS | pour le flux Opus allégé |

BASS est gratuit pour un usage non commercial. Tout autre usage demande une licence auprès d'un4seen.

### Lancer et compiler

```
npm install
npm run dev
```

Pour fabriquer l'installateur :

```
npm run build
```

Il se trouve ensuite dans `src-tauri/target/release/bundle/nsis/`.

### App Android

Ouvre le dossier `android` dans [Android Studio](https://developer.android.com/studio), laisse la synchronisation se faire, puis **Run** sur ton téléphone. Sur le PC, active le partage dans **Paramètres › Téléphone** et recopie l'adresse et le code affichés.

## Crédits

- [MusicBee](https://getmusicbee.com/), l'inspiration et l'origine du nom
- [BASS](https://www.un4seen.com/) d'un4seen Developments : moteur audio
- [Tauri](https://tauri.app/) : application de bureau
- [Lucide](https://lucide.dev/) : icônes (licence ISC)
- [Manrope](https://github.com/sharanda/manrope) : police (SIL Open Font License)
- Crates Rust : lofty, ureq, image, souvlaki, serde
- Android : Jetpack Compose, Media3 / ExoPlayer, OkHttp, Coil, kotlinx.serialization
- Données en ligne : [MusicBrainz](https://musicbrainz.org/), [Cover Art Archive](https://coverartarchive.org/), [Discogs](https://www.discogs.com/), Deezer, iTunes, [Last.fm](https://www.last.fm/)

## Licence

Le code source de MusicPi est sous [licence MIT](LICENSE). BASS et les autres bibliothèques gardent leurs propres licences.
