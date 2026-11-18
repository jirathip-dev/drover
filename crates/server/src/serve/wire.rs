use flate2::write::GzEncoder;
use flate2::Compression;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Mutex;

const PACKED: [&str; 4] = ["application/json", "text/javascript", "text/css", "image/svg+xml"];
static ASSETS: Mutex<BTreeMap<String, Vec<u8>>> = Mutex::new(BTreeMap::new());

fn names(head: &str, header: &str, token: &str) -> bool {
    super::header(head, header).is_some_and(|v| {
        v.split(',').any(|e| {
            let mut it = e.split(';').map(str::trim);
            it.next().is_some_and(|t| t.eq_ignore_ascii_case(token)) && it.all(|p| p != "q=0")
        })
    })
}

/// Whether the request takes a gzip body (`Accept-Encoding: gzip`).
pub fn gzip(head: &str) -> bool {
    names(head, "accept-encoding", "gzip")
}

/// Whether the connection stays open: HTTP/1.1 unless `Connection: close`, HTTP/1.0 only with `Connection: keep-alive`.
pub fn keep(head: &str) -> bool {
    let v11 = head.lines().next().is_some_and(|l| l.ends_with(" HTTP/1.1"));
    !names(head, "connection", "close") && (v11 || names(head, "connection", "keep-alive"))
}

/// One request off the socket: its bytes, and whether its body was over the limit; what follows stays in `buf`.
pub fn request(s: &mut TcpStream, buf: &mut Vec<u8>) -> std::io::Result<(Vec<u8>, bool)> {
    let (mut chunk, mut want, mut over) = (vec![0u8; 1 << 16], None, false);
    loop {
        let end = buf.windows(4).position(|w| w == b"\r\n\r\n");
        if let Some(i) = end.filter(|_| want.is_none()) {
            let h = String::from_utf8_lossy(buf.get(..i).unwrap_or_default());
            let n: usize = super::header(&h, "content-length")
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            let most = if h.starts_with("POST /thread ") || h.starts_with("POST /action ") {
                140 << 20
            } else if h.starts_with("POST /report ") {
                3 << 20
            } else {
                32_768
            };
            over = i + 4 + n > most;
            want = Some((i + 4 + n).min(most));
        }
        if want.map_or(buf.len() >= 32_768, |w| buf.len() >= w) {
            break;
        }
        match s.read(&mut chunk)? {
            0 => break,
            n => buf.extend_from_slice(chunk.get(..n).unwrap_or_default()),
        }
    }
    let rest = buf.split_off(want.unwrap_or(buf.len()).min(buf.len()));
    Ok((std::mem::replace(buf, rest), over))
}

fn pack(body: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut z = GzEncoder::new(Vec::with_capacity(body.len() / 4), Compression::default());
    z.write_all(body)?;
    z.finish()
}

/// The response as sent: gzipped when asked and worth it (a hashed asset's once), kept open when asked.
pub fn finish(res: Vec<u8>, path: &str, gz: bool, keep: bool) -> Vec<u8> {
    let Some((head, body)) = res
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .and_then(|i| res.split_at_checked(i + 4))
    else {
        return res;
    };
    let mut head = String::from_utf8_lossy(head).into_owned();
    if keep {
        head = head.replace("Connection: close\r\n", "Connection: keep-alive\r\n");
    }
    let ctype = super::header(&head, "content-type").unwrap_or_default();
    let worth = gz
        && body.len() > 1024
        && PACKED.iter().any(|p| ctype.starts_with(p))
        && super::header(&head, "content-encoding").is_none();
    let packed = worth.then(|| {
        if !super::header(&head, "cache-control").is_some_and(|c| c.contains("immutable")) {
            return pack(body).ok();
        }
        let mut kept = super::lock(&ASSETS);
        if let Some(z) = kept.get(path) {
            return Some(z.clone());
        }
        let z = pack(body).ok()?;
        kept.insert(path.to_string(), z.clone());
        Some(z)
    });
    let Some(z) = packed.flatten() else {
        return if keep { [head.as_bytes(), body].concat() } else { res };
    };
    let head = head.replacen(
        &format!("Content-Length: {}\r\n", body.len()),
        &format!(
            "Content-Length: {}\r\nContent-Encoding: gzip\r\nVary: Accept-Encoding\r\n",
            z.len()
        ),
        1,
    );
    [head.into_bytes(), z].concat()
}

/// An event stream's bytes, gzipped and flushed message by message when the client took gzip.
pub struct Stream(Option<GzEncoder<Vec<u8>>>);

impl Stream {
    /// A stream for the request `head`.
    pub fn new(head: &str) -> Self {
        Self(gzip(head).then(|| GzEncoder::new(vec![], Compression::default())))
    }

    /// `msg`'s bytes on the wire: its status line and headers as they are, the rest through the stream's gzip.
    pub fn frame(&mut self, msg: &str) -> std::io::Result<Vec<u8>> {
        let Some(z) = self.0.as_mut() else {
            return Ok(msg.as_bytes().to_vec());
        };
        let (head, body) = match msg.split_once("\r\n\r\n") {
            Some((h, b)) if msg.starts_with("HTTP/") => (format!("{h}\r\nContent-Encoding: gzip\r\n\r\n"), b),
            _ => (String::new(), msg),
        };
        z.write_all(body.as_bytes())?;
        z.flush()?;
        Ok([head.into_bytes(), std::mem::take(z.get_mut())].concat())
    }
}
