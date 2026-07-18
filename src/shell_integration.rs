//! Automatic shell integration.
//!
//! Terminal features like command navigation, the pass/fail status gutter and
//! portable cwd tracking rely on the shell emitting OSC 133 prompt marks and OSC 7
//! cwd reports. Rather than make the user hand-edit their rc files, runnir ships
//! snippets for fish/zsh/bash and arranges for the spawned shell to source them
//! without touching the user's config — the same trick kitty and ghostty use:
//!
//! - **fish**: prepend a runnir dir to `XDG_DATA_DIRS`; fish autoloads
//!   `<dir>/fish/vendor_conf.d/runnir.fish` from it.
//! - **zsh**: point `ZDOTDIR` at a runnir dir whose startup files source the user's
//!   real ones (via `RUNNIR_ZDOTDIR`) and then restore `ZDOTDIR`.
//! - **bash**: pass `--rcfile <snippet>`; the snippet sources the user's `~/.bashrc`
//!   then installs the hooks. (Only interactive non-login bash honours `--rcfile`.)
//!
//! Everything here is best-effort and fail-safe: if the shell is unrecognised or a
//! file can't be written, the shell is spawned completely unchanged. The snippets
//! are idempotent and never clobber the user's own prompt.

use std::path::{Path, PathBuf};

use crate::pty::Spawn;

/// Injects shell integration into `spawn` when `enabled`, based on the detected
/// shell. A no-op (leaving `spawn` untouched) whenever detection fails, the data
/// dir is unavailable, or the snippets can't be written — never breaks spawning.
pub fn apply(spawn: &mut Spawn, enabled: bool) {
    if !enabled {
        return;
    }
    let Some(program) = shell_program(spawn) else {
        return;
    };
    // argv[0] of a login shell is like "-bash"; strip the dash before matching.
    let shell = Path::new(&program)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .trim_start_matches('-');
    let Some(base) = data_base() else {
        return;
    };
    if ensure_snippets(&base).is_err() {
        return; // Couldn't write the snippets: spawn unchanged rather than break.
    }
    match shell {
        "fish" => apply_fish(spawn, &base),
        "zsh" => apply_zsh(spawn, &base),
        "bash" => apply_bash(spawn, &base, &program),
        _ => {} // Unknown shell: leave it alone.
    }
}

/// The program that will actually run: the explicit command, else `$SHELL`.
fn shell_program(spawn: &Spawn) -> Option<String> {
    match &spawn.command {
        Some(cmd) if !cmd.is_empty() => Some(cmd[0].clone()),
        _ => std::env::var("SHELL").ok(),
    }
}

/// `<data dir>/runnir/shell`, the root holding the generated snippets.
fn data_base() -> Option<PathBuf> {
    let mut p = dirs::data_dir()?;
    p.push("runnir");
    p.push("shell");
    Some(p)
}

/// Sets or replaces an env var on the spawn (last write wins, no duplicates).
fn push_env(spawn: &mut Spawn, key: &str, val: String) {
    spawn.env.retain(|(k, _)| k != key);
    spawn.env.push((key.to_string(), val));
}

fn apply_fish(spawn: &mut Spawn, base: &Path) {
    // fish scans `<entry>/fish/vendor_conf.d/*.fish` for every entry of
    // XDG_DATA_DIRS. Prepend `base` so our runnir.fish loads, but keep the existing
    // entries (and the spec default when unset) so system integrations still work.
    let existing = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    let base_str = base.to_string_lossy();
    if existing.split(':').any(|d| d == base_str) {
        return; // Already present (nested runnir): don't prepend twice.
    }
    push_env(spawn, "XDG_DATA_DIRS", format!("{base_str}:{existing}"));
}

fn apply_zsh(spawn: &mut Spawn, base: &Path) {
    // Redirect zsh's startup-file search to our dir; its files source the user's
    // real ones and then restore ZDOTDIR. Carry the real value through RUNNIR_ZDOTDIR.
    let real = std::env::var("ZDOTDIR").unwrap_or_default();
    push_env(spawn, "RUNNIR_ZDOTDIR", real);
    push_env(spawn, "ZDOTDIR", base.join("zsh").to_string_lossy().into_owned());
}

fn apply_bash(spawn: &mut Spawn, base: &Path, program: &str) {
    let rc = base.join("bash").join("runnir.bash");
    let cmd = spawn
        .command
        .clone()
        .unwrap_or_else(|| vec![program.to_string()]);
    if cmd.iter().any(|a| a == "--rcfile") {
        return; // Caller already set an rcfile: don't fight it.
    }
    // `bash --rcfile <snippet> <rest>`. Only interactive non-login bash reads it;
    // a login shell ignores --rcfile (reads .bash_profile) — the documented gap.
    let mut new = vec![cmd[0].clone(), "--rcfile".to_string(), rc.to_string_lossy().into_owned()];
    new.extend_from_slice(&cmd[1..]);
    spawn.command = Some(new);
}

/// Writes each snippet to disk when missing or out of date. Idempotent and cheap;
/// content-compared so an unchanged snippet is not rewritten.
fn ensure_snippets(base: &Path) -> std::io::Result<()> {
    let fish_dir = base.join("fish").join("vendor_conf.d");
    std::fs::create_dir_all(&fish_dir)?;
    write_if_changed(&fish_dir.join("runnir.fish"), FISH_SNIPPET)?;

    let zsh_dir = base.join("zsh");
    std::fs::create_dir_all(&zsh_dir)?;
    write_if_changed(&zsh_dir.join(".zshenv"), ZSH_ZSHENV)?;
    write_if_changed(&zsh_dir.join(".zprofile"), ZSH_ZPROFILE)?;
    write_if_changed(&zsh_dir.join(".zshrc"), ZSH_ZSHRC)?;

    let bash_dir = base.join("bash");
    std::fs::create_dir_all(&bash_dir)?;
    write_if_changed(&bash_dir.join("runnir.bash"), BASH_SNIPPET)?;
    Ok(())
}

fn write_if_changed(path: &Path, content: &str) -> std::io::Result<()> {
    if std::fs::read_to_string(path).map(|c| c == content).unwrap_or(false) {
        return Ok(());
    }
    std::fs::write(path, content)
}

// ---- Snippets --------------------------------------------------------------
//
// All three emit the same escapes: OSC 133 ;A (prompt start) ;B (prompt end) ;C
// (command start) ;D;<code> (command done + exit) and OSC 7 file://host/cwd. `\e`
// is understood by fish/zsh/bash `printf`; `\e\\` is ESC + `\` = the ST terminator.

const FISH_SNIPPET: &str = r#"# runnir shell integration (auto-injected via XDG_DATA_DIRS). Do not edit;
# regenerated by runnir. Emits OSC 133 prompt marks + OSC 7 cwd.
status is-interactive; or return
set -q __runnir_integration; and return

# fish >= 4.0 ships built-in OSC 133/7 integration; injecting ours on top would
# double every mark (and double-count commands). Step aside and let fish do it.
set -l __runnir_fish_major (string split '.' -- $version)[1]
if test -n "$__runnir_fish_major"; and test "$__runnir_fish_major" -ge 4 2>/dev/null
    return
end
set -g __runnir_integration 1

# Wrap fish_prompt lazily on the first prompt, when the user's own fish_prompt is
# guaranteed loaded — wrapping now (conf.d time) would shadow it. The event fires
# before fish_prompt runs, so even the first prompt gets marks. The wrapper also
# reports the cwd (OSC 7) each prompt, like zsh/bash precmd — running that at
# conf.d time is too early (PATH not yet set) and errors.
function __runnir_install --on-event fish_prompt
    functions --erase __runnir_install
    functions -q fish_prompt; and functions --copy fish_prompt __runnir_orig_prompt
    function fish_prompt
        # $hostname is set natively by fish (no external `hostname` binary needed);
        # runnir strips the host component anyway.
        printf '\e]7;file://%s%s\e\\' $hostname "$PWD"
        printf '\e]133;A\e\\'
        if functions -q __runnir_orig_prompt
            __runnir_orig_prompt
        else
            printf '%s@%s %s> ' "$USER" (prompt_hostname) (prompt_pwd)
        end
        printf '\e]133;B\e\\'
    end
end

function __runnir_preexec --on-event fish_preexec
    printf '\e]133;C\e\\'
end

function __runnir_postexec --on-event fish_postexec
    printf '\e]133;D;%s\e\\' $status
end
"#;

const ZSH_ZSHENV: &str = r#"# runnir shell integration — auto-injected via ZDOTDIR. Do not edit.
# Load the user's real .zshenv while keeping ZDOTDIR pointed at runnir so runnir's
# .zshrc loads next.
__runnir_self_zdotdir="$ZDOTDIR"
[[ -f "${RUNNIR_ZDOTDIR:-$HOME}/.zshenv" ]] && source "${RUNNIR_ZDOTDIR:-$HOME}/.zshenv"
ZDOTDIR="$__runnir_self_zdotdir"
unset __runnir_self_zdotdir
"#;

const ZSH_ZPROFILE: &str = r#"# runnir shell integration — auto-injected via ZDOTDIR. Do not edit.
__runnir_self_zdotdir="$ZDOTDIR"
[[ -f "${RUNNIR_ZDOTDIR:-$HOME}/.zprofile" ]] && source "${RUNNIR_ZDOTDIR:-$HOME}/.zprofile"
ZDOTDIR="$__runnir_self_zdotdir"
unset __runnir_self_zdotdir
"#;

const ZSH_ZSHRC: &str = r#"# runnir shell integration — auto-injected via ZDOTDIR. Do not edit.
# Load the user's real interactive config first, then restore ZDOTDIR so the rest
# of the session (and any .zlogin) sees the real value.
__runnir_real_zdotdir="${RUNNIR_ZDOTDIR:-$HOME}"
[[ -f "$__runnir_real_zdotdir/.zshrc" ]] && source "$__runnir_real_zdotdir/.zshrc"
if [[ -n "$RUNNIR_ZDOTDIR" ]]; then
    export ZDOTDIR="$RUNNIR_ZDOTDIR"
else
    unset ZDOTDIR
fi
unset RUNNIR_ZDOTDIR __runnir_real_zdotdir

# Prompt marks (A/B/C/D) + OSC 7 cwd. Idempotent; hooks never touch the user's PS1
# except to append a zero-width end-of-prompt mark.
if [[ -o interactive ]] && [[ -z "$__runnir_integration" ]]; then
    __runnir_integration=1
    autoload -Uz add-zsh-hook

    __runnir_precmd() {
        local __ret=$?
        printf '\e]133;D;%s\e\\' "$__ret"
        printf '\e]7;file://%s%s\e\\' "${HOST}" "${PWD}"
        printf '\e]133;A\e\\'
    }
    __runnir_preexec() {
        printf '\e]133;C\e\\'
    }
    add-zsh-hook precmd __runnir_precmd
    add-zsh-hook preexec __runnir_preexec

    # B (end of prompt / input start): a zero-width escape appended to PS1. %{...%}
    # tells zsh it occupies no columns, so the prompt renders unchanged.
    if [[ "$PS1" != *"133;B"* ]]; then
        PS1="$PS1%{$(printf '\e]133;B\e\\')%}"
    fi
fi
"#;

const BASH_SNIPPET: &str = r#"# runnir shell integration — auto-injected via --rcfile. Do not edit.
# --rcfile replaces ~/.bashrc, so source it ourselves, then add the hooks.
if [[ -f "$HOME/.bashrc" ]]; then
    source "$HOME/.bashrc"
fi

if [[ $- == *i* ]] && [[ -z "$__runnir_integration" ]]; then
    __runnir_integration=1

    __runnir_precmd() {
        local __ret=$?
        printf '\e]133;D;%s\e\\' "$__ret"
        printf '\e]7;file://%s%s\e\\' "${HOSTNAME}" "${PWD}"
        printf '\e]133;A\e\\'
        __runnir_preexec_done=
    }

    # bash has no native preexec: use the DEBUG trap, firing C once per command.
    __runnir_preexec() {
        [[ -n "$COMP_LINE" ]] && return                     # completion, not a command
        [[ "$BASH_COMMAND" == "__runnir_precmd" ]] && return # our own prompt command
        [[ -n "$__runnir_preexec_done" ]] && return          # already marked this line
        __runnir_preexec_done=1
        printf '\e]133;C\e\\'
    }
    trap '__runnir_preexec' DEBUG

    case ";${PROMPT_COMMAND};" in
        *";__runnir_precmd;"*) ;;
        *) PROMPT_COMMAND="__runnir_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}" ;;
    esac

    # B mark appended to PS1; \[ \] are bash's zero-width markers.
    if [[ "$PS1" != *"133;B"* ]]; then
        PS1="$PS1\[$(printf '\e]133;B\e\\')\]"
    fi
fi
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_with(cmd: Option<Vec<&str>>) -> Spawn {
        Spawn {
            command: cmd.map(|c| c.into_iter().map(String::from).collect()),
            ..Default::default()
        }
    }

    fn env_of<'a>(spawn: &'a Spawn, key: &str) -> Option<&'a str> {
        spawn.env.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    #[test]
    fn disabled_leaves_spawn_untouched() {
        let mut s = spawn_with(Some(vec!["/usr/bin/fish"]));
        apply(&mut s, false);
        assert!(s.env.is_empty());
        assert_eq!(s.command, Some(vec!["/usr/bin/fish".to_string()]));
    }

    #[test]
    fn fish_prepends_xdg_data_dirs() {
        let mut s = spawn_with(Some(vec!["/usr/bin/fish"]));
        apply(&mut s, true);
        let xdg = env_of(&s, "XDG_DATA_DIRS").expect("XDG_DATA_DIRS set");
        let base = data_base().unwrap();
        assert!(
            xdg.starts_with(&*base.to_string_lossy()),
            "runnir dir must be prepended: {xdg}"
        );
        // Command is left alone for fish (env-only mechanism).
        assert_eq!(s.command, Some(vec!["/usr/bin/fish".to_string()]));
    }

    #[test]
    fn zsh_sets_zdotdir_and_carries_the_real_one() {
        let mut s = spawn_with(Some(vec!["zsh"]));
        apply(&mut s, true);
        let zdotdir = env_of(&s, "ZDOTDIR").expect("ZDOTDIR set");
        assert!(zdotdir.ends_with("runnir/shell/zsh"), "got {zdotdir}");
        assert!(env_of(&s, "RUNNIR_ZDOTDIR").is_some(), "must carry the real ZDOTDIR");
    }

    #[test]
    fn bash_injects_rcfile_before_existing_args() {
        let mut s = spawn_with(Some(vec!["/bin/bash"]));
        apply(&mut s, true);
        let cmd = s.command.expect("command");
        assert_eq!(cmd[0], "/bin/bash");
        assert_eq!(cmd[1], "--rcfile");
        assert!(cmd[2].ends_with("runnir/shell/bash/runnir.bash"), "got {}", cmd[2]);
    }

    #[test]
    fn login_shell_argv0_dash_is_recognised() {
        // A login shell's argv[0] is "-zsh"; the dash must not defeat detection.
        let mut s = spawn_with(Some(vec!["-zsh"]));
        apply(&mut s, true);
        assert!(env_of(&s, "ZDOTDIR").is_some(), "'-zsh' must be detected as zsh");
    }

    #[test]
    fn unknown_shell_is_left_alone() {
        let mut s = spawn_with(Some(vec!["/usr/bin/nu"]));
        apply(&mut s, true);
        assert!(s.env.is_empty());
        assert_eq!(s.command, Some(vec!["/usr/bin/nu".to_string()]));
    }

    #[test]
    fn snippets_are_written_and_valid() {
        let base = data_base().expect("data dir");
        ensure_snippets(&base).expect("write snippets");
        // All five files must exist.
        let all = [
            "fish/vendor_conf.d/runnir.fish",
            "zsh/.zshenv",
            "zsh/.zprofile",
            "zsh/.zshrc",
            "bash/runnir.bash",
        ];
        for rel in all {
            assert!(base.join(rel).exists(), "missing snippet: {rel}");
        }
        // The mark-emitting snippets carry OSC 133; the zsh loaders only source the
        // user's real config, so they don't.
        for rel in ["fish/vendor_conf.d/runnir.fish", "zsh/.zshrc", "bash/runnir.bash"] {
            let body = std::fs::read_to_string(base.join(rel)).unwrap();
            assert!(body.contains("133;A"), "{rel} must emit OSC 133 marks");
            assert!(body.contains("]7;file://"), "{rel} must emit OSC 7 cwd");
        }
        // The zsh loaders must chain to the user's real files.
        for rel in ["zsh/.zshenv", "zsh/.zprofile"] {
            let body = std::fs::read_to_string(base.join(rel)).unwrap();
            assert!(body.contains("RUNNIR_ZDOTDIR"), "{rel} must chain to the real config");
        }
    }
}
