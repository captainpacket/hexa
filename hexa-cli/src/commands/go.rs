//! `hexa go` — the next step, and only that.
//!
//! It prints the `Next:` part of `hexa status`: the recorded design draft's
//! step, else record a gate, fix the red gate, the pending task, or the next
//! stage. It reads the loop and this repository's run records and starts no
//! process of its own.
//!
//! It used to be a self-check of the hexa repository run in every project:
//! it said "no release binary — build with cargo" anywhere, started a
//! detached release build in the hexa repo without being asked, ran
//! `cargo test --no-run`, and then reported "a rebuild is in flight" when
//! nothing was building. None of that was about the project's next step, and
//! none of it is here. Building hexa is `cargo build -p hexa-cli --release`;
//! checking it is `hexa dev ci`.

use colored::Colorize;

pub async fn run() -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    println!("{} hexa go\n", "\u{2b21}".cyan());
    print!("{}", super::status::next_text(&cwd));
    Ok(())
}
