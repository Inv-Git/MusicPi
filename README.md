<p align="center">
  <img src="src-tauri/icons/128x128@2x.png" width="128" alt="MusicPi logo">
</p>

<h1 align="center">MusicPi</h1>

<p align="center">
  A FLAC player for Windows, bit-perfect and offline first, with an Android companion app.<br>
  <a href="README.fr.md">Version française</a>
</p>

---

## Why MusicPi

MusicPi is the heir of [MusicBee](https://getmusicbee.com/), the Windows music player that many of us have used for years. Its name is a nod to that project. MusicPi keeps the spirit (your own files, your own folders, no streaming account) and starts again on a modern base: a light interface, a bit-perfect audio engine, and a phone app that stays in sync.

## Features

**Playback**
- FLAC (plus MP3, AAC/M4A, OGG, WAV, AIFF), gapless within an album
- WASAPI shared or exclusive (bit-perfect) output, sample rate and buffer settings
- Crossfade, fade on pause, ReplayGain (per track or per album), volume limit, mono, balance
- 10-band equalizer with presets, one setting per output device, AutoEQ import
- Practice mode: slow down without changing pitch, A/B loop
- Shuffle that avoids the same artist twice in a row, repeat, radio mode at the end of the queue
- Resume where you left off
- Synced lyrics: `.lrc` files, lyrics tags, or [LRCLIB](https://lrclib.net) online, with click-to-seek and per-track offset

**Library**
- Several music folders, automatic detection of added, moved or deleted albums
- Fast ranked search, compilations, "ignore articles" sorting
- Tag editor, cover search, fixes from MusicBrainz / Discogs, file renaming with a pattern, undo history
- Hearts on tracks and albums, playlists, automatic lists (most played, recently added…)

**Interface**
- Light, dark or system theme, Windows accent color
- Ambient mode: full screen with the cover and its colors, starts by itself when you're away
- Windows media controls (lock screen, volume flyout), notification area, launch at startup
- Keyboard shortcuts you can change, English and French

**Android app**
- Browse and play your library from the phone, at home or away (via Tailscale)
- Remote control of the PC, with the phone's volume buttons
- Albums and playlists downloaded for offline listening (original FLAC files), with their lyrics
- Lighter Opus stream on mobile data
- Hearts and playlists synced with the PC, pairing once with a 6-digit code

## Building

### Requirements

- Windows 10 or 11 (WebView2 is already there)
- [Rust](https://rustup.rs/) (stable)
- [Node.js](https://nodejs.org/) 18 or newer
- The BASS audio libraries (see below)

### BASS libraries

MusicPi's audio engine uses [BASS](https://www.un4seen.com/) by un4seen. Its license doesn't allow redistributing it here, so download the **Windows x64** versions yourself from un4seen.com and copy the DLLs (from the `x64` folder of each zip) into `src-tauri/lib/`:

| File | Package | Required |
|---|---|---|
| `bass.dll` | BASS | yes |
| `bassmix.dll` | BASSmix | yes |
| `basswasapi.dll` | BASSWASAPI | yes |
| `bassflac.dll` | BASSFLAC | yes |
| `bass_fx.dll` | BASS_FX | for practice mode (speed) |
| `bassenc.dll` | BASSenc | for the lighter Opus stream |
| `bassenc_opus.dll` | BASSenc_OPUS | for the lighter Opus stream |

BASS is free for non-commercial use. Any other use needs a license from un4seen.

### Run and build

```
npm install
npm run dev
```

To build the installer:

```
npm run build
```

The installer ends up in `src-tauri/target/release/bundle/nsis/`.

### Android app

Open the `android` folder in [Android Studio](https://developer.android.com/studio), let it sync, then **Run** on your phone. On the PC, turn on sharing in **Settings › Phone** and enter the address and code shown there.

## Credits

- [MusicBee](https://getmusicbee.com/), the inspiration and the origin of the name
- [BASS](https://www.un4seen.com/) by un4seen Developments: audio engine
- [Tauri](https://tauri.app/): desktop app framework
- [Lucide](https://lucide.dev/): icons (ISC license)
- [Manrope](https://github.com/sharanda/manrope): typeface (SIL Open Font License)
- Rust crates: lofty, ureq, image, souvlaki, serde
- Android: Jetpack Compose, Media3 / ExoPlayer, OkHttp, Coil, kotlinx.serialization
- Online data: [LRCLIB](https://lrclib.net) (lyrics), [MusicBrainz](https://musicbrainz.org/), [Cover Art Archive](https://coverartarchive.org/), [Discogs](https://www.discogs.com/), Deezer, iTunes, [Last.fm](https://www.last.fm/)

## License

MusicPi's source code is under the [MIT license](LICENSE). BASS and the other libraries keep their own licenses.
