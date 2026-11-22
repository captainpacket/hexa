#![allow(unused, clippy::all)]
mod part_1 {
    #[allow(unused_imports)]
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::process::{Command, Output};

    use hexa_cli::commands::finish::{open_work, session_open, Item};
    use hexa_cli::commands::loop_cmd::{entry_tasks, session_entry};

    const SESSION: &str = "adr-2611220204-part1-session";

    struct Repo {
        dir: tempfile::TempDir,
        home: tempfile::TempDir,
    }

    impl Repo {
        fn path(&self) -> &Path {
            self.dir.path()
        }
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn write(dir: &Path, rel: &str, text: &str) {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, text).unwrap();
    }

    fn repo() -> Repo {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let d = dir.path();
        git(d, &["init", "-q", "-b", "main"]);
        write(d, "README.md", "readme\n");
        git(d, &["add", "-A"]);
        git(d, &["commit", "-q", "-m", "initial"]);
        Repo { dir, home }
    }

    fn hexa(r: &Repo, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_hexa"))
            .current_dir(r.path())
            .args(args)
            .env("HOME", r.home.path())
            .env("CLAUDE_SESSION_ID", SESSION)
            .env("HEXA_SESSION", SESSION)
            .env("HEXA_SESSION_ID", SESSION)
            .env_remove("HEXA_TASK")
            .output()
            .expect("hexa runs")
    }

    fn hexa_ok(r: &Repo, args: &[&str]) -> Output {
        let out = hexa(r, args);
        assert!(
            out.status.success(),
            "hexa {:?} failed:\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }

    fn collect_keys(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(m) => {
                for (k, val) in m {
                    out.push(k.clone());
                    collect_keys(val, out);
                }
            }
            serde_json::Value::Array(a) => {
                for val in a {
                    collect_keys(val, out);
                }
            }
            _ => {}
        }
    }

    fn the_session(dir: &Path) -> String {
        let text = std::fs::read_to_string(dir.join(".hexa/loop.json")).expect("loop.json exists");
        let v: serde_json::Value = serde_json::from_str(&text).expect("loop.json is JSON");
        let mut keys = Vec::new();
        collect_keys(&v, &mut keys);
        let mut ids: Vec<String> = keys
            .into_iter()
            .filter(|k| session_entry(dir, k).is_some())
            .collect();
        ids.sort();
        ids.dedup();
        assert_eq!(
            ids.len(),
            1,
            "expected exactly one session in loop.json: {:?}",
            ids
        );
        ids.remove(0)
    }

    fn keys(items: &[Item]) -> Vec<String> {
        items.iter().map(|i| i.key.clone()).collect()
    }

    fn session_keys(dir: &Path) -> (Vec<String>, Vec<String>) {
        let sid = the_session(dir);
        let open = session_open(dir, &sid);
        (keys(&open.blocking), keys(&open.reported))
    }

    fn add_adr(r: &Repo) {
        write(
        r.path(),
        "docs/adrs/ADR-2611220204-wait.md",
        "# ADR-2611220204: A session can wait\n\n**Status:** Proposed\n**Date:** 2026-11-22\n\n## Decision\n\nWait.\n",
    );
        git(r.path(), &["add", "-A"]);
        git(r.path(), &["commit", "-q", "-m", "adr"]);
    }

    /// Record the ADR on the loop and make sure the two close-out steps are on the checklist.
    fn record_adr_with_closeout(r: &Repo) {
        hexa_ok(r, &["loop", "adr", "ADR-2611220204"]);
        let sid = the_session(r.path());
        let entry = session_entry(r.path(), &sid).unwrap();
        let tasks = entry_tasks(&entry);
        if !tasks.iter().any(|(_, t, _)| t.contains("Land")) {
            hexa_ok(r, &["loop", "task", "add", "Land on main"]);
        }
        if !tasks
            .iter()
            .any(|(_, t, _)| t.contains("Close") || t.contains("close"))
        {
            hexa_ok(r, &["loop", "task", "add", "Close ADR-2611220204"]);
        }
    }

    fn find_task_item<'a>(items: &'a [Item], title: &str) -> &'a Item {
        items
            .iter()
            .find(|i| i.what.contains(title) && i.key.starts_with("task:"))
            .unwrap_or_else(|| panic!("no task item for {title:?} in {items:?}"))
    }

    fn hold_of(dir: &Path) -> Option<serde_json::Value> {
        let sid = the_session(dir);
        let entry = session_entry(dir, &sid)?;
        match entry.get("hold") {
            None | Some(serde_json::Value::Null) => None,
            Some(h) => Some(h.clone()),
        }
    }

    fn hold_keys(hold: &serde_json::Value) -> Vec<String> {
        let obj = hold.as_object().expect("hold is an object");
        for (_, v) in obj {
            if let Some(arr) = v.as_array() {
                if arr.iter().all(|x| x.is_string()) {
                    return arr
                        .iter()
                        .map(|x| x.as_str().unwrap().to_string())
                        .collect();
                }
            }
        }
        panic!("hold has no list of keys: {hold}");
    }

    fn hold_reason(hold: &serde_json::Value) -> String {
        hold.get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("hold has no reason: {hold}"))
            .to_string()
    }

    fn loop_bytes(dir: &Path) -> Option<Vec<u8>> {
        std::fs::read(dir.join(".hexa/loop.json")).ok()
    }

    /// S01: Each uncommitted file is its own item, keyed by its path
    #[test]
    fn s01_each_uncommitted_file_is_its_own_item() {
        let r = repo();
        write(r.path(), "src/a.rs", "fn a() {}\n");
        write(r.path(), "src/b.rs", "fn b() {}\n");
        git(r.path(), &["add", "-A"]);
        git(r.path(), &["commit", "-q", "-m", "src"]);
        write(r.path(), "src/a.rs", "fn a() { 1; }\n");
        write(r.path(), "src/b.rs", "fn b() { 2; }\n");

        let items = open_work(r.path());
        let unc: Vec<&Item> = items
            .iter()
            .filter(|i| i.key.starts_with("uncommitted:"))
            .collect();
        let ks: BTreeSet<String> = unc.iter().map(|i| i.key.clone()).collect();
        let want: BTreeSet<String> = ["uncommitted:src/a.rs", "uncommitted:src/b.rs"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(unc.len(), 2, "two uncommitted items: {items:?}");
        assert_eq!(ks, want);
        for i in unc {
            let name = i.key.trim_start_matches("uncommitted:src/");
            assert!(!i.what.is_empty());
            assert!(i.what.contains(name), "text {:?} names the file", i.what);
            assert_ne!(i.what, i.key, "the text is what is printed, not the key");
        }
    }

    /// S02: Task, close-out, ADR and unpushed items carry their documented keys
    #[test]
    fn s02_task_closeout_adr_unpushed_keys() {
        let r = repo();
        add_adr(&r);
        let remote = tempfile::tempdir().unwrap();
        git(remote.path(), &["init", "-q", "--bare"]);
        git(
            r.path(),
            &["remote", "add", "origin", remote.path().to_str().unwrap()],
        );
        git(r.path(), &["push", "-q", "-u", "origin", "main"]);
        write(r.path(), "later.txt", "later\n");
        git(r.path(), &["add", "-A"]);
        git(r.path(), &["commit", "-q", "-m", "ahead"]);

        hexa_ok(&r, &["loop", "task", "add", "Write the parser"]);
        record_adr_with_closeout(&r);

        // The open work of the session that recorded it, read by the verb
        // (the test process is not that session, so an in-process call reads none).
        let fin = hexa_ok(&r, &["finish", "--json"]);
        let v: serde_json::Value = serde_json::from_slice(&fin.stdout).expect("finish --json is JSON");
        let mut all: BTreeSet<String> = v["open"]
            .as_array()
            .expect("an open list")
            .iter()
            .filter_map(|i| i["key"].as_str().map(String::from))
            .collect();
        let (b, rep) = session_keys(r.path());
        all.extend(b);
        all.extend(rep);
        for k in [
            "task:Write the parser",
            "closeout:land",
            "closeout:close:ADR-2611220204",
            "adr:ADR-2611220204",
            "unpushed:origin/main",
        ] {
            assert!(all.contains(k), "missing key {k:?} in {all:?}");
        }
    }

    /// S03: An unlanded branch is keyed by its name and tip sha
    #[test]
    fn s03_unlanded_branch_keyed_by_name_and_tip() {
        let r = repo();
        git(r.path(), &["checkout", "-q", "-b", "feat/x"]);
        write(r.path(), "x.txt", "x\n");
        git(r.path(), &["add", "-A"]);
        git(r.path(), &["commit", "-q", "-m", "x"]);
        let tip = git(r.path(), &["rev-parse", "HEAD"]);

        let items = open_work(r.path());
        let item = items
            .iter()
            .find(|i| i.key.starts_with("unlanded:"))
            .unwrap_or_else(|| panic!("no unlanded item: {items:?}"));
        let rest = item
            .key
            .strip_prefix("unlanded:feat/x@")
            .unwrap_or_else(|| panic!("key {:?}", item.key));
        assert!(
            rest.len() >= 7 && tip.starts_with(rest),
            "key {:?} names tip {tip}",
            item.key
        );
    }

    /// S04: A task's key does not change when its position or marker changes
    #[test]
    fn s04_task_key_stable_across_position_and_marker() {
        let r = repo();
        hexa_ok(&r, &["loop", "task", "add", "First step"]);
        hexa_ok(&r, &["loop", "task", "add", "Write the parser"]);
        let sid = the_session(r.path());

        let before = session_open(r.path(), &sid).blocking;
        let t0 = find_task_item(&before, "Write the parser").clone();
        assert_eq!(t0.key, "task:Write the parser");

        // Its position changes.
        hexa_ok(&r, &["loop", "task", "rm", "1"]);
        let after_rm = session_open(r.path(), &sid).blocking;
        let t1 = find_task_item(&after_rm, "Write the parser");
        assert_eq!(t1.key, "task:Write the parser");

        // Another task is added, and it is started so its marker changes.
        hexa_ok(&r, &["loop", "task", "add", "Another step"]);
        hexa_ok(&r, &["loop", "task", "start", "1"]);
        let after_start = session_open(r.path(), &sid).blocking;
        let t2 = find_task_item(&after_start, "Write the parser");
        assert_eq!(t2.key, "task:Write the parser");
        assert_eq!(
            after_start
                .iter()
                .filter(|i| i.key == "task:Write the parser")
                .count(),
            1
        );
    }

    /// S05: A new commit on a branch does not keep the old key
    #[test]
    fn s05_new_commit_changes_unlanded_key() {
        let r = repo();
        git(r.path(), &["checkout", "-q", "-b", "feat/x"]);
        write(r.path(), "x.txt", "x\n");
        git(r.path(), &["add", "-A"]);
        git(r.path(), &["commit", "-q", "-m", "x1"]);
        let t1 = git(r.path(), &["rev-parse", "HEAD"]);
        let k1 = open_work(r.path())
            .into_iter()
            .find(|i| i.key.starts_with("unlanded:feat/x@"))
            .expect("unlanded item")
            .key;
        assert!(t1.starts_with(k1.trim_start_matches("unlanded:feat/x@")));

        write(r.path(), "x.txt", "x2\n");
        git(r.path(), &["add", "-A"]);
        git(r.path(), &["commit", "-q", "-m", "x2"]);
        let t2 = git(r.path(), &["rev-parse", "HEAD"]);
        let k2 = open_work(r.path())
            .into_iter()
            .find(|i| i.key.starts_with("unlanded:feat/x@"))
            .expect("unlanded item")
            .key;
        let sha2 = k2.trim_start_matches("unlanded:feat/x@");
        assert!(
            sha2.len() >= 7 && t2.starts_with(sha2),
            "key {k2:?} names {t2}"
        );
        assert_ne!(k1, k2);
    }

    /// S06: The key is a function of the work, not of the printed text
    #[test]
    fn s06_keys_are_stable_well_formed_and_unique() {
        let r = repo();
        add_adr(&r);
        write(r.path(), "src/a.rs", "a\n");
        write(r.path(), "src/b.rs", "b\n");
        write(r.path(), "src/c.rs", "c\n");
        git(r.path(), &["add", "-A"]);
        git(r.path(), &["commit", "-q", "-m", "src"]);
        git(r.path(), &["checkout", "-q", "-b", "feat/x"]);
        write(r.path(), "x.txt", "x\n");
        git(r.path(), &["add", "-A"]);
        git(r.path(), &["commit", "-q", "-m", "x"]);
        write(r.path(), "src/a.rs", "a2\n");
        hexa_ok(&r, &["loop", "task", "add", "Write the parser"]);
        hexa_ok(&r, &["loop", "task", "add", "Write the lexer"]);
        record_adr_with_closeout(&r);

        let forms = [
            "uncommitted:",
            "task:",
            "closeout:",
            "unlanded:",
            "adr:",
            "unpushed:",
        ];
        let check = |ks: &[String]| {
            let set: BTreeSet<&String> = ks.iter().collect();
            assert_eq!(set.len(), ks.len(), "keys are unique: {ks:?}");
            for k in ks {
                assert!(!k.is_empty());
                assert!(
                    forms.iter().any(|f| k.starts_with(f) && k.len() > f.len()),
                    "key {k:?} has a documented form"
                );
                if k.starts_with("closeout:") {
                    assert!(
                        k == "closeout:land" || k.starts_with("closeout:close:"),
                        "key {k:?}"
                    );
                }
            }
        };

        let w1 = keys(&open_work(r.path()));
        let (b1, r1) = session_keys(r.path());
        check(&w1);
        check(&b1);
        check(&r1);

        // Changes that only affect printed text: more files (counts), a task closed
        // and another added (positions, markers).
        write(r.path(), "src/b.rs", "b2\n");
        write(r.path(), "src/c.rs", "c2\n");
        hexa_ok(&r, &["loop", "task", "add", "Write the emitter"]);
        hexa_ok(&r, &["loop", "task", "start", "2"]);

        let w2 = keys(&open_work(r.path()));
        let (b2, r2) = session_keys(r.path());
        check(&w2);
        check(&b2);
        check(&r2);
        for k in &w1 {
            assert!(w2.contains(k), "open_work key {k:?} kept: {w2:?}");
        }
        for k in &b1 {
            assert!(b2.contains(k), "blocking key {k:?} kept: {b2:?}");
        }
        for k in &r1 {
            assert!(r2.contains(k), "reported key {k:?} kept: {r2:?}");
        }
    }

    /// S07: A hold records the reason, time and blocking keys
    #[test]
    fn s07_hold_records_reason_time_and_keys() {
        let r = repo();
        add_adr(&r);
        git(r.path(), &["checkout", "-q", "-b", "feat/x"]);
        write(r.path(), "x.txt", "x\n");
        git(r.path(), &["add", "-A"]);
        git(r.path(), &["commit", "-q", "-m", "x"]);
        let tip = git(r.path(), &["rev-parse", "HEAD"]);
        record_adr_with_closeout(&r);

        let (blocking, _) = session_keys(r.path());
        hexa_ok(&r, &["loop", "hold", "waiting on owner approval"]);

        let hold = hold_of(r.path()).expect("an active hold");
        assert_eq!(hold_reason(&hold), "waiting on owner approval");
        let obj = hold.as_object().unwrap();
        assert!(
            obj.iter().any(|(k, v)| k != "reason"
                && ((v.is_string() && !v.as_str().unwrap().is_empty()) || v.is_number())),
            "hold records a time: {hold}"
        );
        let held: BTreeSet<String> = hold_keys(&hold).into_iter().collect();
        let want: BTreeSet<String> = blocking.into_iter().collect();
        assert_eq!(held, want, "the hold holds exactly the blocking keys");
        assert!(held.contains("closeout:land"), "{held:?}");
        assert!(
            held.iter().any(|k| k
                .strip_prefix("unlanded:feat/x@")
                .map(|s| s.len() >= 7 && tip.starts_with(s))
                .unwrap_or(false)),
            "{held:?} holds unlanded:feat/x@{tip}"
        );
    }

    /// S08: A second hold replaces the first and keeps it in history; --clear removes it
    #[test]
    fn s08_second_hold_replaces_first_and_clear_removes() {
        let r = repo();
        hexa_ok(&r, &["loop", "task", "add", "Write the parser"]);
        hexa_ok(&r, &["loop", "hold", "waiting on tests"]);
        assert_eq!(hold_reason(&hold_of(r.path()).unwrap()), "waiting on tests");

        hexa_ok(&r, &["loop", "hold", "waiting on review"]);
        let hold = hold_of(r.path()).expect("an active hold");
        assert_eq!(hold_reason(&hold), "waiting on review");
        let sid = the_session(r.path());
        let mut entry = session_entry(r.path(), &sid).unwrap();
        entry.as_object_mut().unwrap().remove("hold");
        assert!(
            entry.to_string().contains("waiting on tests"),
            "the first hold is kept in the entry's history: {entry}"
        );

        hexa_ok(&r, &["loop", "hold", "--clear"]);
        assert!(hold_of(r.path()).is_none(), "no active hold after --clear");
    }

    /// S09: A blank reason is refused and nothing is written
    #[test]
    fn s09_blank_reason_refused() {
        let r = repo();
        hexa_ok(&r, &["loop", "task", "add", "Write the parser"]);
        let before = loop_bytes(r.path());
        assert!(before.is_some());

        for reason in ["", "   "] {
            let out = hexa(&r, &["loop", "hold", reason]);
            assert!(!out.status.success(), "hold {reason:?} is refused");
            assert_eq!(
                loop_bytes(r.path()),
                before,
                "loop.json unchanged after hold {reason:?}"
            );
        }

        // A real reason is accepted in the same state.
        hexa_ok(&r, &["loop", "hold", "waiting on tests"]);
        assert_ne!(loop_bytes(r.path()), before);
    }

    /// S10: A hold is refused when nothing is open
    #[test]
    fn s10_hold_refused_when_nothing_open() {
        let r = repo();
        hexa_ok(&r, &["loop", "gate", "true"]);
        let sid = the_session(r.path());
        assert!(
            session_open(r.path(), &sid).blocking.is_empty(),
            "nothing is open"
        );
        let before = loop_bytes(r.path());

        let out = hexa(&r, &["loop", "hold", "waiting"]);
        assert!(!out.status.success(), "hold with nothing open is refused");
        assert_eq!(loop_bytes(r.path()), before, "loop.json unchanged");

        // Once something is open, the same hold is accepted.
        hexa_ok(&r, &["loop", "task", "add", "Write the parser"]);
        hexa_ok(&r, &["loop", "hold", "waiting"]);
        assert_eq!(hold_reason(&hold_of(r.path()).unwrap()), "waiting");
    }
}

mod part_2 {
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output, Stdio};

    use serde_json::{json, Value};

    const HEXA: &str = env!("CARGO_BIN_EXE_hexa");

    fn git(dir: &Path, home: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("HOME", home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .stdin(Stdio::null())
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn all(o: &Output) -> String {
        format!(
            "{}\n{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        )
    }

    /// Every loop entry in a loop.json value, as (session id, entry).
    fn entries(v: &Value, key: Option<&str>, out: &mut Vec<(String, Value)>) {
        match v {
            Value::Object(m) => {
                if m.contains_key("tasks") || m.contains_key("gate") {
                    let id = m
                        .get("session")
                        .or_else(|| m.get("session_id"))
                        .and_then(|s| s.as_str())
                        .map(String::from)
                        .or_else(|| key.map(String::from));
                    if let Some(id) = id {
                        out.push((id, v.clone()));
                        return;
                    }
                }
                for (k, c) in m {
                    entries(c, Some(k), out);
                }
            }
            Value::Array(a) => {
                for c in a {
                    entries(c, None, out);
                }
            }
            _ => {}
        }
    }

    /// The value stored under `key` anywhere in `v`.
    fn find_key<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
        match v {
            Value::Object(m) => {
                if let Some(x) = m.get(key) {
                    return Some(x);
                }
                m.values().find_map(|c| find_key(c, key))
            }
            Value::Array(a) => a.iter().find_map(|c| find_key(c, key)),
            _ => None,
        }
    }

    struct Stop {
        blocked: bool,
        text: String,
    }

    struct Repo {
        _tmp: tempfile::TempDir,
        dir: PathBuf,
        home: PathBuf,
        env_id: String,
        sid: String,
    }

    impl Repo {
        /// A git repository on `main` with src/a.rs and src/b.rs committed; with
        /// `feature`, checked out on `feat/x` one commit ahead of main.
        fn setup(feature: bool) -> Repo {
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path().join("proj");
            let home = tmp.path().join("home");
            std::fs::create_dir_all(dir.join("src")).unwrap();
            std::fs::create_dir_all(&home).unwrap();
            git(&dir, &home, &["init", "-q", "-b", "main"]);
            git(&dir, &home, &["config", "commit.gpgsign", "false"]);
            git(&dir, &home, &["config", "user.name", "Test"]);
            git(&dir, &home, &["config", "user.email", "test@example.com"]);
            std::fs::write(dir.join(".git/info/exclude"), ".hexa/\n").unwrap();
            std::fs::write(dir.join("src/a.rs"), "pub fn a() {}\n").unwrap();
            std::fs::write(dir.join("src/b.rs"), "pub fn b() {}\n").unwrap();
            std::fs::write(dir.join("README.md"), "# proj\n").unwrap();
            git(&dir, &home, &["add", "."]);
            git(&dir, &home, &["commit", "-q", "-m", "initial"]);
            if feature {
                git(&dir, &home, &["checkout", "-q", "-b", "feat/x"]);
                std::fs::write(dir.join("feat.txt"), "feature\n").unwrap();
                git(&dir, &home, &["add", "feat.txt"]);
                git(&dir, &home, &["commit", "-q", "-m", "feature work"]);
            }
            std::fs::create_dir_all(dir.join(".hexa")).unwrap();
            Repo {
                _tmp: tmp,
                dir,
                home,
                env_id: uuid::Uuid::new_v4().to_string(),
                sid: String::new(),
            }
        }

        fn new(feature: bool) -> Repo {
            let mut r = Repo::setup(feature);
            let out = r.run(&["loop", "gate", "true"]);
            assert!(out.status.success(), "loop gate: {}", all(&out));
            let es = r.loop_entries();
            assert_eq!(es.len(), 1, "one loop entry expected: {:?}", es);
            r.sid = es[0].0.clone();
            r
        }

        fn base(&self, program: &str, env_id: &str) -> Command {
            let mut c = Command::new(program);
            c.current_dir(&self.dir)
                .env("HOME", &self.home)
                .env_remove("HEXA_TASK")
                .env("CLAUDE_SESSION_ID", env_id)
                .env("HEXA_SESSION_ID", env_id)
                .env("CLAUDE_PROJECT_DIR", &self.dir)
                .env("NO_COLOR", "1")
                .env("GIT_AUTHOR_NAME", "Test")
                .env("GIT_AUTHOR_EMAIL", "test@example.com")
                .env("GIT_COMMITTER_NAME", "Test")
                .env("GIT_COMMITTER_EMAIL", "test@example.com")
                .stdin(Stdio::null());
            c
        }

        fn run(&self, args: &[&str]) -> Output {
            self.base(HEXA, &self.env_id).args(args).output().unwrap()
        }

        fn run_stdin(&self, args: &[&str], input: &str) -> Output {
            let mut child = self
                .base(HEXA, &self.env_id)
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
            child.wait_with_output().unwrap()
        }

        fn git(&self, args: &[&str]) -> String {
            git(&self.dir, &self.home, args)
        }

        fn loop_json(&self) -> Value {
            let text =
                std::fs::read_to_string(self.dir.join(".hexa/loop.json")).unwrap_or_default();
            serde_json::from_str(&text).unwrap_or(Value::Null)
        }

        fn loop_entries(&self) -> Vec<(String, Value)> {
            let mut out = Vec::new();
            entries(&self.loop_json(), None, &mut out);
            out
        }

        fn add_task(&self, title: &str) {
            let out = self.run(&["loop", "task", "add", title]);
            assert!(out.status.success(), "task add: {}", all(&out));
        }

        fn hold(&self, reason: &str) -> Output {
            self.run(&["loop", "hold", reason])
        }

        /// The session edits `rel`: the edit hooks see it and the file changes.
        fn touch_file(&self, rel: &str, content: &str) {
            let abs = self.dir.join(rel);
            let payload = |event: &str| {
                json!({
                    "session_id": self.sid,
                    "cwd": self.dir,
                    "transcript_path": self.home.join("transcript.jsonl"),
                    "hook_event_name": event,
                    "tool_name": "Write",
                    "tool_input": {"file_path": abs, "content": content},
                    "tool_response": {"filePath": abs, "success": true},
                })
                .to_string()
            };
            let _ = self.run_stdin(&["hook", "pre-edit"], &payload("PreToolUse"));
            std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
            std::fs::write(&abs, content).unwrap();
            let _ = self.run_stdin(&["hook", "post-edit"], &payload("PostToolUse"));
        }

        fn stop(&self, active: bool) -> Stop {
            let transcript = self.home.join("transcript.jsonl");
            if !transcript.exists() {
                std::fs::write(&transcript, "").unwrap();
            }
            let payload = json!({
                "session_id": self.sid,
                "transcript_path": transcript,
                "cwd": self.dir,
                "hook_event_name": "Stop",
                "stop_hook_active": active,
                "permission_mode": "default",
            })
            .to_string();
            let out = self.run_stdin(&["hook", "stop"], &payload);
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let mut blocked = out.status.code() == Some(2);
            let mut text = all(&out);
            let mut docs: Vec<Value> = Vec::new();
            if let Ok(v) = serde_json::from_str::<Value>(stdout.trim()) {
                docs.push(v);
            }
            for line in stdout.lines() {
                if let Ok(v) = serde_json::from_str::<Value>(line.trim()) {
                    docs.push(v);
                }
            }
            for v in &docs {
                if v.get("decision").and_then(|d| d.as_str()) == Some("block") {
                    blocked = true;
                }
                for f in ["reason", "systemMessage", "stopReason"] {
                    if let Some(s) = v.get(f).and_then(|s| s.as_str()) {
                        text.push('\n');
                        text.push_str(s);
                    }
                }
            }
            Stop { blocked, text }
        }

        fn stop_blocks(&self) -> Value {
            let text = std::fs::read_to_string(self.dir.join(".hexa/stop-blocks.json"))
                .expect("stop-blocks.json written");
            serde_json::from_str(&text).expect("stop-blocks.json is JSON")
        }
    }

    /// S11: A hold does not loosen any other check
    #[test]
    fn s11_a_hold_does_not_loosen_any_other_check() {
        let plain = Repo::new(true);
        let held = Repo::new(true);
        for r in [&plain, &held] {
            r.touch_file("src/a.rs", "pub fn a() { /* edited */ }\n");
            // `loop adr` appends the close-out steps; step 1 is then `Land on main`.
            std::fs::create_dir_all(r.dir.join("docs/adrs")).unwrap();
            std::fs::write(r.dir.join("docs/adrs/ADR-0001-held-work.md"), "# ADR-0001: held work\n\n**Status:** Accepted\n").unwrap();
            let out = r.run(&["loop", "adr", "ADR-0001"]);
            assert!(out.status.success(), "loop adr: {}", all(&out));
        }
        let h = held.hold("waiting on owner approval");
        assert!(h.status.success(), "hold refused: {}", all(&h));

        for args in [
            &["loop", "task", "done", "1"][..],
            &["loop", "stage", "done"][..],
            &["finish", "--exit-code"][..],
        ] {
            let p = plain.run(args);
            let q = held.run(args);
            assert!(
                !p.status.success(),
                "{:?} should refuse without a hold: {}",
                args,
                all(&p)
            );
            assert_eq!(
                q.status.code(),
                p.status.code(),
                "{:?} with a hold must refuse exactly as without: held {} / plain {}",
                args,
                all(&q),
                all(&p)
            );
        }
    }

    /// S12: Resume does not carry a hold into a new session
    #[test]
    fn s12_resume_does_not_carry_a_hold() {
        let mut repo = Repo::setup(false);
        let a_env = repo.env_id.clone();
        let script = r#""$0" loop gate true && "$0" loop task add 'Step one' && "$0" loop hold 'waiting on review'; s=$?; exit $s"#;
        let out = repo
            .base("sh", &a_env)
            .env("HEXA_SESSION_PID", "0")
            .arg("-c")
            .arg(script)
            .arg(HEXA)
            .output()
            .unwrap();
        assert!(out.status.success(), "session A setup: {}", all(&out));
        let a_entries = repo.loop_entries();
        assert!(
            a_entries
                .iter()
                .any(|(_, e)| e.to_string().contains("waiting on review")),
            "the hold should be recorded: {:?}",
            a_entries
        );
        let a_ids: Vec<String> = a_entries.iter().map(|(id, _)| id.clone()).collect();

        let b_env = uuid::Uuid::new_v4().to_string();
        repo.env_id = b_env;
        let out = repo.run(&["loop", "resume"]);
        assert!(out.status.success(), "resume: {}", all(&out));

        let b = repo
            .loop_entries()
            .into_iter()
            .find(|(id, e)| !a_ids.contains(id) && e.to_string().contains("Step one"))
            .expect("the new session carries the checklist");
        assert!(
            !b.1.to_string().contains("waiting on review"),
            "the new session must have no hold: {}",
            b.1
        );

        repo.sid = b.0.clone();
        let s = repo.stop(false);
        assert!(s.blocked, "without a hold the open task blocks: {}", s.text);
    }

    /// S13: While the hold covers everything open, the stop goes through with one line
    #[test]
    fn s13_hold_covering_everything_lets_stop_through() {
        let repo = Repo::new(true);
        repo.touch_file("src/a.rs", "pub fn a() { 1; }\n");
        repo.add_task("Step one");
        let h = repo.hold("waiting on owner approval");
        assert!(h.status.success(), "hold refused: {}", all(&h));

        let s = repo.stop(false);
        assert!(!s.blocked, "the stop should be let through: {}", s.text);
        let three = regex::Regex::new(r"(?i)\b(3|three)\b").unwrap();
        assert!(
            s.text
                .lines()
                .any(|l| l.contains("waiting on owner approval") && three.is_match(l)),
            "one line naming the reason and three items: {}",
            s.text
        );
    }

    /// S14: An active hold is shown by `hexa loop` and at session start
    #[test]
    fn s14_active_hold_shown_by_loop_and_session_start() {
        let repo = Repo::new(false);
        repo.add_task("Step one");
        let h = repo.hold("waiting on review");
        assert!(h.status.success(), "hold refused: {}", all(&h));

        let out = all(&repo.run(&["loop"]));
        assert!(out.contains("waiting on review"), "hexa loop: {}", out);
        assert!(out.to_lowercase().contains("hold"), "hexa loop: {}", out);

        let payload = json!({
            "session_id": repo.sid,
            "cwd": repo.dir,
            "transcript_path": repo.home.join("transcript.jsonl"),
            "hook_event_name": "SessionStart",
            "source": "startup",
        })
        .to_string();
        let start = all(&repo.run_stdin(&["hook", "session-start"], &payload));
        assert!(
            start.contains("waiting on review"),
            "session start: {}",
            start
        );
    }

    /// S15: Work opened after the hold still blocks, and only it is listed
    #[test]
    fn s15_work_opened_after_hold_blocks() {
        let repo = Repo::new(true);
        repo.touch_file("src/a.rs", "pub fn a() { 1; }\n");
        let h = repo.hold("waiting on review");
        assert!(h.status.success(), "hold refused: {}", all(&h));

        repo.touch_file("src/b.rs", "pub fn b() { 2; }\n");
        repo.add_task("Fix docs");
        std::fs::write(repo.dir.join("c.txt"), "more\n").unwrap();
        repo.git(&["add", "c.txt"]);
        repo.git(&["commit", "-q", "-m", "more work"]);
        let t2 = repo.git(&["rev-parse", "HEAD"]);

        let s = repo.stop(false);
        assert!(s.blocked, "new work must block: {}", s.text);
        assert!(s.text.contains("src/b.rs"), "lists src/b.rs: {}", s.text);
        assert!(s.text.contains("Fix docs"), "lists Fix docs: {}", s.text);
        assert!(s.text.contains("feat/x"), "lists feat/x: {}", s.text);
        assert!(s.text.contains(&t2[..7]), "lists feat/x at T2: {}", s.text);
        assert!(
            !s.text.contains("src/a.rs"),
            "must not list held src/a.rs: {}",
            s.text
        );
    }

    /// S16: A hold whose keys have all closed is inert
    #[test]
    fn s16_stale_hold_is_inert() {
        let repo = Repo::new(false);
        repo.add_task("Step one");
        let h = repo.hold("waiting on review");
        assert!(h.status.success(), "hold refused: {}", all(&h));
        let d = repo.run(&["loop", "task", "done", "1"]);
        assert!(d.status.success(), "task done: {}", all(&d));
        repo.add_task("New work");

        let before = std::fs::read(repo.dir.join(".hexa/loop.json")).unwrap();
        let s = repo.stop(false);
        let after = std::fs::read(repo.dir.join(".hexa/loop.json")).unwrap();
        assert!(s.blocked, "new work blocks as without a hold: {}", s.text);
        assert!(s.text.contains("New work"), "lists New work: {}", s.text);
        assert_eq!(before, after, ".hexa/loop.json must not be written");

        let out = all(&repo.run(&["loop"]));
        assert!(
            out.to_lowercase().contains("stale"),
            "hexa loop shows stale: {}",
            out
        );
    }

    /// S17: An item blocks if and only if its key is not in an active hold
    #[test]
    fn s17_item_blocks_iff_not_in_active_hold() {
        // No hold: every open item blocks and is listed.
        let plain = Repo::new(false);
        plain.touch_file("src/a.rs", "pub fn a() { 1; }\n");
        plain.add_task("Step one");
        plain.touch_file("src/b.rs", "pub fn b() { 2; }\n");
        plain.add_task("Step two");
        let s = plain.stop(false);
        assert!(s.blocked, "no hold: blocks: {}", s.text);
        for w in ["src/a.rs", "Step one", "src/b.rs", "Step two"] {
            assert!(s.text.contains(w), "no hold: lists {}: {}", w, s.text);
        }

        // Partial hold: only the unheld items block and are listed.
        let part = Repo::new(false);
        part.touch_file("src/a.rs", "pub fn a() { 1; }\n");
        part.add_task("Step one");
        let h = part.hold("waiting on review");
        assert!(h.status.success(), "hold refused: {}", all(&h));
        part.touch_file("src/b.rs", "pub fn b() { 2; }\n");
        part.add_task("Step two");
        let s = part.stop(false);
        assert!(s.blocked, "partial hold: blocks: {}", s.text);
        for w in ["src/b.rs", "Step two"] {
            assert!(s.text.contains(w), "partial hold: lists {}: {}", w, s.text);
        }
        for w in ["src/a.rs", "Step one"] {
            assert!(
                !s.text.contains(w),
                "partial hold: must not list {}: {}",
                w,
                s.text
            );
        }

        // Full hold: nothing blocks.
        let full = Repo::new(false);
        full.touch_file("src/a.rs", "pub fn a() { 1; }\n");
        full.add_task("Step one");
        let h = full.hold("waiting on review");
        assert!(h.status.success(), "hold refused: {}", all(&h));
        let s = full.stop(false);
        assert!(!s.blocked, "full hold: let through: {}", s.text);
    }

    /// S18: After three blocks on the same keys, later stops go through with the list
    #[test]
    fn s18_three_blocks_then_through_with_list() {
        let repo = Repo::new(false);
        repo.add_task("Step one");
        for i in 1..=5 {
            let s = repo.stop(i > 1);
            if i <= 3 {
                assert!(s.blocked, "stop {} should be blocked: {}", i, s.text);
            } else {
                assert!(!s.blocked, "stop {} should be let through: {}", i, s.text);
                assert!(
                    s.text.contains("Step one"),
                    "stop {} prints the list: {}",
                    i,
                    s.text
                );
            }
        }
    }

    /// S19: When the keys change the count starts again
    #[test]
    fn s19_count_restarts_when_keys_change() {
        let repo = Repo::new(false);
        repo.add_task("Step one");
        for i in 1..=4 {
            let s = repo.stop(i > 1);
            assert_eq!(s.blocked, i <= 3, "stop {}: {}", i, s.text);
        }
        repo.touch_file("src/b.rs", "pub fn b() { 2; }\n");
        let s = repo.stop(false);
        assert!(s.blocked, "a new run starts with a block: {}", s.text);

        let v = repo.stop_blocks();
        let entry = find_key(&v, &repo.sid).expect("session entry in stop-blocks.json");
        assert_eq!(
            entry.get("count").and_then(|c| c.as_u64()),
            Some(1),
            "entry: {}",
            entry
        );
        assert!(
            entry.to_string().contains("uncommitted:src/b.rs"),
            "entry records the new keys: {}",
            entry
        );
    }

    /// S20: A count from an older hexa is read as a count with no keys
    #[test]
    fn s20_bare_number_count_is_read() {
        let repo = Repo::new(false);
        repo.add_task("Step one");
        let mut old = serde_json::Map::new();
        old.insert(repo.sid.clone(), json!(2));
        std::fs::write(
            repo.dir.join(".hexa/stop-blocks.json"),
            Value::Object(old).to_string(),
        )
        .unwrap();

        let s = repo.stop(false);
        assert!(s.blocked, "open work blocks: {}", s.text);

        let v = repo.stop_blocks();
        let entry = find_key(&v, &repo.sid).expect("session entry in stop-blocks.json");
        assert!(
            entry.is_object(),
            "entry now holds a count and keys: {}",
            entry
        );
        assert!(
            entry.get("count").and_then(|c| c.as_u64()).is_some(),
            "entry has a count: {}",
            entry
        );
        assert!(
            entry.to_string().contains("task:Step one"),
            "entry has the keys blocked on: {}",
            entry
        );
    }
}

mod part_3 {
    #![allow(dead_code, unused_imports, unused_variables)]

    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use hexa_cli::commands::finish;
    use hexa_cli::commands::land;
    use hexa_cli::commands::loop_cmd;
    use hexa_cli::commands::task_ledger;

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn fake_home() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!(
            "hexa-adr-2611220204-home-{}-{}",
            std::process::id(),
            n
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    fn repo() -> (tempfile::TempDir, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().canonicalize().unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("README.md"), "readme\n").unwrap();
        git(&dir, &["add", "README.md"]);
        git(&dir, &["commit", "-q", "-m", "initial"]);
        (t, dir)
    }

    fn hexa(dir: &Path, args: &[&str], stdin: &str) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_hexa"))
            .current_dir(dir)
            .args(args)
            .env("HOME", fake_home())
            .env("CLAUDE_PROJECT_DIR", dir)
            .env("NO_COLOR", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("hexa runs");
        {
            let mut input = child.stdin.take().unwrap();
            let _ = input.write_all(stdin.as_bytes());
        }
        child.wait_with_output().expect("hexa finishes")
    }

    fn text_of(out: &Output) -> String {
        format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    }

    struct StopResult {
        blocked: bool,
        text: String,
    }

    fn is_block_json(s: &str) -> bool {
        match serde_json::from_str::<serde_json::Value>(s.trim()) {
            Ok(v) => v.get("decision").and_then(|d| d.as_str()) == Some("block"),
            Err(_) => false,
        }
    }

    fn stop(dir: &Path, sid: &str) -> StopResult {
        let payload = serde_json::json!({
            "session_id": sid,
            "hook_event_name": "Stop",
            "cwd": dir,
            "stop_hook_active": false,
        });
        let out = hexa(dir, &["hook", "stop"], &payload.to_string());
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let mut blocked = out.status.code() == Some(2);
        if is_block_json(&stdout) || stdout.lines().any(is_block_json) {
            blocked = true;
        }
        StopResult {
            blocked,
            text: text_of(&out),
        }
    }

    fn open_file(dir: &Path, name: &str) {
        let p = dir.join(name);
        std::fs::write(&p, format!("content of {name}\n")).unwrap();
        loop_cmd::touch(dir, p.to_str().unwrap()).expect("touch records the file");
    }

    fn session_with_file(dir: &Path, name: &str) -> String {
        loop_cmd::ensure_state_dir(dir).expect("state dir");
        let sid = loop_cmd::session_id();
        open_file(dir, name);
        assert!(
            !finish::session_open(dir, &sid).blocking.is_empty(),
            "precondition: the session has open blocking work"
        );
        sid
    }

    fn blocking_keys(dir: &Path, sid: &str) -> Vec<String> {
        let mut keys: Vec<String> = finish::session_open(dir, sid)
            .blocking
            .iter()
            .map(|i| i.key.clone())
            .collect();
        keys.sort();
        keys
    }

    fn stop_blocks_path(dir: &Path) -> PathBuf {
        dir.join(".hexa").join("stop-blocks.json")
    }

    fn gitignore_lines(dir: &Path) -> Vec<String> {
        std::fs::read_to_string(dir.join(".hexa").join(".gitignore"))
            .unwrap_or_default()
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect()
    }

    fn line_names(line: &str, name: &str) -> bool {
        let l = line.trim().trim_start_matches('/');
        l == name || l.ends_with(&format!("/{name}"))
    }

    fn ledger_name(dir: &Path) -> String {
        task_ledger::ledger_path(dir)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string()
    }

    fn untracked(dir: &Path) -> String {
        git(dir, &["status", "--porcelain", "--untracked-files=all"])
    }

    // ---------- landing hints ----------

    fn forced_merge(t: &str) -> bool {
        let re = regex::Regex::new(r"merge[^\n]*--force").unwrap();
        (t.contains("hexa dev worktree merge") && t.contains("--force")) || re.is_match(t)
    }

    fn ff_landing(t: &str, branch: &str) -> bool {
        let re = regex::Regex::new(&format!(
            r"git\s+(?:-C\s+\S+\s+)?merge\s+--ff-only\s+{}(?:$|[^\w/.-])",
            regex::escape(branch)
        ))
        .unwrap();
        re.is_match(t)
    }

    fn merge_regate_ff(t: &str, base: &str) -> bool {
        let lower = t.to_lowercase();
        let re = regex::Regex::new(&format!(
            r"merge\s+(?:the\s+)?(?:origin/)?{}\b",
            regex::escape(base)
        ))
        .unwrap();
        let merges_base =
            re.is_match(&lower) || (lower.contains("merge the base") && lower.contains("into"));
        merges_base
            && lower.contains("gate")
            && (lower.contains("fast-forward")
                || lower.contains("fast forward")
                || lower.contains("ff-only"))
    }

    fn keeps_commits(t: &str, branch: &str, base: &str) -> bool {
        ff_landing(t, branch) || merge_regate_ff(t, base)
    }

    /// A repository with a worktree on `branch` carrying one commit. When
    /// `moved`, main gains a commit after the branch was made.
    fn unlanded_repo(
        branch: &str,
        moved: bool,
    ) -> (tempfile::TempDir, tempfile::TempDir, PathBuf, PathBuf) {
        let (t, dir) = repo();
        let w = tempfile::tempdir().unwrap();
        let wt = w.path().canonicalize().unwrap().join("wt");
        git(
            &dir,
            &["worktree", "add", "-q", "-b", branch, wt.to_str().unwrap()],
        );
        std::fs::write(wt.join("feature.txt"), "feature\n").unwrap();
        git(&wt, &["add", "feature.txt"]);
        git(&wt, &["commit", "-q", "-m", "feature work"]);
        if moved {
            std::fs::write(dir.join("other.txt"), "base moved\n").unwrap();
            git(&dir, &["add", "other.txt"]);
            git(&dir, &["commit", "-q", "-m", "base moves"]);
        }
        (t, w, dir, wt)
    }

    fn unlanded_items(dir: &Path, wt: &Path, branch: &str) -> Vec<finish::Item> {
        let mut all = Vec::new();
        all.extend(finish::open_work(dir));
        all.extend(finish::open_work(wt));
        all.extend(finish::not_on_main(dir));
        all.extend(finish::not_on_main(wt));
        let mut out: Vec<finish::Item> = Vec::new();
        for i in all {
            if i.kind == "unlanded"
                && (i.what.contains(branch) || i.key.contains(branch))
                && !out.contains(&i)
            {
                out.push(i);
            }
        }
        out
    }

    fn conflict_landing_why(branch: &str) -> String {
        let (t, dir) = repo();
        std::fs::write(dir.join("c.txt"), "base\n").unwrap();
        git(&dir, &["add", "c.txt"]);
        git(&dir, &["commit", "-q", "-m", "c"]);
        let w = tempfile::tempdir().unwrap();
        let wt = w.path().canonicalize().unwrap().join("wt");
        git(
            &dir,
            &["worktree", "add", "-q", "-b", branch, wt.to_str().unwrap()],
        );
        std::fs::write(wt.join("c.txt"), "branch side\n").unwrap();
        git(&wt, &["add", "c.txt"]);
        git(&wt, &["commit", "-q", "-m", "branch edits c"]);
        std::fs::write(dir.join("c.txt"), "main side\n").unwrap();
        git(&dir, &["add", "c.txt"]);
        git(&dir, &["commit", "-q", "-m", "main edits c"]);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let landing = rt.block_on(land::land(&wt, "true", true, true));
        match landing {
            land::Landing::NotLanded { why, .. } => why,
            other => panic!("a conflicting rebase must not land: {:?}", other),
        }
    }

    fn subagent_stop_text(dir: &Path, wt: &Path) -> String {
        loop_cmd::ensure_state_dir(dir).expect("state dir");
        let sid = loop_cmd::session_id();
        let _ = std::fs::write(wt.join("loose.txt"), "loose\n");
        let payload = serde_json::json!({
            "session_id": sid,
            "hook_event_name": "SubagentStop",
            "cwd": wt,
            "agent_id": "agent-1",
            "stop_hook_active": false,
        });
        let out = hexa(wt, &["hook", "subagent-stop"], &payload.to_string());
        text_of(&out)
    }

    fn merge_order_text(dir: &Path) -> String {
        let out = hexa(dir, &["worktree", "merge-order"], "");
        let mut t = text_of(&out);
        let out2 = hexa(dir, &["dev", "worktree", "merge-order"], "");
        t.push_str(&text_of(&out2));
        t
    }

    fn doctor_texts(dir: &Path) -> Vec<String> {
        let adrs = dir.join("docs").join("adrs");
        std::fs::create_dir_all(&adrs).unwrap();
        let body = "# ADR-2001010000: Old\n\n**Status:** Proposed\n**Date:** 2001-01-01\n**Depends on:** ADR-1999010100\n\n## Context\n\nx\n";
        let path = adrs.join("ADR-2001010000-old.md");
        std::fs::write(&path, body).unwrap();
        let mut texts: Vec<String> = hexa_cli::commands::adr::doctor::scan(
            &[(path.clone(), body.to_string())],
            chrono::NaiveDate::from_ymd_opt(2026, 11, 22).unwrap(),
        )
        .into_iter()
        .map(|f| f.detail)
        .collect();
        texts.push(text_of(&hexa(dir, &["adr", "doctor"], "")));
        texts
    }

    /// S21: A let-through does not reset the bound into a cycle
    #[test]
    fn s21_let_through_does_not_reset_bound_into_cycle() {
        let (_t, dir) = repo();
        let sid = session_with_file(&dir, "a.txt");
        let results: Vec<StopResult> = (0..8).map(|_| stop(&dir, &sid)).collect();
        for (i, r) in results.iter().enumerate().take(3) {
            assert!(r.blocked, "stop {} should be blocked: {}", i + 1, r.text);
        }
        for (i, r) in results.iter().enumerate().skip(3) {
            assert!(
                !r.blocked,
                "stop {} is after three blocks on the same keys and must be let through: {}",
                i + 1,
                r.text
            );
            assert!(
                r.text.contains("a.txt"),
                "stop {} prints the list: {}",
                i + 1,
                r.text
            );
        }
    }

    /// S22: The session's entry is removed when nothing is open
    #[test]
    fn s22_entry_removed_when_nothing_open() {
        let (_t, dir) = repo();
        let sid = session_with_file(&dir, "a.txt");
        let first = stop(&dir, &sid);
        assert!(first.blocked, "open work blocks: {}", first.text);
        let before = std::fs::read_to_string(stop_blocks_path(&dir)).expect("a count was written");
        assert!(before.contains(&sid), "the session has an entry: {before}");

        git(&dir, &["add", "a.txt"]);
        git(&dir, &["commit", "-q", "-m", "close a"]);
        assert!(
            blocking_keys(&dir, &sid).is_empty(),
            "precondition: nothing open"
        );

        let r = stop(&dir, &sid);
        assert!(
            !r.blocked,
            "nothing open, the stop goes through: {}",
            r.text
        );
        let after = std::fs::read_to_string(stop_blocks_path(&dir)).unwrap_or_default();
        if !after.trim().is_empty() {
            let v: serde_json::Value =
                serde_json::from_str(&after).expect("stop-blocks.json is JSON");
            assert!(
                !v.to_string().contains(&sid),
                "the session's entry is removed: {after}"
            );
        }
    }

    #[derive(Clone, Copy)]
    enum Act {
        Nothing,
        Touch(&'static str),
        Edit(&'static str),
        Commit(&'static str),
    }

    /// S23: No stop is blocked more than three times in a row on the same keys
    #[test]
    fn s23_never_more_than_three_blocks_on_same_keys() {
        let (_t, dir) = repo();
        let sid = session_with_file(&dir, "a.txt");
        use Act::*;
        let steps = [
            Nothing,
            Nothing,
            Nothing,
            Nothing,
            Edit("a.txt"),
            Nothing,
            Nothing,
            Touch("b.txt"),
            Nothing,
            Nothing,
            Nothing,
            Nothing,
            Commit("b.txt"),
            Nothing,
            Nothing,
            Nothing,
            Commit("a.txt"),
            Touch("c.txt"),
            Edit("c.txt"),
            Nothing,
            Nothing,
            Nothing,
        ];
        let mut prev: Option<Vec<String>> = None;
        let mut run_index = 0usize;
        for (n, act) in steps.iter().enumerate() {
            match *act {
                Nothing => {}
                Touch(f) => open_file(&dir, f),
                Edit(f) => {
                    let p = dir.join(f);
                    let mut s = std::fs::read_to_string(&p).unwrap_or_default();
                    s.push_str("more\n");
                    std::fs::write(&p, s).unwrap();
                }
                Commit(f) => {
                    git(&dir, &["add", f]);
                    git(&dir, &["commit", "-q", "-m", &format!("commit {f}")]);
                }
            }
            let keys = blocking_keys(&dir, &sid);
            if prev.as_ref() == Some(&keys) {
                run_index += 1;
            } else {
                run_index = 0;
            }
            let r = stop(&dir, &sid);
            if keys.is_empty() {
                assert!(
                    !r.blocked,
                    "step {n}: nothing open, nothing blocks: {}",
                    r.text
                );
            } else if run_index >= 3 {
                assert!(
                    !r.blocked,
                    "step {n}: stop {} in a run on keys {:?} must be let through: {}",
                    run_index + 1,
                    keys,
                    r.text
                );
                let listed = keys.iter().any(|k| {
                    let name = k.rsplit('/').next().unwrap_or(k);
                    let name = name.trim_start_matches("uncommitted:");
                    r.text.contains(name)
                });
                assert!(
                    listed,
                    "step {n}: the let-through prints the list: {}",
                    r.text
                );
            }
            prev = Some(keys);
        }
    }

    /// S24: The unlanded hint names a landing that keeps the branch's commits
    #[test]
    fn s24_unlanded_hint_names_ff_only_merge() {
        let (_t, _w, dir, wt) = unlanded_repo("feat/x", false);
        let items = unlanded_items(&dir, &wt, "feat/x");
        assert!(!items.is_empty(), "feat/x is listed as unlanded");
        for i in &items {
            assert!(
                ff_landing(&i.fix, "feat/x"),
                "the fix names `git merge --ff-only feat/x`: {}",
                i.fix
            );
            assert!(
                i.fix.contains("main") || i.fix.contains(dir.to_str().unwrap()),
                "the fix says to run it from the base's checkout: {}",
                i.fix
            );
            assert!(!forced_merge(&i.fix), "no forced merge: {}", i.fix);
        }
    }

    /// S25: When the base has moved the hint names merge, re-gate, fast-forward
    #[test]
    fn s25_moved_base_hint_names_merge_regate_fast_forward() {
        let (_t, _w, dir, wt) = unlanded_repo("feat/x", true);
        let items = unlanded_items(&dir, &wt, "feat/x");
        assert!(!items.is_empty(), "feat/x is listed as unlanded");
        for i in &items {
            assert!(
                merge_regate_ff(&i.fix, "main"),
                "the fix says merge main into the branch, re-run the gate, fast-forward: {}",
                i.fix
            );
            assert!(!forced_merge(&i.fix), "no forced merge: {}", i.fix);
        }
    }

    /// S26: No hint names a forced worktree merge
    #[test]
    fn s26_no_hint_names_forced_worktree_merge() {
        let mut hints: Vec<(String, String)> = Vec::new();

        let (_t, _w, dir, wt) = unlanded_repo("feat/x", false);
        let items = unlanded_items(&dir, &wt, "feat/x");
        assert!(!items.is_empty(), "an unlanded item exists");
        for i in items {
            hints.push(("unlanded".into(), i.fix));
        }
        hints.push(("land conflict".into(), conflict_landing_why("feat/x")));
        hints.push(("merge-order".into(), merge_order_text(&dir)));
        hints.push(("subagent-stop".into(), subagent_stop_text(&dir, &wt)));
        for d in doctor_texts(&dir) {
            hints.push(("adr doctor".into(), d));
        }

        for (what, text) in &hints {
            assert!(
                !(text.contains("hexa dev worktree merge") && text.contains("--force")),
                "{what} hint names a forced worktree merge: {text}"
            );
        }
    }

    /// S27: Every landing hint keeps the branch's commits
    #[test]
    fn s27_every_landing_hint_keeps_branch_commits() {
        for branch in ["feat/x", "feat/x-2", "fix/y"] {
            for moved in [false, true] {
                let (_t, _w, dir, wt) = unlanded_repo(branch, moved);
                let items = unlanded_items(&dir, &wt, branch);
                assert!(!items.is_empty(), "{branch} (moved={moved}) is unlanded");
                for i in &items {
                    assert!(
                        !forced_merge(&i.fix),
                        "{branch} moved={moved}: forced merge: {}",
                        i.fix
                    );
                    assert!(
                        keeps_commits(&i.fix, branch, "main"),
                        "{branch} moved={moved}: the fix names a landing that keeps commits: {}",
                        i.fix
                    );
                }
                let sa = subagent_stop_text(&dir, &wt);
                assert!(
                    !forced_merge(&sa),
                    "{branch} moved={moved}: subagent-stop: {sa}"
                );
                let mo = merge_order_text(&dir);
                assert!(
                    !forced_merge(&mo),
                    "{branch} moved={moved}: merge-order: {mo}"
                );
            }
            let why = conflict_landing_why(branch);
            assert!(
                !forced_merge(&why),
                "{branch}: land conflict names a forced merge: {why}"
            );
            assert!(
                keeps_commits(&why, branch, "main"),
                "{branch}: land conflict names a landing that keeps commits: {why}"
            );
        }
        let (_t, dir) = repo();
        for d in doctor_texts(&dir) {
            assert!(!forced_merge(&d), "adr doctor names a forced merge: {d}");
        }
    }

    /// S28: `hexa init` ignores stop-blocks.json
    #[test]
    fn s28_init_ignores_stop_blocks() {
        let (_t, dir) = repo();
        let out = hexa(&dir, &["init"], "");
        assert!(
            out.status.success(),
            "hexa init succeeds: {}",
            text_of(&out)
        );
        let lines = gitignore_lines(&dir);
        assert!(
            lines.iter().any(|l| line_names(l, "stop-blocks.json")),
            ".hexa/.gitignore lists stop-blocks.json: {lines:?}"
        );
        let ledger = ledger_name(&dir);
        assert!(
            lines.iter().any(|l| line_names(l, &ledger)),
            ".hexa/.gitignore keeps the ledger's entry {ledger}: {lines:?}"
        );
        std::fs::write(stop_blocks_path(&dir), "{}\n").unwrap();
        let status = untracked(&dir);
        assert!(
            !status.contains("stop-blocks.json"),
            "git status does not show stop-blocks.json: {status}"
        );
    }

    /// S29: The Stop hook covers a project that never re-ran init
    #[test]
    fn s29_stop_hook_covers_project_without_reinit() {
        let (_t, dir) = repo();
        std::fs::create_dir_all(dir.join(".hexa")).unwrap();
        let ledger = ledger_name(&dir);
        std::fs::write(
            dir.join(".hexa").join(".gitignore"),
            format!("{ledger}\n{ledger}.lock\n"),
        )
        .unwrap();
        git(&dir, &["add", ".hexa/.gitignore"]);
        git(&dir, &["commit", "-q", "-m", "older hexa"]);

        let sid = session_with_file(&dir, "a.txt");
        let r = stop(&dir, &sid);
        assert!(r.blocked, "open work blocks and writes a count: {}", r.text);
        assert!(stop_blocks_path(&dir).exists(), "a count was written");

        let lines = gitignore_lines(&dir);
        assert!(
            lines.iter().any(|l| line_names(l, "stop-blocks.json")),
            ".hexa/.gitignore now lists stop-blocks.json: {lines:?}"
        );
        let status = untracked(&dir);
        assert!(
            !status.contains("stop-blocks.json"),
            "git status shows no untracked stop-blocks.json: {status}"
        );
    }

    /// S30: Ignoring the stop count does not duplicate or drop the ledger's entries
    #[test]
    fn s30_ignore_does_not_duplicate_or_drop_entries() {
        let (_t, dir) = repo();
        std::fs::create_dir_all(dir.join(".hexa")).unwrap();
        task_ledger::ensure_ignored(&dir).expect("ensure_ignored");
        let gi = dir.join(".hexa").join(".gitignore");
        let mut content = std::fs::read_to_string(&gi).unwrap_or_default();
        if !content.lines().any(|l| line_names(l, "stop-blocks.json")) {
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str("stop-blocks.json\n");
            std::fs::write(&gi, &content).unwrap();
        }
        let original = gitignore_lines(&dir);
        let ledger = ledger_name(&dir);
        assert!(
            original.iter().any(|l| line_names(l, &ledger)),
            "precondition: the ledger is listed: {original:?}"
        );

        // init runs again on an initialised project: that is `init --force`.
        let out = hexa(&dir, &["init", "--force"], "");
        assert!(
            out.status.success(),
            "hexa init succeeds: {}",
            text_of(&out)
        );

        let sid = session_with_file(&dir, "a.txt");
        let r = stop(&dir, &sid);
        assert!(r.blocked, "open work blocks and writes a count: {}", r.text);
        assert!(stop_blocks_path(&dir).exists(), "a count was written");

        let lines = gitignore_lines(&dir);
        let count = lines
            .iter()
            .filter(|l| line_names(l, "stop-blocks.json"))
            .count();
        assert_eq!(count, 1, "stop-blocks.json appears exactly once: {lines:?}");
        for l in &original {
            assert!(lines.contains(l), "entry {l:?} is still present: {lines:?}");
        }
    }
}
