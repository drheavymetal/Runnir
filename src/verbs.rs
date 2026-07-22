//! The real verbs of a repository: what is actually typed here, learned from use.
//!
//! Every project has five or six commands it is really worked with. They are not the
//! aliases someone defined, and they are not the README nobody updated — they are
//! what people type. runnir sees that, so it can offer it: open a repo and the window
//! already knows how it is built, tested and deployed.
//!
//! **Arguments are never stored.** `curl -H "Authorization: Bearer …"` and
//! `scp ~/clients/acme/dump.sql host:` are commands people run; the head is the verb,
//! the tail is private. That line is enforced HERE, at capture, not at display —
//! anything else means the secret is already on disk by the time someone thinks about
//! it. This is the difference between a useful feature and a leak.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Below this many runs, a command is not a habit. Two runs of something is an
/// experiment; the point of this feature is to show what the project is worked with.
pub const DEFAULT_THRESHOLD: u32 = 3;

/// Tools whose first argument is a SUBCOMMAND rather than an argument, so the verb is
/// two words: `cargo build`, not `cargo`.
///
/// A list rather than a heuristic on purpose. The heuristic ("a bare word is a
/// subcommand") turns `python train.py` into the verb `python train.py` and
/// `ssh cloudmax` into `ssh cloudmax` — a hostname is not a verb, and a filename is
/// somebody's directory layout.
const SUBCOMMAND_TOOLS: &[&str] = &[
    "cargo", "git", "npm", "pnpm", "yarn", "bun", "deno", "go", "docker", "podman",
    "kubectl", "systemctl", "brew", "apt", "pacman", "dnf", "pip", "uv", "poetry",
    "composer", "gradle", "mvn", "dotnet", "terraform", "gh", "flatpak", "make",
    "just", "task", "wrangler", "flyctl", "heroku", "aws", "gcloud", "az", "nix",
];

/// Shell noise: navigation and looking-around. These are how anyone uses any
/// directory, so they say nothing about how THIS project is built, tested or
/// deployed — and a list whose top entry is `cd` teaches a newcomer nothing.
const NOISE: &[&str] = &[
    "cd", "ls", "ll", "la", "pwd", "clear", "exit", "cat", "bat", "less", "more",
    "echo", "which", "type", "man", "history", "export", "source", "tree", "head",
    "tail", "wc", "touch", "mkdir", "cp", "mv", "rm", "chmod", "chown", "sudo",
];

/// The verb of a command line, or `None` when there is nothing worth learning.
///
/// Returns at most two words, and only words that cannot carry private data: a
/// subcommand from a known tool's fixed vocabulary. Everything else — paths, hosts,
/// flags, URLs, tokens — is dropped before it can be written anywhere. A line whose
/// shape cannot be read with certainty yields nothing at all, which is always the
/// cheaper mistake.
pub fn verb_of(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    // A pipeline, a chain or a redirection is not one verb: take the first stage,
    // so `cargo build && ./run` is a `cargo build` habit. Where the stage ends and
    // where each word ends are the same question — quotes decide both — so one pass
    // answers them together. Splitting on whitespace instead is what turns
    // `AUTH="Bearer sk-live-abc123" curl …` into the verb `sk-live-abc123`.
    let stage = first_stage_words(line)?;
    let mut words = stage.iter().map(String::as_str);
    let head = words.next()?;

    // An assignment prefix (`RUST_LOG=debug cargo test`) is environment, not a verb —
    // and there can be any number of them. Stepping over only the first is how
    // `RUST_LOG=debug OPENAI_API_KEY=sk-… cargo run` gets learned as the key itself,
    // written to disk under the repo's name.
    let mut head = head;
    while head.contains('=') {
        head = words.next()?;
    }
    // A path to something in the project is not a shared verb: `./deploy.sh` says as
    // much about someone's directory as about the project, and `/home/pedro/x` more.
    if head.starts_with('-') || head.contains('/') || head.starts_with('$') {
        return None;
    }
    // A command name is one word. Anything that is only one word because a quote held
    // it together is somebody's data — a password, a message, a query — sitting where
    // the verb should be.
    if head.contains(char::is_whitespace) {
        return None;
    }
    let head = head.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_');
    if head.is_empty() {
        return None;
    }

    if NOISE.contains(&head) {
        return None;
    }
    if SUBCOMMAND_TOOLS.contains(&head) {
        // The subcommand only counts when it looks like one: a bare word from the
        // tool's own vocabulary. `docker compose` yes; `docker /var/run/x.sock` no.
        if let Some(sub) = words.next() {
            if is_subcommand(sub) {
                return Some(format!("{head} {sub}"));
            }
        }
    }
    Some(head.to_string())
}

/// The words of a command line's FIRST stage, split the way a shell splits them: a
/// quoted run is one word however many spaces it holds, a backslash escapes the
/// character after it, and an unquoted `|`, `;` or `&&` ends the stage.
///
/// `None` when a quote never closes. A line that cannot be taken apart with certainty
/// is one nothing is learned from, because the only alternative is to guess where the
/// value in `AUTH="Bearer sk-live-abc123" curl …` ends — and a wrong guess writes the
/// tail of somebody's token to disk as this repository's verb. A verb missed costs a
/// line in a panel nobody notices; a verb invented out of a secret cannot be recalled.
fn first_stage_words(line: &str) -> Option<Vec<String>> {
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    // Tracked apart from `word.is_empty()`: `FOO=""` is a word, and an empty one.
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            // Inside single quotes everything is literal, backslash included — which
            // is exactly why people put secrets in them.
            Some('\'') => {
                if c == '\'' {
                    quote = None;
                } else {
                    word.push(c);
                }
            }
            Some(q) => {
                if c == '\\' {
                    if let Some(next) = chars.next() {
                        word.push(next);
                    }
                } else if c == q {
                    quote = None;
                } else {
                    word.push(c);
                }
            }
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    in_word = true;
                }
                '\\' => {
                    if let Some(next) = chars.next() {
                        word.push(next);
                        in_word = true;
                    }
                }
                '|' | ';' | '\n' => break,
                '&' if chars.peek() == Some(&'&') => break,
                c if c.is_whitespace() => {
                    if in_word {
                        words.push(std::mem::take(&mut word));
                        in_word = false;
                    }
                }
                c => {
                    word.push(c);
                    in_word = true;
                }
            },
        }
    }
    if quote.is_some() {
        return None;
    }
    if in_word {
        words.push(word);
    }
    Some(words)
}

/// Whether a word can be a subcommand: letters, digits and dashes only, short, not a
/// flag, not a path, not a file with an extension.
fn is_subcommand(w: &str) -> bool {
    !w.is_empty()
        && w.len() <= 20
        && !w.starts_with('-')
        && !w.contains('/')
        && !w.contains('.')
        && !w.contains('=')
        && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Counts per repository. Serialised to runnir's data directory — never inside the
/// repo, where a `.runnir-verbs` file would get committed and publish somebody's
/// shell habits to the team.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Verbs {
    /// repo root -> verb -> times it succeeded.
    repos: HashMap<String, HashMap<String, u32>>,
}

impl Verbs {
    /// Records one SUCCESSFUL run. Failures are deliberately not counted: a verb
    /// learned from what does not work teaches the wrong thing to whoever reads it.
    pub fn record(&mut self, repo: &Path, line: &str) -> Option<String> {
        let verb = verb_of(line)?;
        let entry = self.repos.entry(repo.to_string_lossy().into_owned()).or_default();
        *entry.entry(verb.clone()).or_insert(0) += 1;
        Some(verb)
    }

    /// The verbs this repo is worked with, most used first, above the threshold.
    /// Ties break alphabetically so the list does not shuffle between openings.
    pub fn top(&self, repo: &Path, threshold: u32, limit: usize) -> Vec<(String, u32)> {
        let Some(counts) = self.repos.get(&repo.to_string_lossy().into_owned()) else {
            return Vec::new();
        };
        let mut v: Vec<(String, u32)> =
            counts.iter().filter(|(_, n)| **n >= threshold).map(|(k, n)| (k.clone(), *n)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.truncate(limit);
        v
    }

    pub fn path() -> Option<PathBuf> {
        Some(dirs::data_dir()?.join("runnir/verbs.json"))
    }

    pub fn load() -> Self {
        let Some(p) = Self::path() else { return Self::default() };
        std::fs::read_to_string(p)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Best-effort save. A lost count is not worth an error in the user's face.
    pub fn save(&self) {
        let Some(p) = Self::path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, text);
        }
    }

    /// Forgets everything about one repo, for when a project's history is nobody
    /// else's business any more.
    pub fn forget(&mut self, repo: &Path) {
        self.repos.remove(&repo.to_string_lossy().into_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule that makes this feature shareable instead of a leak: whatever the
    /// command carried, only the verb survives.
    #[test]
    fn arguments_never_survive_capture() {
        for line in [
            "curl -H \"Authorization: Bearer sk-live-abc123\" https://api.example.com",
            "scp ~/clients/acme/dump.sql root@10.0.0.4:/tmp/",
            "psql postgres://user:hunter2@db.internal/prod",
            "ssh cloudmax",
        ] {
            let verb = verb_of(line).unwrap();
            assert!(
                !verb.contains("sk-live") && !verb.contains("acme") && !verb.contains("hunter2"),
                "{line} leaked through as {verb}"
            );
            assert!(verb.split_whitespace().count() <= 2, "{verb} is not a verb");
        }
        assert_eq!(verb_of("ssh cloudmax").unwrap(), "ssh", "a hostname is not a subcommand");
    }


    /// A command line can carry as many assignments as it likes before the verb, and
    /// the second one holds a secret as readily as the first.
    #[test]
    fn every_env_assignment_is_stepped_over_not_just_the_first() {
        assert_eq!(
            verb_of("RUST_LOG=debug OPENAI_API_KEY=sk-live-abc123 cargo run").unwrap(),
            "cargo run"
        );
        assert_eq!(
            verb_of("AWS_PROFILE=prod AWS_SECRET_ACCESS_KEY=wJal DEBUG=1 terraform apply").unwrap(),
            "terraform apply"
        );
        // Assignments and nothing else is an environment change, not a verb at all.
        assert!(verb_of("OPENAI_API_KEY=sk-live-abc123").is_none());
        assert!(verb_of("A=1 B=2").is_none());
    }

    /// A value in quotes holds spaces, so whitespace alone cannot say where it ends.
    /// The second half of `AUTH="Bearer sk-live-…"` is not this repository's verb.
    #[test]
    fn a_quoted_value_never_leaks_its_tail_as_the_verb() {
        assert_eq!(
            verb_of("AUTH=\"Bearer sk-live-abc123\" curl -s https://api.example.com").unwrap(),
            "curl"
        );
        assert_eq!(verb_of("TOKEN='super secret value' terraform apply").unwrap(), "terraform apply");
        assert_eq!(verb_of("PASS=hunter\\ two psql").unwrap(), "psql");
        // A separator inside the quotes belongs to the value, not to the line.
        assert_eq!(verb_of("TOKEN='a|b;c && d' cargo test").unwrap(), "cargo test");
        // A quote that never closes cannot be taken apart at all: learn nothing rather
        // than guess where the secret ends.
        assert!(verb_of("AUTH=\"Bearer sk-live-abc123 curl").is_none());
        // …and a word that is only one word because a quote held it together is data
        // sitting where the command name should be.
        assert!(verb_of("'super secret value'").is_none());
    }

    /// A list whose top entry is `cd` teaches a newcomer nothing: navigation is how
    /// anyone uses any directory, not how this project is worked.
    #[test]
    fn navigation_and_looking_around_are_not_verbs() {
        for line in ["cd ~/projects/runnir", "ls -la", "cat README.md", "clear", "sudo pacman -Syu"] {
            assert!(verb_of(line).is_none(), "{line} was learned as a verb");
        }
        // …while the real verbs still are.
        assert_eq!(verb_of("cargo test").unwrap(), "cargo test");
    }

    /// A tool's subcommand IS the verb — `cargo` alone says nothing about whether
    /// this repo is built, tested or published here.
    #[test]
    fn a_known_tool_keeps_its_subcommand() {
        assert_eq!(verb_of("cargo build --release").unwrap(), "cargo build");
        assert_eq!(verb_of("git push origin main").unwrap(), "git push");
        assert_eq!(verb_of("npm run deploy").unwrap(), "npm run");
        assert_eq!(verb_of("docker compose up -d").unwrap(), "docker compose");
    }

    /// …but only when the next word is really a subcommand. A filename is somebody's
    /// directory layout and a path may be private.
    #[test]
    fn a_filename_or_path_is_not_a_subcommand() {
        assert_eq!(verb_of("python train.py --epochs 40").unwrap(), "python");
        assert_eq!(verb_of("docker /var/run/docker.sock").unwrap(), "docker");
        assert_eq!(verb_of("make -j8").unwrap(), "make");
    }

    /// Scripts and paths are not shared verbs: `./deploy.sh` describes a directory as
    /// much as a project, and an absolute path describes a machine.
    #[test]
    fn paths_and_flags_are_not_verbs() {
        assert!(verb_of("./deploy.sh prod").is_none());
        assert!(verb_of("/usr/local/bin/thing").is_none());
        assert!(verb_of("--help").is_none());
        assert!(verb_of("   ").is_none());
    }

    /// A pipeline is one habit, not a new verb per stage.
    #[test]
    fn a_pipeline_counts_as_its_first_stage() {
        assert_eq!(verb_of("cargo test 2>&1 | rg FAIL").unwrap(), "cargo test");
        assert_eq!(verb_of("cargo build && ./target/release/runnir").unwrap(), "cargo build");
        assert_eq!(verb_of("RUST_LOG=debug cargo run").unwrap(), "cargo run");
    }

    /// Two runs is an experiment. The threshold is what separates a verb from a
    /// thing somebody tried once.
    #[test]
    fn a_command_run_twice_is_not_yet_a_verb() {
        let repo = Path::new("/r");
        let mut v = Verbs::default();
        v.record(repo, "cargo build");
        v.record(repo, "cargo build");
        assert!(v.top(repo, DEFAULT_THRESHOLD, 5).is_empty());
        v.record(repo, "cargo build --release");
        assert_eq!(v.top(repo, DEFAULT_THRESHOLD, 5), vec![("cargo build".to_string(), 3)]);
    }

    /// Most used first, ties alphabetical — a list that reorders between openings is
    /// one nobody trusts.
    #[test]
    fn the_list_is_ordered_and_stable() {
        let repo = Path::new("/r");
        let mut v = Verbs::default();
        for _ in 0..5 {
            v.record(repo, "cargo test");
        }
        for _ in 0..3 {
            v.record(repo, "git push");
        }
        for _ in 0..3 {
            v.record(repo, "cargo build");
        }
        let top = v.top(repo, 3, 10);
        assert_eq!(
            top,
            vec![
                ("cargo test".to_string(), 5),
                ("cargo build".to_string(), 3),
                ("git push".to_string(), 3),
            ]
        );
    }

    /// Repos do not bleed into each other, and one can be forgotten on its own.
    #[test]
    fn repos_are_separate_and_forgettable() {
        let (a, b) = (Path::new("/a"), Path::new("/b"));
        let mut v = Verbs::default();
        for _ in 0..3 {
            v.record(a, "cargo test");
            v.record(b, "npm test");
        }
        assert_eq!(v.top(a, 3, 5).len(), 1);
        v.forget(a);
        assert!(v.top(a, 3, 5).is_empty());
        assert_eq!(v.top(b, 3, 5).len(), 1, "forgetting one repo leaves the others alone");
    }
}
