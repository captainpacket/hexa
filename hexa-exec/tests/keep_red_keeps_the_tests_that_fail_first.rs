//! When some generated tests already pass before the code, the
//! ones that fail first are kept and the passing ones are reported as guards.
//!
//! Refusing the whole generation for a green test threw away every test that
//! did fail first. Keeping the red ones is the default for red at HEAD
//! (ADR-2611161600 §3); with an earlier red commit a green test still
//! refuses it (ADR-2610281442 §3).
//! The helpers are copied from a_spec_is_drafted_and_its_tests_generated_red.rs,
//! which is sealed and cannot be extended.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use hexa_core::domain::messages::{ContentBlock, StopReason};
use hexa_core::ports::inference::{InferenceError, InferenceRequest, InferenceResponse};
use hexa_core::ports::spec::Spec;

const MARKER: &str = "IMPLEMENTATION-MARKER-7f3a";

struct Double {
    answers: Mutex<Vec<String>>,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Double {
    fn new(answers: &[String]) -> (Self, Arc<Mutex<Vec<String>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut a: Vec<String> = answers.to_vec();
        a.reverse();
        (Self { answers: Mutex::new(a), seen: seen.clone() }, seen)
    }
}

#[async_trait]
impl hexa_infer::ports::Backends for Double {
    async fn complete(&self, request: InferenceRequest) -> Result<InferenceResponse, InferenceError> {
        assert!(request.tools.is_empty(), "the spec author and test writer have no tools");
        let mut text = request.system_prompt.clone();
        for m in &request.messages {
            for c in &m.content {
                if let ContentBlock::Text { text: t } = c {
                    text.push('\n');
                    text.push_str(t);
                }
            }
        }
        self.seen.lock().unwrap().push(text);
        let answer = self.answers.lock().unwrap().pop().expect("an answer for every call");
        Ok(InferenceResponse {
            content: vec![ContentBlock::Text { text: answer }],
            model_used: request.model,
            stop_reason: StopReason::EndTurn,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            latency_ms: 0,
            telemetry: std::collections::HashMap::new(),
        })
    }

    fn record_spend(&self, _model: &str, _i: u64, _o: u64) {}
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn id() -> String {
    format!("ADR-{}", "2701010000")
}

/// A repository holding an ADR that names its port and its use case, a
/// crate whose port is a stub, and a use case carrying a marker.
fn repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path().to_path_buf();
    std::fs::create_dir_all(r.join("docs/adrs")).unwrap();
    std::fs::create_dir_all(r.join("src")).unwrap();
    std::fs::write(
        r.join("docs/adrs").join(format!("{}-it-doubles.md", id())),
        format!(
            "# {}: It doubles\n\n**Status:** Accepted\n\n## Decision\n\n\
             1. **`double` returns twice its input.** The port is `src/ports.rs`.\n\
             2. **`double` never returns its input unchanged, except at zero.** The use case is `src/usecase.rs`.\n\n\
             ## References\n\nnone\n",
            id()
        ),
    )
    .unwrap();
    std::fs::write(r.join("Cargo.toml"), "[package]\nname = \"fx\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[workspace]\n").unwrap();
    std::fs::write(r.join("src/lib.rs"), "pub mod ports;\npub mod usecase;\npub use ports::double;\n").unwrap();
    std::fs::write(r.join("src/ports.rs"), "/// Twice `n`.\npub fn double(n: u32) -> u32 {\n    let _ = n;\n    todo!(\"the decision is not built yet\")\n}\n").unwrap();
    std::fs::write(r.join("src/usecase.rs"), format!("// {MARKER}\npub fn helper() -> u32 {{ 7 }}\n")).unwrap();
    std::fs::write(r.join(".gitignore"), "target/\n").unwrap();
    git(&r, &["init", "-q"]);
    git(&r, &["add", "."]);
    git(&r, &["commit", "-qm", "init"]);
    (tmp, r)
}

fn scenario(sid: &str, d: &str, kind: &str) -> String {
    format!(r#"{{"id":"{sid}","decision":["{d}"],"kind":"{kind}","description":"x","given":"g","when":"w","then":"t"}}"#)
}

fn drafted(both_negatives: bool) -> String {
    let mut s = vec![scenario("S1", "§1", "example"), scenario("S2", "§1", "negative"), scenario("S3", "§2", "example")];
    if both_negatives {
        s.push(scenario("S4", "§2", "negative"));
    }
    format!("```json\n{{\"adr\":\"{}\",\"feature\":\"it doubles\",\"specs\":[{}]}}\n```", id(), s.join(","))
}

/// The drafted spec, accepted as a person would.
fn accepted(r: &Path) -> Spec {
    let text = drafted(true);
    let json = text.trim_start_matches("```json\n").trim_end_matches("\n```");
    let spec: Spec = serde_json::from_str(json).unwrap();
    { let _ = r; hexa_exec::spec_author::accept(spec, "a reviewer") }
}

const TESTS_RED: &str = "```rust\n\
/// S1: two doubles to four\n#[test]\nfn two_doubles_to_four() { assert_eq!(fx::double(2), 4); }\n\n\
/// S2: one does not stay one\n#[test]\nfn one_does_not_stay_one() { assert_ne!(fx::double(1), 1); }\n\n\
/// S3: three doubles to six\n#[test]\nfn three_doubles_to_six() { assert_eq!(fx::double(3), 6); }\n\n\
/// S4: five does not stay five\n#[test]\nfn five_does_not_stay_five() { assert_ne!(fx::double(5), 5); }\n```";


/// No option set: red at HEAD, the default.
fn options() -> hexa_exec::spec_author::TestOptions {
    hexa_exec::spec_author::TestOptions::default()
}

#[tokio::test]
async fn keep_red_leaves_a_green_test_out_as_a_guard_and_commits_the_red_ones() {
    let (_t, r) = repo();
    let spec = accepted(&r);
    let guard = TESTS_RED.replace("assert_ne!(fx::double(1), 1)", "let _ = 1");
    let (double, _) = Double::new(&[guard]);
    let path_env = std::env::var("PATH").unwrap_or_default();
    let head = git(&r, &["rev-parse", "HEAD"]);
    let g = hexa_exec::spec_author::generate_tests_with(&double, "a-local-model", &r, &spec, Path::new("."), &path_env, &options())
        .await
        .expect("the failing tests are kept");
    assert_eq!(g.guards.len(), 1, "{:?}", g.guards);
    assert!(g.guards[0].contains("S2") && g.guards[0].contains("passes"), "the guard is reported: {:?}", g.guards);
    assert!(!g.refs.contains_key("S2"), "the guard's scenario stays unbound: {:?}", g.refs);
    assert_eq!(g.refs.len(), 3, "{:?}", g.refs);
    let body = std::fs::read_to_string(r.join(&g.file)).unwrap();
    assert!(!body.contains("one_does_not_stay_one"), "the green test is not sealed:\n{body}");
    assert!(body.contains("two_doubles_to_four") && body.contains("five_does_not_stay_five"), "{body}");
    assert_ne!(git(&r, &["rev-parse", "HEAD"]), head, "the red tests are committed");
    assert_eq!(g.red, git(&r, &["rev-parse", "--short", "HEAD"]));
}

#[tokio::test]
async fn keep_red_still_refuses_a_generation_whose_every_test_is_green() {
    let (_t, r) = repo();
    let spec = accepted(&r);
    let vacuous = TESTS_RED
        .replace("assert_eq!(fx::double(2), 4)", "let _ = 2")
        .replace("assert_ne!(fx::double(1), 1)", "let _ = 1")
        .replace("assert_eq!(fx::double(3), 6)", "let _ = 3")
        .replace("assert_ne!(fx::double(5), 5)", "let _ = 5");
    let (double, _) = Double::new(&[vacuous]);
    let path_env = std::env::var("PATH").unwrap_or_default();
    let head = git(&r, &["rev-parse", "HEAD"]);
    let err = hexa_exec::spec_author::generate_tests_with(&double, "a-local-model", &r, &spec, Path::new("."), &path_env, &options())
        .await
        .unwrap_err();
    assert!(err.contains("passes") && err.contains("nothing kept"), "a file of guards tests nothing: {err}");
    assert!(!r.join("tests/adr_2701010000_spec.rs").exists(), "nothing is left in the tree");
    assert_eq!(git(&r, &["rev-parse", "HEAD"]), head, "nothing is committed");
}

#[tokio::test]
async fn a_green_test_still_refuses_the_generation_when_red_is_proved_at_a_named_commit() {
    let (_t, r) = repo();
    let spec = accepted(&r);
    let guard = TESTS_RED.replace("assert_ne!(fx::double(1), 1)", "let _ = 1");
    let (double, _) = Double::new(&[guard]);
    let path_env = std::env::var("PATH").unwrap_or_default();
    let head = git(&r, &["rev-parse", "HEAD"]);
    let opts = hexa_exec::spec_author::TestOptions { red_at: Some(head.clone()), ..Default::default() };
    let err = hexa_exec::spec_author::generate_tests_with(&double, "a-local-model", &r, &spec, Path::new("."), &path_env, &opts)
        .await
        .unwrap_err();
    assert!(err.contains("passes"), "a green test refuses a generation proved at an earlier commit: {err}");
    assert!(!r.join("tests/adr_2701010000_spec.rs").exists(), "nothing is left in the tree");
    assert_eq!(git(&r, &["rev-parse", "HEAD"]), head, "nothing is committed");
}
