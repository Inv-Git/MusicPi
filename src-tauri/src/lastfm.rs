//! Last.fm : connexion, « en cours d'écoute » et scrobbles.
//! Il faut une clé d'API personnelle (gratuite) : https://www.last.fm/api/account/create

use std::collections::BTreeMap;

/// MD5 (RFC 1321), pour signer les requêtes Last.fm.
pub fn md5_hex(data: &[u8]) -> String {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16,
        23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: Vec<u32> = (0..64).map(|i| ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32).collect();
    let (mut a0, mut b0, mut c0, mut d0): (u32, u32, u32, u32) = (0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476);
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());
    for chunk in msg.chunks(64) {
        let m: Vec<u32> = chunk.chunks(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    [a0, b0, c0, d0].iter().flat_map(|x| x.to_le_bytes()).map(|b| format!("{b:02x}")).collect()
}

/// Signature Last.fm : paramètres triés, concaténés (clé + valeur), plus le secret, en MD5.
pub fn sign(params: &BTreeMap<String, String>, secret: &str) -> String {
    let mut s = String::new();
    for (k, v) in params {
        if k == "format" || k == "callback" {
            continue;
        }
        s.push_str(k);
        s.push_str(v);
    }
    s.push_str(secret);
    md5_hex(s.as_bytes())
}

pub struct Account {
    pub api_key: String,
    pub secret: String,
    pub session: String,
}

fn call(method: &str, mut params: BTreeMap<String, String>, key: &str, secret: &str) -> Result<serde_json::Value, String> {
    params.insert("method".into(), method.into());
    params.insert("api_key".into(), key.into());
    let sig = sign(&params, secret);
    params.insert("api_sig".into(), sig);
    params.insert("format".into(), "json".into());
    let form: Vec<(&str, &str)> = params.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let resp = ureq::post("https://ws.audioscrobbler.com/2.0/").timeout(std::time::Duration::from_secs(12)).send_form(&form);
    let v: serde_json::Value = match resp {
        Ok(r) => r.into_json().map_err(|e| e.to_string())?,
        Err(ureq::Error::Status(_, r)) => r.into_json().map_err(|e| e.to_string())?,
        Err(e) => return Err(format!("Last.fm injoignable ({e})")),
    };
    if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
        if v.get("error").is_some() {
            return Err(msg.to_string());
        }
    }
    Ok(v)
}

/// Connexion : renvoie la clé de session (à garder, le mot de passe n'est pas conservé).
pub fn login(key: &str, secret: &str, user: &str, password: &str) -> Result<String, String> {
    let mut p = BTreeMap::new();
    p.insert("username".to_string(), user.to_string());
    p.insert("password".to_string(), password.to_string());
    let v = call("auth.getMobileSession", p, key, secret)?;
    v["session"]["key"].as_str().map(|s| s.to_string()).ok_or_else(|| "Réponse inattendue de Last.fm".into())
}

pub struct Scrobble {
    pub artist: String,
    pub title: String,
    pub album: String,
    pub duration: u32,
    pub timestamp: u64,
}

fn track_params(s: &Scrobble, session: &str) -> BTreeMap<String, String> {
    let mut p = BTreeMap::new();
    p.insert("artist".to_string(), s.artist.clone());
    p.insert("track".to_string(), s.title.clone());
    if !s.album.is_empty() {
        p.insert("album".to_string(), s.album.clone());
    }
    if s.duration > 0 {
        p.insert("duration".to_string(), s.duration.to_string());
    }
    p.insert("sk".to_string(), session.to_string());
    p
}

pub fn now_playing(acc: &Account, s: &Scrobble) -> Result<(), String> {
    call("track.updateNowPlaying", track_params(s, &acc.session), &acc.api_key, &acc.secret).map(|_| ())
}

pub fn scrobble(acc: &Account, s: &Scrobble) -> Result<(), String> {
    let mut p = track_params(s, &acc.session);
    p.insert("timestamp".to_string(), s.timestamp.to_string());
    call("track.scrobble", p, &acc.api_key, &acc.secret).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_vecteurs() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(md5_hex(b"The quick brown fox jumps over the lazy dog"), "9e107d9d372bb6826bd81d3542a419d6");
        assert_eq!(md5_hex(&[b'a'; 200]), "887f30b43b2867f4a9accceee7d16e6c");
        assert_eq!(md5_hex("é".as_bytes()), "66ddcd97cfdeabb2f6fb8a999b4bc76f");
    }

    #[test]
    fn signature() {
        let mut p = BTreeMap::new();
        p.insert("method".to_string(), "auth.getMobileSession".to_string());
        p.insert("api_key".to_string(), "k".to_string());
        p.insert("format".to_string(), "json".to_string());
        assert_eq!(sign(&p, "s"), md5_hex(b"api_keykmethodauth.getMobileSessions"));
    }
}
