//! "Whisper" — talk to the terminal.
//!
//! You type an intention in plain language; a model turns it into a plan of
//! terminal actions, which runnir executes. It unifies driving the terminal
//! itself (splits, tabs, ssh, search) with running shell commands, and fits the
//! name: *rún* is a whisper to the machine.
//!
//! The model is asked for JSON so the plan is machine-checkable; anything it
//! invents outside the vocabulary is dropped rather than trusted.

use serde::Deserialize;

/// One step of a whisper plan.
#[derive(Debug, Deserialize, PartialEq)]
pub struct Step {
    pub action: String,
    #[serde(default)]
    pub arg: String,
}

/// The system prompt that constrains the model to runnir's action vocabulary.
/// Kept here so the vocabulary and the parser cannot drift apart.
pub fn prompt(request: &str) -> String {
    format!(
        "You control a terminal emulator called runnir. Turn the user's request into \
         a JSON array of steps. Output ONLY the JSON array, nothing else.\n\n\
         Each step is {{\"action\": \"<name>\", \"arg\": \"<string>\"}}. Valid actions:\n\
         - new_tab            open a new tab\n\
         - split_h            split the pane left/right\n\
         - split_v            split the pane up/down\n\
         - close_pane         close the focused pane\n\
         - focus_left|focus_right|focus_up|focus_down   move focus\n\
         - ssh   arg=host     open a split and ssh to host\n\
         - run   arg=command  type a shell command at the prompt (do NOT include a newline)\n\
         - search arg=text    search the scrollback for text\n\
         - font_bigger|font_smaller\n\
         - broadcast          toggle broadcast input\n\
         - launch_claude      launch Claude Code in a split\n\
         - docs               show the help\n\n\
         Prefer runnir actions over shell where possible. For 'connect to the four \
         servers' emit an ssh step per host. Never invent actions outside this list.\n\n\
         Request: {request}"
    )
}

/// Parses a model reply into a plan, tolerating markdown fences and surrounding
/// prose. Returns only well-formed steps.
pub fn parse(reply: &str) -> Vec<Step> {
    // Find the JSON array even if the model wrapped it in text or a code fence.
    let start = reply.find('[');
    let end = reply.rfind(']');
    let json = match (start, end) {
        (Some(s), Some(e)) if e > s => &reply[s..=e],
        _ => return Vec::new(),
    };
    serde_json::from_str::<Vec<Step>>(json).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_clean_plan() {
        let plan = parse(r#"[{"action":"split_v"},{"action":"ssh","arg":"192.168.1.3"}]"#);
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].action, "split_v");
        assert_eq!(plan[1].action, "ssh");
        assert_eq!(plan[1].arg, "192.168.1.3");
    }

    #[test]
    fn extracts_json_from_surrounding_text() {
        let reply = "Sure! Here is the plan:\n```json\n[{\"action\":\"new_tab\"}]\n```\nDone.";
        let plan = parse(reply);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].action, "new_tab");
    }

    #[test]
    fn a_step_without_arg_defaults_to_empty() {
        let plan = parse(r#"[{"action":"font_bigger"}]"#);
        assert_eq!(plan[0].arg, "");
    }

    #[test]
    fn garbage_yields_no_steps() {
        assert!(parse("I cannot help with that.").is_empty());
        assert!(parse("").is_empty());
        assert!(parse("[not json]").is_empty());
    }

    #[test]
    fn the_prompt_lists_the_vocabulary() {
        // A guard so the prompt and the dispatcher stay in sync: every action the
        // dispatcher handles must be named in the prompt.
        let p = prompt("test");
        for action in ["new_tab", "split_h", "split_v", "ssh", "run", "search", "launch_claude"] {
            assert!(p.contains(action), "prompt must document {action}");
        }
    }
}
