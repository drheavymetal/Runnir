//! The catch-up: one headline per pane, for coming back after a while away.
//!
//! Six panes of scrollback is not a status report — reconstructing it costs more
//! attention than the answer is worth. This turns each pane into a single line and
//! ranks them, so the first line you read is the one that wants you.
//!
//! Everything here is pure: a snapshot of facts in, a headline out. The facts come
//! from what OSC 133 already records (a command's exit code, whether one is running,
//! how long it took) plus what the app knows (a pane blocked on a confirmation, a
//! keyword watch that hit). Keeping it pure is what makes the ranking testable
//! without a window, a PTY or a shell.

/// What a pane is doing, in the order that decides which line you read first.
///
/// The order is the whole point: something WAITING on you outranks a failure,
/// because it is not going to proceed on its own; a failure outranks a success,
/// because success needs no attention; and anything still running outranks a pane
/// that merely finished quietly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    /// Blocked on you: a guardian confirm, a prompt waiting for an answer.
    Waiting,
    /// The last command exited non-zero.
    Failed,
    /// A watched word appeared in the output.
    Watched,
    /// Still running.
    Running,
    /// Finished cleanly.
    Done,
}

impl State {
    /// A short tag for the line. Deliberately words, not symbols: a glyph needs a
    /// legend and this is read once, in a hurry.
    pub fn tag(self) -> &'static str {
        match self {
            State::Waiting => "waiting",
            State::Failed => "failed",
            State::Watched => "watch",
            State::Running => "running",
            State::Done => "ok",
        }
    }
}

/// The facts about one pane, gathered by the caller.
///
/// `changed` is what decides whether the pane appears at all — a pane that sat at a
/// prompt the whole time you were away has nothing to say, and four lines saying
/// "nothing happened" is worse than no catch-up.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub pane: u64,
    /// What the pane is called in the UI (the foreground process, usually).
    pub title: String,
    /// Whether anything happened in this pane while the user was away.
    pub changed: bool,
    /// Blocked on the user: a confirm prompt is up in this pane.
    pub waiting: bool,
    /// A command is running right now.
    pub running: bool,
    /// Exit code of the last finished command, when the shell reports one.
    pub exit: Option<i32>,
    /// How long the last command took, when known.
    pub secs: Option<u64>,
    /// A watched keyword that appeared while away.
    pub watch_hit: Option<String>,
    /// The last non-blank line, for panes with no shell integration to speak of.
    pub last_line: Option<String>,
    /// Whether this pane reports OSC 133 marks at all. Without them there is no
    /// exit code and no timing, and the honest headline is the last line verbatim.
    pub marked: bool,
}

/// One line of the catch-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Headline {
    pub pane: u64,
    pub state: State,
    pub title: String,
    pub detail: String,
}

/// Turns a pane's facts into its headline, or `None` when it has nothing to say.
///
/// A pane with no marks does NOT get an invented status: it gets its last line,
/// verbatim, labelled as unmarked. A wrong headline is worse than no headline, and
/// "the terminal told me it was fine" is the worst possible way to be wrong.
pub fn headline(s: &Snapshot) -> Option<Headline> {
    if !s.changed && !s.waiting {
        return None;
    }
    let (state, detail) = if s.waiting {
        (State::Waiting, "waiting for you".to_string())
    } else if let Some(word) = &s.watch_hit {
        (State::Watched, format!("saw {word:?}"))
    } else if s.running {
        (State::Running, match s.secs {
            Some(secs) => format!("running for {}", human_secs(secs)),
            None => "running".to_string(),
        })
    } else if !s.marked {
        // No shell integration: say what is on screen, and say that is all we know.
        let line = s.last_line.clone().unwrap_or_default();
        let line = truncate(line.trim(), 60);
        if line.is_empty() {
            return None;
        }
        (State::Done, format!("no marks · {line}"))
    } else {
        match s.exit {
            Some(0) | None => (State::Done, finished(s)),
            Some(code) => (State::Failed, format!("exit {code}{}", took(s))),
        }
    };
    Some(Headline { pane: s.pane, state, title: s.title.clone(), detail })
}

fn finished(s: &Snapshot) -> String {
    match s.secs {
        Some(secs) => format!("ok · {}", human_secs(secs)),
        None => "ok".to_string(),
    }
}

fn took(s: &Snapshot) -> String {
    match s.secs {
        Some(secs) => format!(" · {}", human_secs(secs)),
        None => String::new(),
    }
}

/// Durations read by a person who just walked back to the desk: seconds up to a
/// minute, then minutes, then hours. Nobody needs "4211s".
fn human_secs(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m{:02}s", secs / 60, secs % 60),
        _ => format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}

/// When the "what did I miss" baseline is taken.
///
/// Everything the catch-up can ever report is measured from a mark on each pane's
/// command counter, so WHERE that mark sits decides what is reportable at all. Taking
/// it when the absence is finally noticed — after a minute of silence — quietly
/// swallows that minute: a deploy that failed thirty seconds after the user stood up
/// is folded into the baseline and can never be a headline, which is precisely the
/// case the feature exists for.
///
/// So the mark belongs at the last KEYSTROKE, the moment the user was demonstrably
/// still there. This flag is what puts it there: a key arms it, and the next sweep
/// over the panes takes the mark.
#[derive(Debug, Default)]
pub struct Baseline {
    armed: bool,
}

impl Baseline {
    /// A key reached a child: the user is here, and this is the new "before".
    pub fn key_reached_a_child(&mut self) {
        self.armed = true;
    }

    /// Whether this sweep owes the panes a mark. Consumes the arming, so a stretch of
    /// typing costs one pass over the panes rather than one per sweep for ever.
    pub fn take_due(&mut self) -> bool {
        std::mem::take(&mut self.armed)
    }
}

/// Every pane's headline, most urgent first.
///
/// Ties break by pane id so the list is stable: a catch-up whose lines reorder
/// between two glances is one you have to read twice.
pub fn catch_up(panes: &[Snapshot]) -> Vec<Headline> {
    let mut out: Vec<Headline> = panes.iter().filter_map(headline).collect();
    out.sort_by_key(|h| (h.state, h.pane));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(pane: u64) -> Snapshot {
        Snapshot {
            pane,
            title: format!("pane{pane}"),
            changed: true,
            waiting: false,
            running: false,
            exit: Some(0),
            secs: Some(3),
            watch_hit: None,
            last_line: None,
            marked: true,
        }
    }

    /// The ordering IS the feature: the first line you read has to be the one that
    /// will not proceed without you.
    #[test]
    fn what_waits_for_you_outranks_what_merely_broke() {
        let mut waiting = snap(3);
        waiting.waiting = true;
        let mut failed = snap(1);
        failed.exit = Some(1);
        let mut running = snap(2);
        running.running = true;
        let done = snap(4);

        let order: Vec<u64> =
            catch_up(&[done, running, failed, waiting]).iter().map(|h| h.pane).collect();
        assert_eq!(order, vec![3, 1, 2, 4], "waiting, failed, running, done");
    }

    /// Four panes that sat at a prompt produce a four-line report saying nothing
    /// happened, which is worse than no report at all.
    #[test]
    fn a_pane_with_nothing_to_say_says_nothing() {
        let mut quiet = snap(1);
        quiet.changed = false;
        assert!(headline(&quiet).is_none());

        // …unless it is blocking on you, which is worth saying even if the pane
        // itself has been still the whole time.
        quiet.waiting = true;
        assert_eq!(headline(&quiet).unwrap().state, State::Waiting);
    }

    /// Without shell integration there is no exit code, so the honest answer is the
    /// last line and an admission — never an invented "ok".
    #[test]
    fn an_unmarked_pane_gets_its_last_line_not_an_invented_status() {
        let mut s = snap(1);
        s.marked = false;
        s.exit = None;
        s.last_line = Some("  Watching for file changes...  ".into());
        let h = headline(&s).unwrap();
        assert_eq!(h.detail, "no marks · Watching for file changes...");

        // Nothing on screen either: still no headline rather than an empty line.
        s.last_line = Some("   ".into());
        assert!(headline(&s).is_none());
    }

    /// Durations are read by someone who just walked back, not by a stopwatch.
    #[test]
    fn durations_are_written_for_a_person() {
        assert_eq!(human_secs(9), "9s");
        assert_eq!(human_secs(72), "1m12s");
        assert_eq!(human_secs(4211), "1h10m");
    }

    /// A long last line cannot push the rest of the line off screen.
    #[test]
    fn a_long_line_is_cut_with_an_ellipsis() {
        let mut s = snap(1);
        s.marked = false;
        s.last_line = Some("x".repeat(200));
        let h = headline(&s).unwrap();
        assert!(h.detail.chars().count() < 80, "{}", h.detail);
        assert!(h.detail.ends_with('…'));
    }

    /// Two panes in the same state keep a stable order, or the list reads
    /// differently every time you glance at it.
    #[test]
    fn ties_break_by_pane_so_the_list_does_not_shuffle() {
        let a = snap(7);
        let b = snap(2);
        let order: Vec<u64> = catch_up(&[a, b]).iter().map(|h| h.pane).collect();
        assert_eq!(order, vec![2, 7]);
    }

    /// The baseline belongs at the last keystroke. Taken a minute later, when the
    /// silence finally reads as absence, it has already absorbed the thirty-second
    /// deploy that failed just after the user stood up — the one headline the whole
    /// catch-up exists to print.
    #[test]
    fn the_baseline_is_taken_at_the_last_keystroke_not_when_the_absence_is_noticed() {
        let mut baseline = Baseline::default();
        assert!(!baseline.take_due(), "nothing to mark before anyone has typed");

        baseline.key_reached_a_child();
        assert!(baseline.take_due(), "the mark follows the key, not a timer");
        assert!(!baseline.take_due(), "and it is taken once, not again on every sweep");

        // Typing again moves the baseline forward: whatever ran while the user was at
        // the keyboard is not something they missed.
        baseline.key_reached_a_child();
        baseline.key_reached_a_child();
        assert!(baseline.take_due());
        assert!(!baseline.take_due());
    }

    /// A watched word is news even when the command itself succeeded.
    #[test]
    fn a_watch_hit_outranks_a_clean_exit() {
        let mut s = snap(1);
        s.watch_hit = Some("ERROR".into());
        let h = headline(&s).unwrap();
        assert_eq!(h.state, State::Watched);
        assert!(h.detail.contains("ERROR"));
    }
}
