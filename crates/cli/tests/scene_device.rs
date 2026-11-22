mod common;
use common::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct Serve {
    p: Child,
    board: String,
    device: Option<String>,
}

impl Drop for Serve {
    fn drop(&mut self) {
        let _ = (self.p.kill(), self.p.wait());
    }
}

fn serve(name: &str, config: &str) -> Serve {
    let c = ctx(&tmpdir(name), config);
    let mut p = Command::new(env!("CARGO_BIN_EXE_drover"))
        .args(["serve", "--addr", "127.0.0.1:0"])
        .env("FLEET_CONFIG", &c.config)
        .env("FLEET_STATE", &c.state)
        .env("FLEET_WORKTREES", &c.worktrees)
        .env("DROVER_HOME", &c.home)
        .env("PATH", "/usr/bin:/bin")
        .env_remove("DROVER_LANE")
        .env_remove("DROVER_SERVE_FDS")
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut err = BufReader::new(p.stderr.take().unwrap());
    let mut line = || {
        let mut l = String::new();
        err.read_line(&mut l).unwrap();
        l.trim().to_string()
    };
    let first = line();
    let board = first
        .strip_prefix("drover serve: phone page on http://")
        .unwrap_or_else(|| panic!("{first}"))
        .trim_end_matches('/')
        .to_string();
    let device = config.contains("device_addr = \"127").then(|| {
        let l = line();
        l.strip_prefix("drover serve: scene device on http://")
            .and_then(|a| a.strip_suffix("/screens/scene"))
            .unwrap_or_else(|| panic!("{l}"))
            .to_string()
    });
    std::thread::spawn(move || err.lines().map_while(Result::ok).for_each(drop));
    Serve { p, board, device }
}

fn request(addr: &str, method: &str, path: &str, host: &str) -> (String, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let req = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).unwrap();
    let mut res = String::new();
    s.read_to_string(&mut res).unwrap();
    let (head, body) = res.split_once("\r\n\r\n").unwrap();
    (head.lines().next().unwrap().to_string(), body.to_string())
}

fn listening(pid: u32) -> Vec<String> {
    let out = Command::new("lsof")
        .args(["-nP", "-a", "-p", &pid.to_string(), "-iTCP", "-sTCP:LISTEN", "-Fn"])
        .output()
        .unwrap();
    let mut v: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.strip_prefix('n').map(String::from))
        .collect();
    v.sort();
    v
}

#[test]
fn with_device_addr_unset_serve_listens_on_the_board_alone() {
    let s = serve("device-off", "[serve]\nhooks_addr = \"\"\n");
    assert_eq!(listening(s.p.id()), vec![s.board.clone()]);
    let s = serve("device-empty", "[serve]\nhooks_addr = \"\"\ndevice_addr = \"\"\n");
    assert_eq!(listening(s.p.id()), vec![s.board.clone()]);
}

#[test]
fn the_device_listener_serves_the_unpaired_scene_and_nothing_else() {
    let s = serve(
        "device-on",
        "[serve]\nhooks_addr = \"\"\ndevice_addr = \"127.0.0.1:0\"\ndevice_hosts = [\"scene.lan\"]\n",
    );
    let dev = s.device.clone().unwrap();
    let mut want = vec![s.board.clone(), dev.clone()];
    want.sort();
    assert_eq!(listening(s.p.id()), want);

    let main = || request(&s.board, "GET", "/screens/scene", &s.board);
    let (st, body) = request(&dev, "GET", "/screens/scene", &dev);
    let (mst, mut mbody) = main();
    if mbody != body {
        mbody = main().1;
    }
    assert_eq!((st.as_str(), mst.as_str()), ("HTTP/1.1 200 OK", "HTTP/1.1 200 OK"));
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["id"], "scene");
    assert_eq!(body, mbody);

    let port = dev.rsplit_once(':').unwrap().1;
    for host in [
        "192.168.1.20",
        "192.168.1.20:7779",
        "[::1]:7779",
        "scene.lan",
        &format!("scene.lan:{port}"),
    ] {
        assert_eq!(
            request(&dev, "GET", "/screens/scene", host).0,
            "HTTP/1.1 200 OK",
            "{host}"
        );
    }
    for host in ["evil.example", "evil.example:7779", "scene.lan.evil.example", ""] {
        assert_eq!(
            request(&dev, "GET", "/screens/scene", host).0,
            "HTTP/1.1 403 Forbidden",
            "{host}"
        );
    }
    for path in [
        "/",
        "/screens/queue",
        "/screens/scene/stream",
        "/live",
        "/api/v1/health",
    ] {
        assert_eq!(request(&dev, "GET", path, &dev).0, "HTTP/1.1 404 Not Found", "{path}");
    }
    for (method, path) in [
        ("POST", "/action"),
        ("POST", "/screens/scene"),
        ("PUT", "/screens/scene"),
        ("DELETE", "/"),
    ] {
        assert_eq!(
            request(&dev, method, path, &dev).0,
            "HTTP/1.1 405 Method Not Allowed",
            "{method} {path}"
        );
    }
    let (head, _) = {
        let mut st = TcpStream::connect(&dev).unwrap();
        st.write_all(
            format!("GET /screens/scene HTTP/1.1\r\nHost: {dev}\r\nOrigin: http://evil.example\r\n\r\n").as_bytes(),
        )
        .unwrap();
        let mut res = String::new();
        st.read_to_string(&mut res).unwrap();
        res.split_once("\r\n\r\n")
            .map(|(h, b)| (h.to_string(), b.to_string()))
            .unwrap()
    };
    assert!(!head.to_ascii_lowercase().contains("access-control-"), "{head}");
}
