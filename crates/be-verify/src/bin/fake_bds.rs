//! `fake_bds` — a stand-in Bedrock Dedicated Server for tests.
//!
//! Reads commands from stdin; for each line that starts with a configured match
//! prefix, prints the configured response lines followed by a sentinel. This lets
//! tests exercise the harness framing and the `/locate` parser without a real BDS.
//!
//! Usage:
//!   fake_bds --script <script.json> [--sentinel <line>]
//!
//! Script format (JSON):
//! ```json
//! {
//!   "responses": [
//!     { "match_prefix": "/locate structure village",
//!       "lines": ["Structure found: Village at 1234, 64, -5678 (in 1234 blocks)"] }
//!   ],
//!   "default_lines": ["Could not find that structure"]
//! }
//! ```
//!
//! The sentinel defaults to `__BDS_RESPONSE_END__` and is printed after the matched
//! (or default) lines. The command "stop" terminates the process.

use std::io::{BufRead, Write};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Script {
    #[serde(default)]
    responses: Vec<Response>,
    #[serde(default)]
    default_lines: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Response {
    match_prefix: String,
    lines: Vec<String>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut script_path: Option<String> = None;
    let mut sentinel = "__BDS_RESPONSE_END__".to_string();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--script" => {
                i += 1;
                script_path = Some(args[i].clone());
            }
            "--sentinel" => {
                i += 1;
                sentinel = args[i].clone();
            }
            _ => {}
        }
        i += 1;
    }
    let script: Script = {
        let path = script_path.unwrap_or_else(|| {
            eprintln!("missing --script");
            std::process::exit(2);
        });
        let text = std::fs::read_to_string(&path).expect("read script");
        serde_json::from_str(&text).expect("parse script")
    };

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("stop") {
            break;
        }
        let response_lines: Vec<String> = script
            .responses
            .iter()
            .find(|r| trimmed.starts_with(r.match_prefix.as_str()))
            .map(|r| r.lines.clone())
            .unwrap_or_else(|| script.default_lines.clone());

        for l in &response_lines {
            let _ = writeln!(stdout, "{l}");
        }
        let _ = writeln!(stdout, "{sentinel}");
        let _ = stdout.flush();
    }
}
