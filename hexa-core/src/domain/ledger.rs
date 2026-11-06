//! The task ledger and its lease rules (ADR-2611051850). Pure data and pure
//! judgement, no I/O: the ledger file, the lock and git are the adapters'.
//!
//! Sign conventions: a lease is a repo-relative path with `/` separators and
//! no trailing slash. It covers itself and everything below it, component by
//! component (`src/a` covers `src/a/x.rs`, not `src/ab`). Two leases overlap
//! when either covers the other.

use serde::{Deserialize, Serialize};

/// What went wrong, in the shape an agent can act on without guessing:
/// every field is always present in the JSON form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerError {
    /// Stable machine name: `lease_overlap`, `lease_not_allowed`,
    /// `lease_invalid`, `task_not_found`, `task_not_active`,
    /// `ledger_unreadable`.
    pub code: String,
    pub message: String,
    /// The next step, as a sentence a model can follow.
    pub hint: String,
    /// The argument that caused it (`lease`, `id`, ...), or `null`.
    pub field: Option<String>,
    /// Whether the same call may succeed later without a change of arguments.
    pub retryable: bool,
}

impl LedgerError {
    pub fn new(
        code: &str,
        message: String,
        hint: String,
        field: Option<&str>,
        retryable: bool,
    ) -> Self {
        LedgerError {
            code: code.into(),
            message,
            hint,
            field: field.map(str::to_string),
            retryable,
        }
    }
}

impl std::fmt::Display for LedgerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({}). {}", self.message, self.code, self.hint)
    }
}

impl std::error::Error for LedgerError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Claimed,
    Running,
    Done,
    Failed,
    Released,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            TaskStatus::Claimed => "claimed",
            TaskStatus::Running => "running",
            TaskStatus::Done => "done",
            TaskStatus::Failed => "failed",
            TaskStatus::Released => "released",
        })
    }
}

impl TaskStatus {
    /// A task that still holds its leases.
    pub fn is_active(self) -> bool {
        matches!(self, TaskStatus::Claimed | TaskStatus::Running)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub name: String,
    /// An agent label: a session id or the `agent_id` Claude Code gives a
    /// subagent. A session whose `agent_id` equals it is bound to this task.
    pub owner: Option<String>,
    pub worktree: Option<String>,
    pub gate: Option<String>,
    pub leases: Vec<String>,
    pub status: TaskStatus,
    /// The run record id (`~/.hexa/runs/<id>.json`).
    pub run: Option<String>,
    /// `passed|failed|killed` once reported.
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default)]
    pub commit: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    pub created: String,
    pub updated: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ledger {
    pub version: u32,
    pub tasks: Vec<Task>,
}

impl Default for Ledger {
    fn default() -> Self {
        Ledger {
            version: 1,
            tasks: Vec::new(),
        }
    }
}

impl Ledger {
    pub fn get(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    pub fn active(&self) -> impl Iterator<Item = &Task> {
        self.tasks.iter().filter(|t| t.status.is_active())
    }

    /// The active task registered with this owner label.
    pub fn active_for_owner(&self, owner: &str) -> Option<&Task> {
        self.active().find(|t| t.owner.as_deref() == Some(owner))
    }
}

// ── leases ───────────────────────────────────────────────────────────────────

/// A lease in canonical form: `./` and doubled slashes removed, no trailing
/// slash. Refuses what cannot be a repo-relative path.
pub fn normalize_lease(raw: &str) -> Result<String, LedgerError> {
    let bad = |why: &str| {
        LedgerError::new(
            "lease_invalid",
            format!("lease `{raw}`: {why}"),
            "Give a repo-relative file or directory such as `src/a/` or `src/a/x.rs`.".into(),
            Some("lease"),
            false,
        )
    };
    if raw.trim().is_empty() {
        return Err(bad("is empty"));
    }
    if raw.starts_with('/') || raw.contains('\\') {
        return Err(bad("must be repo-relative with `/` separators"));
    }
    let mut parts: Vec<&str> = Vec::new();
    for c in raw.trim().split('/') {
        match c {
            "" | "." => {}
            ".." => return Err(bad("`..` may leave the repository")),
            ".git" => return Err(bad("a path into .git is never leasable")),
            c => parts.push(c),
        }
    }
    if parts.is_empty() {
        return Err(bad("names no file"));
    }
    Ok(parts.join("/"))
}

/// Does `lease` cover `path`? Both canonical.
pub fn covers(lease: &str, path: &str) -> bool {
    path == lease
        || (path.len() > lease.len()
            && path.starts_with(lease)
            && path.as_bytes()[lease.len()] == b'/')
}

pub fn overlaps(a: &str, b: &str) -> bool {
    covers(a, b) || covers(b, a)
}

/// The first `(held lease, requested lease, holder task)` where a requested
/// lease overlaps a lease an active task holds.
pub fn find_overlap<'a>(
    ledger: &'a Ledger,
    requested: &'a [String],
) -> Option<(&'a str, &'a str, &'a Task)> {
    for t in ledger.active() {
        for held in &t.leases {
            for want in requested {
                if overlaps(held, want) {
                    return Some((held.as_str(), want.as_str(), t));
                }
            }
        }
    }
    None
}

// ── never leasable, never committed ──────────────────────────────────────────

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// A name that is a credential whatever it is called near: `.runner`, `.env`
/// and `.env.*` (not `.env.example`), `*.pem`, `*.key`, `*.p12`, `id_rsa*`,
/// `credentials*`, `.netrc`. A fixed built-in list (ADR-2611051850).
pub fn is_strict_credential_path(path: &str) -> bool {
    let n = basename(path).to_ascii_lowercase();
    if n == ".env.example" {
        return false;
    }
    n == ".runner"
        || n == ".env"
        || n.starts_with(".env.")
        || n.ends_with(".pem")
        || n.ends_with(".key")
        || n.ends_with(".p12")
        || n.starts_with("id_rsa")
        || n.starts_with("credentials")
        || n == ".netrc"
}

/// The strict list plus the loose name patterns `*token*` and `*secret*`.
/// The loose ones also match source files such as `tokenizer.rs`, so they
/// bind leases and neighbours, and never the `--file` an operator named.
pub fn is_credential_path(path: &str) -> bool {
    if is_strict_credential_path(path) {
        return true;
    }
    let n = basename(path).to_ascii_lowercase();
    n != ".env.example" && (n.contains("token") || n.contains("secret"))
}

/// Why `lease` may not be leased, or `None`. `sealed` is every path a spec in
/// `docs/specs/*.json` lists. A lease that is a sealed path, or that sits
/// inside a sealed directory entry, or names a credential, is refused.
pub fn never_leasable(lease: &str, sealed: &[String]) -> Option<LedgerError> {
    let why = if sealed.iter().any(|s| covers(s, lease)) {
        Some("is a sealed test (docs/specs lists it); a sealed file changes through its spec, never through a lease")
    } else if is_credential_path(lease) {
        Some("looks like a credential (.runner, .env*, *.pem, *.key, id_rsa*, *.p12, credentials*, .netrc, *token*, *secret*)")
    } else {
        None
    };
    why.map(|w| {
        LedgerError::new(
            "lease_not_allowed",
            format!("lease `{lease}` {w}"),
            "Lease the directory or source file the task edits instead; this path is never leasable.".into(),
            Some("lease"),
            false,
        )
    })
}

// ── the edit decision ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Allow,
    Deny(String),
}

/// May the session bound to `task` edit `rel` (a canonical repo-relative
/// path)? Outside the leases: no. A sealed or credential-looking path: no,
/// even inside a directory lease.
pub fn lease_decision(task: &Task, rel: &str, sealed: &[String]) -> Decision {
    if !task.status.is_active() {
        return Decision::Deny(format!(
            "task {} ({}) is {}, so it holds no lease. Register a task: `hexa agent register --task <name> --lease <path>`.",
            task.id,
            task.name,
            task.status
        ));
    }
    if sealed.iter().any(|s| covers(s, rel)) {
        return Decision::Deny(format!(
            "{rel} is a sealed test; it is never part of task {} ({}) and changes only through its spec",
            task.id, task.name
        ));
    }
    if is_credential_path(rel) {
        return Decision::Deny(format!(
            "{rel} looks like a credential; task {} ({}) may never edit it",
            task.id, task.name
        ));
    }
    if task.leases.iter().any(|l| covers(l, rel)) {
        return Decision::Allow;
    }
    Decision::Deny(format!(
        "{rel} is outside the lease of task {} ({}); leased: {}. Edit within the lease, or register a task that leases it.",
        task.id,
        task.name,
        task.leases.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(leases: &[&str]) -> Task {
        Task {
            id: "t1".into(),
            name: "one".into(),
            owner: None,
            worktree: None,
            gate: None,
            leases: leases.iter().map(|s| s.to_string()).collect(),
            status: TaskStatus::Claimed,
            run: None,
            outcome: None,
            commit: None,
            note: None,
            created: "x".into(),
            updated: "x".into(),
        }
    }

    #[test]
    fn a_lease_covers_by_component_not_by_prefix_of_the_name() {
        assert!(covers("src/a", "src/a"));
        assert!(covers("src/a", "src/a/x.rs"));
        assert!(!covers("src/a", "src/ab/x.rs"));
        assert!(!covers("src/a/x.rs", "src/a"));
    }

    // Independent oracle: overlap is "some path in a small universe lies under
    // both leases", computed by walking the universe, not by the prefix rule.
    #[test]
    fn overlap_agrees_with_a_brute_force_walk_of_a_small_tree_and_is_symmetric() {
        let universe: Vec<String> = {
            let mut u = Vec::new();
            for a in ["src", "tests"] {
                u.push(a.to_string());
                for b in ["a", "ab", "b"] {
                    u.push(format!("{a}/{b}"));
                    for c in ["x.rs", "y.rs"] {
                        u.push(format!("{a}/{b}/{c}"));
                    }
                }
            }
            u
        };
        let under = |lease: &str| -> Vec<&String> {
            universe
                .iter()
                .filter(|p| {
                    let lp: Vec<&str> = lease.split('/').collect();
                    let pp: Vec<&str> = p.split('/').collect();
                    pp.len() >= lp.len() && pp[..lp.len()] == lp[..]
                })
                .collect()
        };
        for a in &universe {
            for b in &universe {
                let oracle = under(a).iter().any(|p| under(b).contains(p));
                assert_eq!(overlaps(a, b), oracle, "{a} vs {b}");
                assert_eq!(overlaps(a, b), overlaps(b, a), "{a} vs {b} not symmetric");
            }
        }
    }

    #[test]
    fn leases_are_normalised_and_the_bad_ones_refused_with_a_field() {
        assert_eq!(normalize_lease("./src//a/").unwrap(), "src/a");
        for bad in [
            "",
            "  ",
            "/abs",
            "../x",
            "a/../b",
            ".git/config",
            ".",
            "a\\b",
        ] {
            let e = normalize_lease(bad).unwrap_err();
            assert_eq!(
                (e.code.as_str(), e.field.as_deref(), e.retryable),
                ("lease_invalid", Some("lease"), false),
                "{bad}"
            );
            assert!(!e.hint.is_empty());
        }
    }

    #[test]
    fn the_credential_list_matches_what_the_adr_names_and_not_look_alikes() {
        for yes in [
            ".runner",
            "src/a/.runner",
            ".env",
            ".env.local",
            "a/b.pem",
            "k.key",
            "id_rsa",
            "id_rsa.pub",
            "x.p12",
            "credentials.json",
            ".netrc",
            "api_token.txt",
            "my-secret.yaml",
            "A/.ENV",
        ] {
            assert!(is_credential_path(yes), "{yes} must be credential-looking");
        }
        for no in [
            ".env.example",
            "src/a/x.rs",
            "environment.rs",
            "keyboard.rs",
            "src/tokens/mod.rs",
            "runner.rs",
            "pem.rs",
        ] {
            assert!(
                !is_credential_path(no),
                "{no} must not be credential-looking"
            );
        }
        // The loose name patterns never bind a strict check.
        assert!(is_credential_path("tokenizer.rs") && !is_strict_credential_path("tokenizer.rs"));
    }

    #[test]
    fn a_sealed_path_and_a_credential_are_never_leasable() {
        let sealed = vec!["tests/sealed_spec.rs".to_string()];
        for l in ["tests/sealed_spec.rs", ".runner", "a/.env", "x.pem"] {
            let e = never_leasable(l, &sealed).unwrap_or_else(|| panic!("{l} was leasable"));
            assert_eq!((e.code.as_str(), e.retryable), ("lease_not_allowed", false));
            assert_eq!(e.field.as_deref(), Some("lease"));
        }
        assert!(never_leasable("tests/other.rs", &sealed).is_none());
        assert!(never_leasable("src/a", &sealed).is_none());
    }

    #[test]
    fn the_decision_allows_inside_and_refuses_outside_sealed_credential_and_inactive() {
        let sealed = vec!["tests/sealed_spec.rs".to_string()];
        let t = task(&["src/a", "tests"]);
        assert_eq!(lease_decision(&t, "src/a/x.rs", &sealed), Decision::Allow);
        assert_eq!(lease_decision(&t, "tests/new.rs", &sealed), Decision::Allow);
        for no in [
            "src/b/y.rs",
            "tests/sealed_spec.rs",
            "src/a/.env",
            "docs/x.md",
        ] {
            assert!(
                matches!(lease_decision(&t, no, &sealed), Decision::Deny(m) if m.contains("t1")),
                "{no}"
            );
        }
        let mut done = task(&["src/a"]);
        done.status = TaskStatus::Done;
        assert!(matches!(
            lease_decision(&done, "src/a/x.rs", &sealed),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn overlap_is_found_only_against_active_tasks() {
        let mut l = Ledger::default();
        let mut a = task(&["src/a"]);
        a.id = "a".into();
        l.tasks.push(a);
        let want = vec!["src/a/x.rs".to_string()];
        assert_eq!(
            find_overlap(&l, &want).map(|(_, _, t)| t.id.as_str()),
            Some("a")
        );
        l.tasks[0].status = TaskStatus::Done;
        assert!(
            find_overlap(&l, &want).is_none(),
            "a finished task holds no lease"
        );
    }

    #[test]
    fn the_ledger_round_trips_with_lowercase_status() {
        let mut l = Ledger::default();
        l.tasks.push(task(&["src/a"]));
        let s = serde_json::to_string(&l).unwrap();
        assert!(s.contains("\"status\":\"claimed\""), "{s}");
        assert_eq!(serde_json::from_str::<Ledger>(&s).unwrap(), l);
    }
}
