//! Der MCP-Server als echter Prozess: JSON-RPC über stdin/stdout.

mod common;

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

const EXE: &str = env!("CARGO_BIN_EXE_osulzer-imageoptimizer");

struct Server {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

impl Server {
    fn start(config: &std::path::Path) -> Server {
        let mut child = Command::new(EXE)
            .arg("mcp")
            .env("OSULZER_IMAGEOPTIMIZER_CONFIG", config)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut s = Server { child, stdin, lines };
        let init = s.request(1, "initialize", json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "1" }
        }));
        assert!(init["result"]["serverInfo"].is_object(), "{init}");
        s.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        s
    }

    fn send(&mut self, msg: Value) {
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let line = self.lines.recv_timeout(Duration::from_secs(60)).expect("Antwort vom Server");
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == id {
                return v;
            }
        }
    }

    fn call(&mut self, id: u64, tool: &str, args: Value) -> Value {
        let v = self.request(id, "tools/call", json!({ "name": tool, "arguments": args }));
        let text = v["result"]["content"][0]["text"].as_str().unwrap_or_else(|| panic!("{v}")).to_string();
        serde_json::from_str(&text).unwrap_or(Value::String(text))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[test]
fn lists_the_four_tools() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Server::start(&dir.path().join("c.json"));
    let v = s.request(2, "tools/list", json!({}));
    let mut names: Vec<_> = v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    names.sort();
    assert_eq!(names, ["describe_image", "image_info", "list_presets", "optimize_images"]);
}

#[test]
fn optimize_images_with_texts_from_the_agent() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg(dir.path(), "IMG_1.jpg", 1600, 1200);
    // Deutsch festlegen: Umlaute werden je nach Sprache zu „ue“ oder „u“.
    std::fs::write(dir.path().join("c.json"), r#"{"ui_lang":"de"}"#).unwrap();
    let mut s = Server::start(&dir.path().join("c.json"));
    let reports = s.call(2, "optimize_images", json!({
        "paths": [src.to_str().unwrap()],
        "preset": "web-medium",
        "texts": [{ "path": src.to_str().unwrap(), "file_name": "Grüner Ofen", "alt": "Ein grüner Kachelofen" }]
    }));
    let out = reports[0]["output"].as_str().unwrap_or_else(|| panic!("{reports}"));
    assert!(out.ends_with("gruener-ofen.jpg"), "{out}");
    let bytes = std::fs::read(out).unwrap();
    let alt = "Ein grüner Kachelofen".as_bytes();
    assert!(bytes.windows(alt.len()).any(|w| w == alt));
}

#[test]
fn relative_path_is_an_error_and_server_keeps_running() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Server::start(&dir.path().join("c.json"));
    let reports = s.call(2, "optimize_images", json!({ "paths": ["relativ.jpg"] }));
    assert!(reports[0]["error"].is_string(), "{reports}");
    let v = s.request(3, "tools/list", json!({}));
    assert!(v["result"]["tools"].is_array());
}

#[test]
fn presets_and_info() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg(dir.path(), "a.jpg", 320, 240);
    let mut s = Server::start(&dir.path().join("c.json"));
    let presets = s.call(2, "list_presets", json!({}));
    assert!(presets.to_string().contains("web-large"));
    let info = s.call(3, "image_info", json!({ "paths": [src.to_str().unwrap()] }));
    assert_eq!(info[0]["width"], 320);
}
