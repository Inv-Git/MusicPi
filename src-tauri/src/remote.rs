//! Petit serveur HTTP (bibliothèque standard uniquement) pour l'app téléphone :
//! réponses JSON, envoi de fichiers avec reprise (Range) et flux d'événements (SSE).

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::Duration;

pub struct Request {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    /// Noms en minuscules.
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    #[allow(dead_code)]
    pub peer: Option<SocketAddr>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(|s| s.as_str())
    }
}

pub enum Body {
    Bytes(Vec<u8>),
    /// Fichier envoyé tel quel, avec prise en charge de « Range ».
    File(PathBuf),
    /// Flux d'événements : chaque message reçu est envoyé tel quel (« data: … »).
    Events(Receiver<String>),
}

pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Body,
}

impl Response {
    pub fn bytes(status: u16, content_type: &str, b: Vec<u8>) -> Self {
        Response { status, headers: vec![("Content-Type".into(), content_type.into())], body: Body::Bytes(b) }
    }
    pub fn json(s: String) -> Self {
        Self::bytes(200, "application/json; charset=utf-8", s.into_bytes())
    }
    pub fn error(status: u16, msg: &str) -> Self {
        let m = msg.replace('\\', "\\\\").replace('"', "\\\"");
        Self::bytes(status, "application/json; charset=utf-8", format!("{{\"error\":\"{m}\"}}").into_bytes())
    }
    pub fn file(path: PathBuf, content_type: &str) -> Self {
        Response { status: 200, headers: vec![("Content-Type".into(), content_type.into())], body: Body::File(path) }
    }
    pub fn events(rx: Receiver<String>) -> Self {
        Response { status: 200, headers: vec![], body: Body::Events(rx) }
    }
    pub fn with_header(mut self, k: &str, v: &str) -> Self {
        self.headers.push((k.into(), v.into()));
        self
    }
}

pub trait Handler: Send + Sync + 'static {
    fn handle(&self, req: &Request) -> Response;
}

/// Serveur en marche ; l'arrêter libère le port.
pub struct Server {
    stop: Arc<AtomicBool>,
    port: u16,
}

impl Server {
    pub fn start(port: u16, handler: Arc<dyn Handler>) -> io::Result<Server> {
        let listener = TcpListener::bind(("0.0.0.0", port))?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let st = stop.clone();
        std::thread::Builder::new().name("serveur-telephone".into()).spawn(move || {
            for conn in listener.incoming() {
                if st.load(Ordering::Acquire) {
                    break;
                }
                let Ok(s) = conn else { continue };
                let h = handler.clone();
                let st2 = st.clone();
                let _ = std::thread::Builder::new().name("client-telephone".into()).spawn(move || {
                    let _ = serve_conn(s, &*h, &st2);
                });
            }
        })?;
        Ok(Server { stop, port })
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Débloque l'attente de connexion.
        let _ = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], self.port)), Duration::from_millis(300));
    }
}

const MAX_BODY: usize = 1 << 20;

fn serve_conn(stream: TcpStream, h: &dyn Handler, stop: &AtomicBool) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    let _ = stream.set_nodelay(true);
    let peer = stream.peer_addr().ok();
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut out = stream;
    // Connexions persistantes : plusieurs requêtes à la suite sur la même connexion.
    loop {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        let Some(req) = read_request(&mut reader, peer)? else { return Ok(()) };
        let close = req.header("connection").map(|c| c.eq_ignore_ascii_case("close")).unwrap_or(false);
        let head_only = req.method == "HEAD";
        let resp = h.handle(&req);
        let keep = write_response(&mut out, &req, resp, head_only, stop)?;
        if close || !keep {
            let _ = out.shutdown(Shutdown::Both);
            return Ok(());
        }
    }
}

fn read_request(r: &mut BufReader<TcpStream>, peer: Option<SocketAddr>) -> io::Result<Option<Request>> {
    let mut line = String::new();
    if r.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else { return Ok(None) };
    let method = method.to_string();
    let (path, qs) = target.split_once('?').unwrap_or((target, ""));
    let path = percent_decode(path);
    let query = parse_query(qs);
    let mut headers = HashMap::new();
    loop {
        let mut l = String::new();
        if r.read_line(&mut l)? == 0 {
            return Ok(None);
        }
        let l = l.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let len: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    if len > MAX_BODY {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "corps trop gros"));
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    Ok(Some(Request { method, path, query, headers, body, peer }))
}

pub fn parse_query(qs: &str) -> HashMap<String, String> {
    qs.split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (percent_decode(k), percent_decode(&v.replace('+', " ")))
        })
        .collect()
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        204 => "No Content",
        206 => "Partial Content",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        416 => "Range Not Satisfiable",
        429 => "Too Many Requests",
        _ => "Error",
    }
}

fn head(out: &mut TcpStream, status: u16, headers: &[(String, String)], len: Option<u64>) -> io::Result<()> {
    let mut h = format!("HTTP/1.1 {status} {}\r\n", reason(status));
    for (k, v) in headers {
        h.push_str(&format!("{k}: {v}\r\n"));
    }
    if let Some(l) = len {
        h.push_str(&format!("Content-Length: {l}\r\n"));
    }
    h.push_str("\r\n");
    out.write_all(h.as_bytes())
}

/// « bytes=début-fin » → (début, fin incluse), bornés à la taille du fichier.
pub fn parse_range(v: &str, size: u64) -> Option<(u64, u64)> {
    let spec = v.trim().strip_prefix("bytes=")?;
    let first = spec.split(',').next()?.trim();
    let (a, b) = first.split_once('-')?;
    if size == 0 {
        return None;
    }
    if a.is_empty() {
        // Les N derniers octets.
        let n: u64 = b.parse().ok()?;
        if n == 0 {
            return None;
        }
        return Some((size.saturating_sub(n), size - 1));
    }
    let start: u64 = a.parse().ok()?;
    if start >= size {
        return None;
    }
    let end = if b.is_empty() { size - 1 } else { b.parse::<u64>().ok()?.min(size - 1) };
    if end < start {
        return None;
    }
    Some((start, end))
}

/// Renvoie `false` si la connexion doit être fermée ensuite.
fn write_response(out: &mut TcpStream, req: &Request, resp: Response, head_only: bool, stop: &AtomicBool) -> io::Result<bool> {
    let mut headers = resp.headers;
    match resp.body {
        Body::Bytes(b) => {
            head(out, resp.status, &headers, Some(b.len() as u64))?;
            if !head_only {
                out.write_all(&b)?;
            }
            Ok(true)
        }
        Body::File(p) => {
            let mut f = match File::open(&p) {
                Ok(f) => f,
                Err(_) => {
                    let r = Response::error(404, "fichier introuvable");
                    return write_response(out, req, r, head_only, stop);
                }
            };
            let size = f.metadata()?.len();
            headers.push(("Accept-Ranges".into(), "bytes".into()));
            let (status, start, len) = match req.header("range") {
                Some(r) => match parse_range(r, size) {
                    Some((a, b)) => {
                        headers.push(("Content-Range".into(), format!("bytes {a}-{b}/{size}")));
                        (206, a, b - a + 1)
                    }
                    None => {
                        headers.push(("Content-Range".into(), format!("bytes */{size}")));
                        head(out, 416, &headers, Some(0))?;
                        return Ok(true);
                    }
                },
                None => (200, 0, size),
            };
            head(out, status, &headers, Some(len))?;
            if head_only {
                return Ok(true);
            }
            f.seek(SeekFrom::Start(start))?;
            let mut left = len;
            let mut buf = vec![0u8; 64 * 1024];
            while left > 0 {
                let want = left.min(buf.len() as u64) as usize;
                let n = f.read(&mut buf[..want])?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n])?;
                left -= n as u64;
            }
            Ok(true)
        }
        Body::Events(rx) => {
            headers.push(("Content-Type".into(), "text/event-stream".into()));
            headers.push(("Cache-Control".into(), "no-cache".into()));
            headers.push(("Connection".into(), "close".into()));
            head(out, 200, &headers, None)?;
            out.flush()?;
            loop {
                if stop.load(Ordering::Acquire) {
                    return Ok(false);
                }
                match rx.recv_timeout(Duration::from_secs(15)) {
                    Ok(msg) => {
                        let mut s = String::with_capacity(msg.len() + 8);
                        // « event: nom » en première ligne : événement nommé.
                        let body = match msg.strip_prefix("event: ").and_then(|r| r.split_once('\n')) {
                            Some((name, rest)) => {
                                s.push_str("event: ");
                                s.push_str(name);
                                s.push('\n');
                                rest
                            }
                            None => msg.as_str(),
                        };
                        for l in body.lines() {
                            s.push_str("data: ");
                            s.push_str(l);
                            s.push('\n');
                        }
                        s.push('\n');
                        out.write_all(s.as_bytes())?;
                    }
                    // Ligne de commentaire : garde la connexion ouverte (box, 4G…).
                    Err(RecvTimeoutError::Timeout) => out.write_all(b":\n\n")?,
                    Err(RecvTimeoutError::Disconnected) => return Ok(false),
                }
                out.flush()?;
            }
        }
    }
}

/// Adresses où le téléphone peut joindre ce PC : réseau local, et Tailscale s'il est installé.
pub fn local_addresses() -> Vec<String> {
    let probe = |target: &str| -> Option<std::net::IpAddr> {
        let s = UdpSocket::bind("0.0.0.0:0").ok()?;
        s.connect(target).ok()?;
        Some(s.local_addr().ok()?.ip())
    };
    let mut out = vec![];
    if let Some(ip) = probe("192.0.2.1:9").or_else(|| probe("8.8.8.8:53")) {
        if !ip.is_loopback() && !ip.is_unspecified() && !is_tailscale(&ip) {
            out.push(ip.to_string());
        }
    }
    if let Some(ip) = probe("100.100.100.100:53") {
        if is_tailscale(&ip) {
            out.push(ip.to_string());
        }
    }
    out
}

fn is_tailscale(ip: &std::net::IpAddr) -> bool {
    match ip {
        // 100.64.0.0/10
        std::net::IpAddr::V4(v) => v.octets()[0] == 100 && (v.octets()[1] & 0xC0) == 64,
        _ => false,
    }
}

/// Jeton aléatoire (hexadécimal) tiré de l'aléa du système.
pub fn random_hex(bytes: usize) -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut s = String::new();
    while s.len() < bytes * 2 {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        s.push_str(&format!("{:016x}", h.finish()));
    }
    s.truncate(bytes * 2);
    s
}

pub fn random_code() -> String {
    let n = u64::from_str_radix(&random_hex(8), 16).unwrap_or(0);
    format!("{:06}", n % 1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::mpsc;
    use std::sync::Mutex;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-", 100), Some((0, 99)));
        assert_eq!(parse_range("bytes=10-19", 100), Some((10, 19)));
        assert_eq!(parse_range("bytes=90-200", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=-10", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=100-", 100), None);
        assert_eq!(parse_range("bytes=5-2", 100), None);
    }

    #[test]
    fn decode() {
        assert_eq!(percent_decode("D%3A%5CMusic%5C%C3%A9t%C3%A9.flac"), "D:\\Music\\été.flac");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%2"), "a%2");
        let q = parse_query("p=a%26b&t=xyz&x");
        assert_eq!(q["p"], "a&b");
        assert_eq!(q["t"], "xyz");
        assert_eq!(q["x"], "");
    }

    #[test]
    fn codes() {
        let a = random_hex(16);
        let b = random_hex(16);
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
        assert_eq!(random_code().len(), 6);
    }

    struct H {
        file: PathBuf,
        tx: Mutex<Option<mpsc::Sender<String>>>,
    }
    impl Handler for H {
        fn handle(&self, req: &Request) -> Response {
            match req.path.as_str() {
                "/json" => Response::json(format!("{{\"m\":\"{}\",\"b\":{}}}", req.method, req.body.len())),
                "/file" => Response::file(self.file.clone(), "audio/flac"),
                "/events" => {
                    let (tx, rx) = mpsc::channel();
                    tx.send("bonjour".into()).unwrap();
                    *self.tx.lock().unwrap() = Some(tx);
                    Response::events(rx)
                }
                _ => Response::error(404, "inconnu"),
            }
        }
    }

    fn raw(port: u16, req: &str) -> Vec<u8> {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(req.as_bytes()).unwrap();
        let mut v = vec![];
        s.read_to_end(&mut v).unwrap();
        v
    }

    #[test]
    fn serveur() {
        let dir = std::env::temp_dir().join(format!("remote-test-{}", random_hex(4)));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.bin");
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&file, &data).unwrap();
        let h = Arc::new(H { file, tx: Mutex::new(None) });
        let srv = Server::start(0, h.clone()).unwrap();
        let port = srv.port();

        let r = String::from_utf8(raw(port, "POST /json HTTP/1.1\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc")).unwrap();
        assert!(r.starts_with("HTTP/1.1 200"), "{r}");
        assert!(r.ends_with("{\"m\":\"POST\",\"b\":3}"), "{r}");

        // Deux requêtes sur la même connexion.
        let r = String::from_utf8(raw(port, "GET /json HTTP/1.1\r\n\r\nGET /nope HTTP/1.1\r\nConnection: close\r\n\r\n")).unwrap();
        assert_eq!(r.matches("HTTP/1.1 ").count(), 2, "{r}");
        assert!(r.contains("404 Not Found"));

        let r = raw(port, "GET /file HTTP/1.1\r\nRange: bytes=1000-1999\r\nConnection: close\r\n\r\n");
        let pos = r.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let hd = String::from_utf8_lossy(&r[..pos]);
        assert!(hd.starts_with("HTTP/1.1 206"), "{hd}");
        assert!(hd.contains("Content-Range: bytes 1000-1999/200000"));
        assert_eq!(&r[pos + 4..], &data[1000..2000]);

        let r = raw(port, "GET /file HTTP/1.1\r\nConnection: close\r\n\r\n");
        let pos = r.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        assert_eq!(&r[pos + 4..], &data[..]);

        let r = String::from_utf8(raw(port, "GET /file HTTP/1.1\r\nRange: bytes=300000-\r\nConnection: close\r\n\r\n")).unwrap();
        assert!(r.starts_with("HTTP/1.1 416"));

        // Flux d'événements.
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(b"GET /events HTTP/1.1\r\n\r\n").unwrap();
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut got = String::new();
        let mut buf = [0u8; 1024];
        while !got.contains("data: bonjour\n\n") {
            let n = s.read(&mut buf).unwrap();
            got.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        h.tx.lock().unwrap().as_ref().unwrap().send("{\"a\":1}".into()).unwrap();
        h.tx.lock().unwrap().as_ref().unwrap().send("event: coll\n{\"v\":2}".into()).unwrap();
        while !got.contains("data: {\"a\":1}\n\n") {
            let n = s.read(&mut buf).unwrap();
            got.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        while !got.contains("event: coll\ndata: {\"v\":2}\n\n") {
            let n = s.read(&mut buf).unwrap();
            got.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        assert!(got.contains("text/event-stream"));
        drop(srv);
        let _ = std::fs::remove_dir_all(dir);
    }
}
