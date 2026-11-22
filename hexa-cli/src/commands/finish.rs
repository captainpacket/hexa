//! `hexa finish`: the open work, each item with the command that closes it
//! (ADR-2611161800 §1). Read-only.
//!
//! A plan ends on main: its tasks done, its ADR closed, everything landed.
//! This is the list of what stands between the repository and that end. The
//! Stop hook asks the same question for one session (`session_open`), and
//! `loop stage done` refuses while the answer is not empty.

use clap::Args;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Args, Debug)]
pub struct FinishArgs {
    /// Print the list as JSON
    #[arg(long)]
    pub json: bool,
    /// Exit 1 when anything is open
    #[arg(long)]
    pub exit_code: bool,
}

/// One piece of open work: what kind, what exactly, and the command that closes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub kind: &'static str,
    pub what: String,
    pub fix: String,
    /// What stays the same while the work is the same (ADR-2611220204 §1):
    /// the text is what is printed, the key is what is compared.
    pub key: String,
}

impl Item {
    fn json(&self) -> Value {
        json!({ "kind": self.kind, "what": self.what, "fix": self.fix, "key": self.key })
    }
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let o = Command::new("git").args(args).current_dir(dir).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim_end().to_string())
}

/// The branch work lands on: origin's default branch, else `main`, else `master`.
pub fn base_branch(dir: &Path) -> String {
    if let Some(head) = git(dir, &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"]) {
        if let Some(b) = head.strip_prefix("refs/remotes/origin/") {
            return b.to_string();
        }
    }
    for b in ["main", "master"] {
        if git(dir, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{b}")]).is_some() {
            return b.to_string();
        }
    }
    "main".to_string()
}

/// hexa's own state files change as it runs; they are not work to commit.
fn is_state(path: &str) -> bool {
    path.starts_with(".hexa/") || path == ".hexa"
}

/// Tracked files with changes not committed, as paths relative to `dir`.
fn uncommitted_files(dir: &Path) -> Vec<String> {
    let Some(out) = git(dir, &["status", "--porcelain=v1", "--untracked-files=no"]) else { return Vec::new() };
    out.lines()
        .filter(|l| l.len() > 3)
        .map(|l| {
            let p = &l[3..];
            p.rsplit_once(" -> ").map(|(_, to)| to).unwrap_or(p).trim_matches('"').to_string()
        })
        .filter(|p| !is_state(p))
        .collect()
}

fn gate_or_placeholder(dir: &Path) -> String {
    crate::commands::loop_cmd::full_gate(dir).unwrap_or_else(|| "<the test command>".to_string())
}

/// Files not yet tracked, as paths relative to `dir`, leaving out what git ignores.
fn untracked_files(dir: &Path) -> Vec<String> {
    let Some(out) = git(dir, &["ls-files", "--others", "--exclude-standard"]) else { return Vec::new() };
    out.lines().map(|l| l.trim_matches('"').to_string()).filter(|p| !p.is_empty() && !is_state(p)).collect()
}

/// One item per file (ADR-2611220204 §1): its key is its path, so a second
/// file changed does not make the first one new work.
fn uncommitted_items(dir: &Path, files: &[String]) -> Vec<Item> {
    let gate = gate_or_placeholder(dir);
    files
        .iter()
        .map(|f| Item {
            kind: "uncommitted",
            what: format!("{f} changed and not committed"),
            fix: format!("commit it: hexa do gate --files {f} -e '{gate}' -m '<what changed>'"),
            key: format!("uncommitted:{f}"),
        })
        .collect()
}

fn unpushed_item(dir: &Path) -> Option<Item> {
    let upstream = git(dir, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])?;
    let n: usize = git(dir, &["rev-list", "--count", "@{u}..HEAD"])?.parse().ok()?;
    (n > 0).then(|| Item {
        kind: "unpushed",
        what: format!("{n} commit{} not on {upstream}", if n == 1 { "" } else { "s" }),
        fix: "push it: git push".to_string(),
        key: format!("unpushed:{upstream}"),
    })
}

/// `(path, branch)` of every worktree of this repository, the current one included.
fn worktrees(dir: &Path) -> Vec<(PathBuf, Option<String>)> {
    let Some(listing) = git(dir, &["worktree", "list", "--porcelain"]) else { return Vec::new() };
    let mut out = Vec::new();
    let mut path: Option<PathBuf> = None;
    for line in listing.lines().chain(std::iter::once("")) {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(p));
        } else if line.is_empty() {
            if let Some(p) = path.take() {
                out.push((p, None));
            }
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            if let Some(p) = path.take() {
                out.push((p, Some(b.to_string())));
            }
        }
    }
    out
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// The command that fast-forwards `base` to `branch`: `git merge --ff-only`
/// from the worktree that has `base` checked out, or, when none has, `git
/// fetch . <branch>:<base>`, which moves the ref only on a fast-forward and
/// needs no checkout. A merge run in a checkout of some other branch would do
/// nothing and leave the branch unlanded.
pub fn fast_forward(dir: &Path, branch: &str, base: &str) -> String {
    match worktrees(dir).into_iter().find(|(_, b)| b.as_deref() == Some(base)) {
        Some((p, _)) => format!("git -C {} merge --ff-only {branch}", p.display()),
        None => format!("git -C {} fetch . {branch}:{base}", dir.display()),
    }
}

/// The words before [`fast_forward`]'s command: where it runs. Kept outside
/// the command, so the command can be pasted as it is.
fn fast_forward_lead(dir: &Path, base: &str) -> String {
    if worktrees(dir).iter().any(|(_, b)| b.as_deref() == Some(base)) {
        format!("from {base}'s checkout")
    } else {
        format!("({base} is checked out nowhere; this moves it only on a fast-forward)")
    }
}

/// How to land `branch` (checked out at `path`) on `base` so its commits stay
/// its commits (ADR-2611220204 §5): a fast-forward from the base's checkout
/// when the base is an ancestor of the branch; otherwise merge the base into
/// the branch, re-run the gate, then fast-forward.
pub fn landing_hint(dir: &Path, branch: &str, base: &str, path: &Path) -> String {
    let ff = fast_forward(dir, branch, base);
    let lead = fast_forward_lead(dir, base);
    if git(dir, &["merge-base", "--is-ancestor", base, branch]).is_some() {
        format!("land it {lead}: {ff}")
    } else {
        format!(
            "{base} has moved: merge {base} into {branch} (git -C {} merge {base}), re-run the gate ({}), then fast-forward {lead}: {ff}",
            path.display(),
            gate_or_placeholder(dir)
        )
    }
}

/// Branches with commits not on the base, and branches already on it whose
/// worktree is clean and could go. `only` limits it to one worktree.
fn branch_items(dir: &Path, base: &str, only: Option<&Path>) -> Vec<Item> {
    let mut out = Vec::new();
    let mut landed = Vec::new();
    for (path, branch) in worktrees(dir) {
        let Some(branch) = branch else { continue };
        if branch == base || only.is_some_and(|o| !same_dir(o, &path)) {
            continue;
        }
        let ahead: usize = git(dir, &["rev-list", "--count", &format!("{base}..{branch}")]).and_then(|n| n.parse().ok()).unwrap_or(0);
        let shown = path.display().to_string();
        if ahead > 0 {
            let tip = git(dir, &["rev-parse", &format!("refs/heads/{branch}")]).unwrap_or_default();
            let short: String = tip.chars().take(7).collect();
            out.push(Item {
                kind: "unlanded",
                what: format!("{branch} at {short}: {ahead} commit{} not on {base} ({shown})", if ahead == 1 { "" } else { "s" }),
                fix: landing_hint(dir, &branch, base, &path),
                key: format!("unlanded:{branch}@{tip}"),
            });
        } else if only.is_none() && path.is_dir() && uncommitted_files(&path).is_empty() {
            landed.push((branch.clone(), format!("{branch} ({shown})")));
        }
    }
    if !landed.is_empty() {
        let names: Vec<&str> = landed.iter().map(|(b, _)| b.as_str()).collect();
        let shown: Vec<&str> = landed.iter().map(|(_, s)| s.as_str()).collect();
        out.push(Item {
            kind: "landed",
            what: format!("{} worktree{} already on {base} and clean: {}", landed.len(), if landed.len() == 1 { "" } else { "s" }, shown.join(", ")),
            fix: "remove them: hexa dev worktree cleanup --force".to_string(),
            key: format!("landed:{}", names.join(",")),
        });
    }
    out
}

/// Workplans that were never planned: drafted from a prompt and left, or
/// approved while still empty (ADR-2611161800 §6).
fn draft_items(dir: &Path) -> Vec<Item> {
    let mut out = Vec::new();
    let unplanned = |p: &Path| -> Option<String> {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()?;
        let empty = ["steps", "tasks", "phases"].iter().all(|k| v.get(*k).and_then(|x| x.as_array()).is_none_or(|a| a.is_empty()));
        (v.get("status").and_then(|s| s.as_str()) == Some("pending-planner") && empty)
            .then(|| v.get("id").and_then(|s| s.as_str()).map(String::from).unwrap_or_else(|| p.file_stem().unwrap_or_default().to_string_lossy().to_string()))
    };
    let list = |sub: &Path| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(sub).map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect()).unwrap_or_default();
        v.sort();
        v
    };
    let drafts: Vec<String> = list(&dir.join("docs/workplans/drafts")).iter().filter_map(|p| unplanned(p)).collect();
    if !drafts.is_empty() {
        out.push(Item {
            kind: "draft",
            what: format!("{} draft workplan{} never planned: {}", drafts.len(), if drafts.len() == 1 { "" } else { "s" }, drafts.join(", ")),
            fix: "discard them: hexa plan drafts clear".to_string(),
            key: format!("draft:{}", drafts.join(",")),
        });
    }
    let approved: Vec<String> = list(&dir.join("docs/workplans"))
        .iter()
        .filter(|p| unplanned(p).is_some())
        .map(|p| p.strip_prefix(dir).unwrap_or(p).display().to_string())
        .collect();
    if !approved.is_empty() {
        out.push(Item {
            kind: "draft",
            what: format!("{} workplan{} approved with no steps: {}", approved.len(), if approved.len() == 1 { "" } else { "s" }, approved.join(", ")),
            fix: format!("remove them: git rm {}", approved.join(" ")),
            key: format!("draft:{}", approved.join(",")),
        });
    }
    out
}

/// The key of one checklist step (ADR-2611220204 §1): its title, or which
/// close-out step it is. Never its position or its marker.
pub fn task_key(task: &Value) -> String {
    let title = task.get("title").and_then(|v| v.as_str()).unwrap_or("");
    match task.get("closeout").and_then(|v| v.as_str()) {
        Some("land") => "closeout:land".to_string(),
        Some("close") => format!("closeout:close:{}", task.get("adr").and_then(|v| v.as_str()).unwrap_or("")),
        _ => format!("task:{title}"),
    }
}

fn task_items(entry: &Value) -> Vec<Item> {
    let raw: Vec<Value> = entry.get("tasks").and_then(|t| t.as_array()).cloned().unwrap_or_default();
    crate::commands::loop_cmd::entry_tasks(entry)
        .into_iter()
        .filter(|(_, _, st)| st != "done")
        .map(|(n, title, st)| Item {
            kind: "task",
            what: format!("[{}] {n} {title}", if st == "doing" { ">" } else { " " }),
            fix: format!("finish it, then: hexa loop task done {n}"),
            key: raw.get(n - 1).map(task_key).unwrap_or_else(|| format!("task:{title}")),
        })
        .collect()
}

/// The `**Status:**` of an ADR file, first word, lowercased.
pub fn adr_status(dir: &Path, id: &str) -> Option<String> {
    let p = crate::commands::loop_cmd::adr_path(dir, id)?;
    let text = std::fs::read_to_string(p).ok()?;
    text.lines().find_map(|l| {
        let l = l.trim();
        l.strip_prefix("**Status:**").or_else(|| l.strip_prefix("Status:")).map(|v| v.trim().trim_end_matches('.').split_whitespace().next().unwrap_or("").to_lowercase())
    })
}

fn adr_item(dir: &Path, entry: &Value) -> Option<Item> {
    let id = entry.get("adr").and_then(|v| v.as_str()).filter(|a| !a.is_empty() && *a != "none")?;
    let status = adr_status(dir, id)?;
    (!matches!(status.as_str(), "completed" | "superseded" | "rejected" | "deprecated")).then(|| Item {
        kind: "adr",
        what: format!("{id} is {status}: the work it decided is not closed out"),
        fix: "close it out, once everything above is done: hexa loop stage done".to_string(),
        key: format!("adr:{id}"),
    })
}

/// The loop entry `finish` reads: this session's, else the most recent ended
/// session's, which is where work that was walked away from is recorded.
fn loop_entry(dir: &Path) -> Option<Value> {
    let id = crate::commands::loop_cmd::session_id();
    crate::commands::loop_cmd::session_entry(dir, &id)
        .filter(|e| e.get("tasks").and_then(|t| t.as_array()).is_some_and(|a| !a.is_empty()) || e.get("adr").and_then(|a| a.as_str()).is_some())
        .or_else(|| crate::commands::loop_cmd::previous(dir).map(|(_, e)| e))
}

/// Everything open in the repository at `dir`.
pub fn open_work(dir: &Path) -> Vec<Item> {
    let base = base_branch(dir);
    let mut out = Vec::new();
    out.extend(uncommitted_items(dir, &uncommitted_files(dir)));
    out.extend(unpushed_item(dir));
    let entry = loop_entry(dir).unwrap_or(Value::Null);
    out.extend(task_items(&entry));
    out.extend(branch_items(dir, &base, None));
    out.extend(draft_items(dir));
    out.extend(adr_item(dir, &entry));
    out
}

/// What stands between this work and main, apart from its checklist: files
/// not committed and branches not landed. `Land on main` is done when this is
/// empty, and `loop stage done` refuses until it is (ADR-2611161800 §2, §3).
/// Local only: commits not pushed are not counted, because a push can need
/// someone's approval.
pub fn not_on_main(dir: &Path) -> Vec<Item> {
    let base = base_branch(dir);
    let mut out = Vec::new();
    out.extend(uncommitted_items(dir, &uncommitted_files(dir)));
    out.extend(branch_items(dir, &base, None).into_iter().filter(|i| i.kind == "unlanded"));
    out
}

/// What one session left open: the touched files it has not committed, its
/// checklist, its own branch when it is not on the base, and its ADR's
/// close-out (ADR-2611161800 §5). Commits not pushed are listed apart, in
/// `reported`, because a push can need someone's approval and a hook must
/// not force one.
pub struct SessionOpen {
    pub blocking: Vec<Item>,
    pub reported: Vec<Item>,
}

pub fn session_open(dir: &Path, session: &str) -> SessionOpen {
    let entry = crate::commands::loop_cmd::session_entry(dir, session).unwrap_or(Value::Null);
    let touched: Vec<String> = crate::commands::loop_cmd::entry_files(&entry)
        .into_iter()
        .filter(|f| !Path::new(f).is_absolute())
        .collect();
    // a file the session made and never added is as uncommitted as one it changed
    let mut dirty: Vec<String> = uncommitted_files(dir).into_iter().filter(|f| touched.contains(f)).collect();
    for f in untracked_files(dir).into_iter().filter(|f| touched.contains(f)) {
        if !dirty.contains(&f) {
            dirty.push(f);
        }
    }
    let mut blocking = Vec::new();
    blocking.extend(uncommitted_items(dir, &dirty));
    blocking.extend(task_items(&entry));
    let base = base_branch(dir);
    blocking.extend(branch_items(dir, &base, Some(dir)));
    let all_done = blocking.is_empty();
    if all_done {
        blocking.extend(adr_item(dir, &entry));
    }
    SessionOpen { blocking, reported: unpushed_item(dir).into_iter().collect() }
}

fn render(project: &str, items: &[Item]) -> String {
    if items.is_empty() {
        return format!("\u{2b21} nothing open in {project}: the work is on main");
    }
    let mut s = format!("\u{2b21} open work in {project}: {} item{}\n", items.len(), if items.len() == 1 { "" } else { "s" });
    for i in items {
        s.push_str(&format!("  {:<12} {}\n  {:<12} {}\n", i.kind, i.what, "", i.fix));
    }
    s.trim_end().to_string()
}

pub async fn run(args: FinishArgs) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let dir = git(&cwd, &["rev-parse", "--show-toplevel"]).map(PathBuf::from).unwrap_or(cwd);
    crate::commands::events::start(&dir);
    let items = open_work(&dir);
    let project = crate::commands::loop_cmd::project_name(&dir);
    let list: Vec<Value> = items.iter().map(Item::json).collect();
    let mut content = Map::new();
    content.insert("project".into(), json!(project));
    content.insert("base".into(), json!(base_branch(&dir)));
    content.insert("open".into(), json!(list));
    crate::commands::events::activity("hexa-open-work", "hexa.open_work", content.clone());
    // with `--events` on stdout, fd 1 is stderr by now (events::init), so this is the person's text either way
    if args.json {
        println!("{}", serde_json::to_string_pretty(&Value::Object(content.clone()))?);
    } else {
        println!("{}", render(&project, &items));
    }
    crate::commands::events::finish(Some(Value::Object(content)));
    if args.exit_code && !items.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}
