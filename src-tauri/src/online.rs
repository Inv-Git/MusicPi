//! Recherches en ligne : pochettes et fiches d'albums.
//! Sources : MusicBrainz (+ Cover Art Archive), Discogs (jeton personnel), iTunes, Deezer.

use std::io::Read;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

const UA: &str = "MusicPi/1.0 (https://github.com/Inv-Git/MusicPi)";

pub fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .user_agent(UA)
        .build()
}

fn get_json(req: ureq::Request) -> Result<Value, String> {
    req.call().map_err(|e| e.to_string())?.into_json::<Value>().map_err(|e| e.to_string())
}

fn s(v: &Value) -> String {
    match v {
        Value::String(x) => x.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// Télécharge une image (20 Mo maximum).
pub fn download(url: &str) -> Result<Vec<u8>, String> {
    let resp = agent().get(url).call().map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    resp.into_reader().take(20_000_000).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    if buf.len() < 100 {
        return Err("Image vide".into());
    }
    Ok(buf)
}

// ---------------------------------------------------------------------------
// Pochettes
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CoverHit {
    pub source: String,
    pub title: String,
    pub artist: String,
    pub year: String,
    /// Image en grand format, à intégrer.
    pub url: String,
    /// Vignette pour l'aperçu.
    pub thumb: String,
}

pub fn search_covers(artist: &str, album: &str, discogs_token: &str) -> (Vec<CoverHit>, Vec<String>) {
    let mut hits = vec![];
    let mut errors = vec![];
    let mut push = |name: &str, r: Result<Vec<CoverHit>, String>| match r {
        Ok(mut v) => hits.append(&mut v),
        Err(e) => errors.push(format!("{name} : {e}")),
    };
    if !discogs_token.is_empty() {
        push("Discogs", covers_discogs(artist, album, discogs_token));
    }
    push("MusicBrainz", covers_musicbrainz(artist, album));
    push("iTunes", covers_itunes(artist, album));
    push("Deezer", covers_deezer(artist, album));
    (hits, errors)
}

fn covers_itunes(artist: &str, album: &str) -> Result<Vec<CoverHit>, String> {
    let v = get_json(agent().get("https://itunes.apple.com/search").query("term", &format!("{artist} {album}")).query("entity", "album").query("limit", "12"))?;
    Ok(v["results"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    let art = s(&r["artworkUrl100"]);
                    (!art.is_empty()).then(|| CoverHit {
                        source: "iTunes".into(),
                        title: s(&r["collectionName"]),
                        artist: s(&r["artistName"]),
                        year: s(&r["releaseDate"]).chars().take(4).collect(),
                        url: art.replace("100x100bb", "3000x3000bb"),
                        thumb: art.replace("100x100bb", "400x400bb"),
                    })
                })
                .collect()
        })
        .unwrap_or_default())
}

fn covers_deezer(artist: &str, album: &str) -> Result<Vec<CoverHit>, String> {
    let q = format!("artist:\"{artist}\" album:\"{album}\"");
    let v = get_json(agent().get("https://api.deezer.com/search/album").query("q", &q).query("limit", "12"))?;
    Ok(v["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    let xl = s(&r["cover_xl"]);
                    (!xl.is_empty()).then(|| CoverHit {
                        source: "Deezer".into(),
                        title: s(&r["title"]),
                        artist: s(&r["artist"]["name"]),
                        year: String::new(),
                        thumb: s(&r["cover_big"]),
                        url: xl,
                    })
                })
                .collect()
        })
        .unwrap_or_default())
}

fn covers_musicbrainz(artist: &str, album: &str) -> Result<Vec<CoverHit>, String> {
    let q = format!("releasegroup:\"{}\" AND artist:\"{}\"", album.replace('"', ""), artist.replace('"', ""));
    let v = get_json(agent().get("https://musicbrainz.org/ws/2/release-group/").query("query", &q).query("fmt", "json").query("limit", "8"))?;
    Ok(v["release-groups"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| {
                    let id = s(&r["id"]);
                    CoverHit {
                        source: "MusicBrainz".into(),
                        title: s(&r["title"]),
                        artist: artist_credit(&r["artist-credit"]),
                        year: s(&r["first-release-date"]).chars().take(4).collect(),
                        url: format!("https://coverartarchive.org/release-group/{id}/front"),
                        thumb: format!("https://coverartarchive.org/release-group/{id}/front-500"),
                    }
                })
                .collect()
        })
        .unwrap_or_default())
}

fn covers_discogs(artist: &str, album: &str, token: &str) -> Result<Vec<CoverHit>, String> {
    let v = get_json(
        agent()
            .get("https://api.discogs.com/database/search")
            .query("type", "release")
            .query("artist", artist)
            .query("release_title", album)
            .query("per_page", "12")
            .query("token", token),
    )?;
    Ok(v["results"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    let img = s(&r["cover_image"]);
                    if img.is_empty() || img.contains("spacer.gif") {
                        return None;
                    }
                    let (ar, ti) = split_discogs_title(&s(&r["title"]));
                    Some(CoverHit { source: "Discogs".into(), title: ti, artist: ar, year: s(&r["year"]), thumb: s(&r["thumb"]), url: img })
                })
                .collect()
        })
        .unwrap_or_default())
}

// ---------------------------------------------------------------------------
// Fiches d'albums (correction automatique)
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseHit {
    pub source: String,
    pub id: String,
    pub title: String,
    pub artist: String,
    pub year: String,
    pub country: String,
    pub label: String,
    pub format: String,
    pub track_count: u32,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseTrack {
    pub disc: u32,
    pub track: u32,
    pub title: String,
    pub artist: String,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    pub source: String,
    pub title: String,
    pub artist: String,
    pub year: String,
    pub genre: String,
    pub cover: String,
    pub tracks: Vec<ReleaseTrack>,
}

pub fn search_releases(source: &str, artist: &str, album: &str, token: &str) -> Result<Vec<ReleaseHit>, String> {
    match source {
        "discogs" => releases_discogs(artist, album, token),
        _ => releases_musicbrainz(artist, album),
    }
}

pub fn release_details(source: &str, id: &str, token: &str) -> Result<ReleaseInfo, String> {
    match source {
        "discogs" => details_discogs(id, token),
        _ => details_musicbrainz(id),
    }
}

fn artist_credit(v: &Value) -> String {
    v.as_array()
        .map(|a| a.iter().map(|c| format!("{}{}", s(&c["name"]), s(&c["joinphrase"]))).collect::<String>())
        .unwrap_or_default()
}

fn releases_musicbrainz(artist: &str, album: &str) -> Result<Vec<ReleaseHit>, String> {
    let q = format!("release:\"{}\" AND artist:\"{}\"", album.replace('"', ""), artist.replace('"', ""));
    let v = get_json(agent().get("https://musicbrainz.org/ws/2/release/").query("query", &q).query("fmt", "json").query("limit", "20"))?;
    Ok(v["releases"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| ReleaseHit {
                    source: "musicbrainz".into(),
                    id: s(&r["id"]),
                    title: s(&r["title"]),
                    artist: artist_credit(&r["artist-credit"]),
                    year: s(&r["date"]).chars().take(4).collect(),
                    country: s(&r["country"]),
                    label: r["label-info"].as_array().and_then(|l| l.first()).map(|l| s(&l["label"]["name"])).unwrap_or_default(),
                    format: r["media"]
                        .as_array()
                        .map(|m| m.iter().map(|x| s(&x["format"])).filter(|f| !f.is_empty()).collect::<Vec<_>>().join(" + "))
                        .unwrap_or_default(),
                    track_count: r["track-count"].as_u64().unwrap_or(0) as u32,
                })
                .collect()
        })
        .unwrap_or_default())
}

fn details_musicbrainz(id: &str) -> Result<ReleaseInfo, String> {
    let v = get_json(
        agent()
            .get(&format!("https://musicbrainz.org/ws/2/release/{id}"))
            .query("inc", "recordings+artist-credits+genres+release-groups")
            .query("fmt", "json"),
    )?;
    let mut genres: Vec<(String, i64)> = v["genres"]
        .as_array()
        .or(v["release-group"]["genres"].as_array())
        .map(|g| g.iter().map(|x| (s(&x["name"]), x["count"].as_i64().unwrap_or(0))).collect())
        .unwrap_or_default();
    genres.sort_by(|a, b| b.1.cmp(&a.1));
    let mut info = ReleaseInfo {
        source: "MusicBrainz".into(),
        title: s(&v["title"]),
        artist: artist_credit(&v["artist-credit"]),
        year: s(&v["date"]).chars().take(4).collect(),
        genre: genres.first().map(|g| capitalize(&g.0)).unwrap_or_default(),
        cover: format!("https://coverartarchive.org/release/{id}/front"),
        tracks: vec![],
    };
    if let Some(media) = v["media"].as_array() {
        for (mi, m) in media.iter().enumerate() {
            let disc = m["position"].as_u64().unwrap_or(mi as u64 + 1) as u32;
            if let Some(ts) = m["tracks"].as_array() {
                for (ti, t) in ts.iter().enumerate() {
                    info.tracks.push(ReleaseTrack {
                        disc,
                        track: t["position"].as_u64().unwrap_or(ti as u64 + 1) as u32,
                        title: s(&t["title"]),
                        artist: artist_credit(&t["artist-credit"]),
                    });
                }
            }
        }
    }
    Ok(info)
}

/// « Daft Punk - Discovery » → (« Daft Punk », « Discovery »).
fn split_discogs_title(t: &str) -> (String, String) {
    match t.split_once(" - ") {
        Some((a, b)) => (clean_discogs_name(a), b.trim().to_string()),
        None => (String::new(), t.to_string()),
    }
}

/// Discogs numérote les homonymes : « Camille (2) » → « Camille ».
fn clean_discogs_name(n: &str) -> String {
    let n = n.trim().trim_end_matches('*');
    if let Some(i) = n.rfind(" (") {
        if n.ends_with(')') && n[i + 2..n.len() - 1].chars().all(|c| c.is_ascii_digit()) {
            return n[..i].to_string();
        }
    }
    n.to_string()
}

fn capitalize(x: &str) -> String {
    let mut c = x.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn releases_discogs(artist: &str, album: &str, token: &str) -> Result<Vec<ReleaseHit>, String> {
    if token.is_empty() {
        return Err("Ajoute ton jeton Discogs dans les paramètres".into());
    }
    let v = get_json(
        agent()
            .get("https://api.discogs.com/database/search")
            .query("type", "release")
            .query("artist", artist)
            .query("release_title", album)
            .query("per_page", "20")
            .query("token", token),
    )?;
    Ok(v["results"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| {
                    let (ar, ti) = split_discogs_title(&s(&r["title"]));
                    let list = |k: &str| r[k].as_array().map(|x| x.iter().map(s).collect::<Vec<_>>()).unwrap_or_default();
                    ReleaseHit {
                        source: "discogs".into(),
                        id: s(&r["id"]),
                        title: ti,
                        artist: ar,
                        year: s(&r["year"]),
                        country: s(&r["country"]),
                        label: list("label").first().cloned().unwrap_or_default(),
                        format: list("format").join(", "),
                        track_count: 0,
                    }
                })
                .collect()
        })
        .unwrap_or_default())
}

fn details_discogs(id: &str, token: &str) -> Result<ReleaseInfo, String> {
    let v = get_json(agent().get(&format!("https://api.discogs.com/releases/{id}")).query("token", token))?;
    let names = |a: &Value| -> String {
        let mut out = String::new();
        for c in a.as_array().into_iter().flatten() {
            let n = if s(&c["anv"]).is_empty() { s(&c["name"]) } else { s(&c["anv"]) };
            out.push_str(&clean_discogs_name(&n));
            match s(&c["join"]).as_str() {
                "" => {}
                "," => out.push_str(", "),
                j => {
                    out.push(' ');
                    out.push_str(j);
                    out.push(' ');
                }
            }
        }
        out.trim().to_string()
    };
    let genre = v["styles"]
        .as_array()
        .and_then(|a| a.first())
        .or(v["genres"].as_array().and_then(|a| a.first()))
        .map(s)
        .unwrap_or_default();
    let cover = v["images"]
        .as_array()
        .and_then(|imgs| imgs.iter().find(|i| s(&i["type"]) == "primary").or(imgs.first()))
        .map(|i| s(&i["uri"]))
        .unwrap_or_default();
    let mut info = ReleaseInfo {
        source: "Discogs".into(),
        title: s(&v["title"]),
        artist: names(&v["artists"]),
        year: s(&v["year"]).trim_start_matches('0').to_string(),
        genre,
        cover,
        tracks: vec![],
    };
    // Positions Discogs : « 3 », « 1-03 », « CD2-4 », « A1 »… On numérote dans l'ordre, par disque.
    let mut disc = 1u32;
    let mut n = 0u32;
    if let Some(list) = v["tracklist"].as_array() {
        for t in list {
            if s(&t["type_"]) != "track" {
                continue;
            }
            let pos = s(&t["position"]);
            if let Some((d, _)) = pos.split_once('-') {
                let d: String = d.chars().filter(|c| c.is_ascii_digit()).collect();
                if let Ok(d) = d.parse::<u32>() {
                    if d != disc {
                        disc = d;
                        n = 0;
                    }
                }
            }
            // Vinyle (faces A1, B2…) : un seul disque, numéroté en continu.
            n += 1;
            let artist = names(&t["artists"]);
            info.tracks.push(ReleaseTrack { disc, track: n, title: s(&t["title"]), artist: if artist.is_empty() { info.artist.clone() } else { artist } });
        }
    }
    Ok(info)
}

// ---------------------------------------------------------------------------
// Photos d'artistes
// ---------------------------------------------------------------------------

/// Photos d'artistes trouvées sur Deezer (et Discogs avec un jeton).
/// Dans chaque résultat, `artist` contient le nom de l'artiste trouvé.
pub fn artist_hits(name: &str, discogs_token: &str) -> (Vec<CoverHit>, Vec<String>) {
    let mut hits = vec![];
    let mut errors = vec![];
    match get_json(agent().get("https://api.deezer.com/search/artist").query("q", name).query("limit", "8")) {
        Ok(v) => {
            for r in v["data"].as_array().into_iter().flatten() {
                let xl = s(&r["picture_xl"]);
                // Deezer renvoie une silhouette générique quand il n'a pas de photo.
                if xl.is_empty() || xl.contains("/artist//") {
                    continue;
                }
                hits.push(CoverHit { source: "Deezer".into(), title: s(&r["name"]), artist: s(&r["name"]), year: String::new(), thumb: s(&r["picture_big"]), url: xl });
            }
        }
        Err(e) => errors.push(format!("Deezer : {e}")),
    }
    if !discogs_token.is_empty() {
        match get_json(agent().get("https://api.discogs.com/database/search").query("type", "artist").query("q", name).query("per_page", "8").query("token", discogs_token)) {
            Ok(v) => {
                for r in v["results"].as_array().into_iter().flatten() {
                    let img = s(&r["cover_image"]);
                    if img.is_empty() || img.contains("spacer.gif") {
                        continue;
                    }
                    let n = clean_discogs_name(&s(&r["title"]));
                    hits.push(CoverHit { source: "Discogs".into(), title: n.clone(), artist: n, year: String::new(), thumb: s(&r["thumb"]), url: img });
                }
            }
            Err(e) => errors.push(format!("Discogs : {e}")),
        }
    }
    (hits, errors)
}
