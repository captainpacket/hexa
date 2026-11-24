//! `hexa do run --fast` puts the file back when its evidence never passes
//! (git.local gary/hexa#18).
//!
//! The single-shot loop applied each attempt's edit on top of the last one and
//! returned without restoring: one run that never passed appended the same
//! block to a systemd unit three times and left it in the tree. The ReAct and
//! frontier paths already restored on failure; this one did not.
//!
//! Spawned as a child process so `HOME` can point at a registry naming the
//! model double, without touching this process's environment.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;

/// A local OpenAI-compatible endpoint answering every request with `content`.
fn model_double(content: &str) -> String {
    let body = format!(
        r#"{{"id":"c","object":"chat.completion","model":"m","choices":[{{"index":0,"message":{{"role":"assistant","content":{}}},"finish_reason":"stop"}}],"usage":{{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}}}"#,
        serde_json::to_string(content).unwrap()
    );
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let mut len = 0usize;
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
                    break;
                }
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            if len > 0 {
                let mut buf = vec![0u8; len];
                let _ = reader.read_exact(&mut buf);
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    url
}

const ORIGINAL: &str = "[Service]\nExecStart=/usr/bin/true\n";

/// A committed project whose one file is `deploy/unit.service`, with a
/// registry in `home/` pointing every tier at the double.
fn project(dir: &Path, model_url: &str) {
    std::fs::create_dir_all(dir.join(".hexa")).unwrap();
    std::fs::create_dir_all(dir.join("deploy")).unwrap();
    std::fs::create_dir_all(dir.join("home/.hexa")).unwrap();
    std::fs::write(dir.join("deploy/unit.service"), ORIGINAL).unwrap();
    std::fs::write(
        dir.join(".hexa/project.json"),
        r#"{"name":"fast","inference":{"tier_models":{"t1":"double-model","t2":"double-model","t2.5":"double-model"}}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("home/.hexa/inference-servers.json"),
        format!(
            r#"{{"endpoints":[{{"id":"double","url":"{model_url}","provider":"openai-compat","model":"double-model","models":"[\"double-model\"]","apiKeyRef":"","requiresAuth":false,"status":"healthy"}}]}}"#
        ),
    )
    .unwrap();
    std::fs::write(dir.join(".gitignore"), "home/\n").unwrap();
    let git = |args: &[&str]| {
        assert!(Command::new("git").args(args).current_dir(dir).status().expect("git").success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "fast@test"]);
    git(&["config", "user.name", "fast"]);
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "fixture"]);
}

#[test]
fn a_fast_run_whose_evidence_never_passes_leaves_the_file_as_it_found_it() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path();
    // Every attempt appends the same key, as in the report.
    let url = model_double("MODE: append\n```\nRestart=always\n```\n");
    project(dir, &url);

    // A counted failure: the evidence ran one test and it failed.
    let evidence = "echo 'test result: FAILED. 0 passed; 1 failed; 0 ignored' && false";
    let out = Command::new(env!("CARGO_BIN_EXE_hexa"))
        .args(["do", "run", "set Restart=always", "--file", "deploy/unit.service", "--evidence", evidence, "--fast"])
        .current_dir(dir)
        .env("HOME", dir.join("home"))
        .output()
        .expect("run hexa");
    let shown = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));

    assert!(!out.status.success(), "a run that never passed must not succeed:\n{shown}");
    let now = std::fs::read_to_string(dir.join("deploy/unit.service")).unwrap();
    assert_eq!(now, ORIGINAL, "the file must be as the run found it, not carry the failed edits:\n{shown}");
    let log = Command::new("git").args(["log", "--oneline"]).current_dir(dir).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&log.stdout).lines().count(), 1, "nothing committed:\n{shown}");
}
