use super::{header, names, Server, JSON};
use crate::config::Config;
use crate::handover::Handover;
use crate::Ctx;
use std::io::Write;
use std::net::{IpAddr, SocketAddr, TcpListener};
use std::time::Duration;

impl Server<'_> {
    pub(super) fn device(&self, l: &TcpListener, ho: Option<&Handover>) {
        for mut s in l.incoming().flatten() {
            s.set_read_timeout(Some(Duration::from_secs(5))).ok();
            let res = crate::hooks::head(&mut s).map_or_else(|e| e, |head| self.device_reply(&head));
            drop(s.write_all(&res));
            if ho.is_some_and(Handover::stopping) {
                break;
            }
        }
        ho.map(Handover::left);
    }

    fn device_reply(&self, head: &str) -> Vec<u8> {
        let mut w = head.split_whitespace();
        let (method, target) = (w.next().unwrap_or_default(), w.next().unwrap_or_default());
        let ctx = Ctx {
            now: crate::now(),
            ..self.ctx.clone()
        };
        let hosts = Config::kept(&ctx).map(|c| c.serve.device_hosts).unwrap_or_default();
        if !lan_host(header(head, "host").unwrap_or_default(), &hosts) {
            return answer(
                "403 Forbidden",
                "text/plain",
                b"forbidden: Host is no IP or device_hosts name\n",
            );
        }
        if method != "GET" {
            return answer("405 Method Not Allowed", "text/plain", b"GET only\n");
        }
        if target.split('?').next() != Some("/screens/scene") {
            return answer("404 Not Found", "text/plain", b"only /screens/scene here\n");
        }
        self.scene(&ctx).map_or_else(
            || answer("503 Service Unavailable", "text/plain", b"the scene is not read yet\n"),
            |j| answer("200 OK", JSON, j.as_bytes()),
        )
    }
}

fn lan_host(host: &str, hosts: &[String]) -> bool {
    let bare = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
    host.parse::<SocketAddr>().is_ok() || bare.parse::<IpAddr>().is_ok() || hosts.iter().any(|h| names(h, host))
}

fn answer(status: &str, ctype: &str, body: &[u8]) -> Vec<u8> {
    let n = body.len();
    let mut res = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {n}\r\nAllow: GET\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n"
    )
    .into_bytes();
    res.extend_from_slice(body);
    res
}
