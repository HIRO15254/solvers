//! Public SPA assets must not weaken authentication of the daemon API.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};

struct UiDaemon {
    child: Child,
    port: u16,
    _home: tempfile::TempDir,
}

impl Drop for UiDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl UiDaemon {
    fn start(with_ui: bool) -> Self {
        let home = tempfile::tempdir().unwrap();
        let assets = home.path().join("assets");
        std::fs::create_dir(&assets).unwrap();
        std::fs::write(assets.join("index.html"), "<html>SPA</html>").unwrap();
        std::fs::write(assets.join("app.js"), "console.log('asset');").unwrap();
        std::fs::write(home.path().join("secret.txt"), "private").unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_solversd"));
        command
            .args(["--bind", "127.0.0.1:0", "--token", "test-token"])
            .arg("--runs")
            .arg(home.path().join("runs"))
            .stdout(Stdio::piped());
        if with_ui {
            command.arg("--ui").arg(&assets);
        }
        let mut child = command.spawn().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        let port: u16 = line
            .split("(port ")
            .nth(1)
            .unwrap()
            .trim_end()
            .trim_end_matches(')')
            .parse()
            .unwrap();
        line.clear();
        output.read_line(&mut line).unwrap();
        assert!(line.starts_with("runs root: "));
        line.clear();
        output.read_line(&mut line).unwrap();
        assert_eq!(line.trim_end(), "token: test-token");
        if with_ui {
            line.clear();
            output.read_line(&mut line).unwrap();
            assert_eq!(
                line.trim_end(),
                format!("gui: http://127.0.0.1:{port}/#token=test-token")
            );
        }
        Self {
            child,
            port,
            _home: home,
        }
    }

    fn request(&self, method: &str, path: &str, token: bool) -> (u16, String, String) {
        let mut socket = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        let auth = if token {
            "Authorization: Bearer test-token\r\n"
        } else {
            ""
        };
        write!(
            socket,
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n{auth}\r\n"
        )
        .unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, headers.to_ascii_lowercase(), body.to_string())
    }
}

#[test]
fn ui_assets_are_public_but_api_still_requires_bearer() {
    let daemon = UiDaemon::start(true);
    let (status, headers, body) = daemon.request("GET", "/app.js", false);
    assert_eq!(status, 200);
    assert!(headers.contains("content-type: text/javascript"));
    assert_eq!(body, "console.log('asset');");
    for path in ["/v1", "/v1/runs"] {
        assert_eq!(daemon.request("GET", path, false).0, 401);
        assert_eq!(daemon.request("GET", path, true).0, 200);
    }
    let (status, headers, body) = daemon.request("GET", "/", false);
    assert_eq!(status, 200);
    assert!(headers.contains("cache-control: no-cache"));
    assert_eq!(body, "<html>SPA</html>");
    assert_eq!(daemon.request("HEAD", "/", false).2, "");
}

#[test]
fn ui_fallback_and_traversal_are_separate() {
    let daemon = UiDaemon::start(true);
    let (status, _, body) = daemon.request("GET", "/runs/example", false);
    assert_eq!(status, 200);
    assert_eq!(body, "<html>SPA</html>");
    assert_eq!(daemon.request("GET", "/missing.js", false).0, 404);
    for path in [
        "/../secret.txt",
        "/%2e%2e/secret.txt",
        "/%2fabsolute",
        "/C%3a/secret.txt",
        "/a%5csecret.txt",
    ] {
        let (status, _, body) = daemon.request("GET", path, false);
        assert_eq!(status, 400, "{path}: {body}");
        assert!(!body.contains("private"));
    }
}

#[test]
fn without_ui_all_requests_still_require_bearer() {
    let daemon = UiDaemon::start(false);
    assert_eq!(daemon.request("GET", "/", false).0, 401);
    assert_eq!(daemon.request("GET", "/app.js", false).0, 401);
    assert_eq!(daemon.request("GET", "/", true).0, 404);
}
