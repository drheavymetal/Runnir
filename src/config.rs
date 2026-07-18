//! User configuration, read from `~/.config/runnir/runnir.toml`.
//!
//! Every field has a default that stands on its own, so an absent or partial file
//! is normal rather than an error. A malformed file is reported and then ignored:
//! a typo in a colour must never cost you your terminal.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub font: Font,
    pub window: WindowCfg,
    pub cursor: CursorCfg,
    pub scrollback: Scrollback,
    pub theme: Theme,
    pub behaviour: Behaviour,
    pub ai: Ai,
    /// Extra keybindings, merged over the built-in ones. `"ctrl+shift+t" = "new_tab"`.
    pub keys: HashMap<String, String>,
    /// Named workspace layouts. Each opens a fresh tab split into one pane per
    /// command. Launch from the palette (Launch layout) — e.g. a `servers` layout
    /// that ssh's into .3/.7/.9/.188 at once.
    #[serde(default)]
    pub layouts: Vec<LayoutDef>,
}

/// A named layout: a tab split into one pane per command. An empty command opens a
/// plain shell pane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutDef {
    pub name: String,
    #[serde(default)]
    pub commands: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font: Font::default(),
            window: WindowCfg::default(),
            cursor: CursorCfg::default(),
            scrollback: Scrollback::default(),
            theme: Theme::default(),
            behaviour: Behaviour::default(),
            ai: Ai::default(),
            keys: HashMap::new(),
            layouts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Font {
    pub family: String,
    pub size: f32,
    pub ligatures: bool,
}

impl Default for Font {
    fn default() -> Self {
        Self {
            family: "JetBrainsMono Nerd Font Mono".into(),
            size: 16.0,
            ligatures: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WindowCfg {
    pub width: f32,
    pub height: f32,
    pub padding: f32,
    pub decorations: bool,
    pub opacity: f32,
    /// Show a status bar along the bottom (cwd, git branch, clock). Costs one row.
    #[serde(default = "yes")]
    pub status_bar: bool,
    /// Path to a background image drawn behind the terminal. Needs opacity < 1 to
    /// show through. `None` = solid background.
    #[serde(default)]
    pub background: Option<String>,
    /// How much to dim the background image (0 = black, 1 = full brightness).
    #[serde(default = "default_bg_dim")]
    pub background_dim: f32,
    /// Show a scrollback minimap on the right edge of the focused pane.
    #[serde(default)]
    pub minimap: bool,
}

fn default_bg_dim() -> f32 {
    0.35
}

impl Default for WindowCfg {
    fn default() -> Self {
        Self {
            width: 1100.0,
            height: 700.0,
            padding: 8.0,
            decorations: false,
            opacity: 1.0,
            status_bar: true,
            background: None,
            background_dim: default_bg_dim(),
            minimap: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorShape {
    Block,
    Beam,
    Underline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CursorCfg {
    pub shape: CursorShape,
    pub blink: bool,
    /// Milliseconds per blink phase.
    pub blink_interval: u64,
    /// Draw a brief fading trail behind the cursor as it jumps (flair, off by
    /// default).
    #[serde(default)]
    pub trail: bool,
}

impl Default for CursorCfg {
    fn default() -> Self {
        Self { shape: CursorShape::Block, blink: true, blink_interval: 600, trail: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Scrollback {
    pub lines: usize,
}

impl Default for Scrollback {
    fn default() -> Self {
        Self { lines: 10_000 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Behaviour {
    pub copy_on_select: bool,
    pub wheel_lines: f32,
    /// Tint the background when the foreground process is ssh / sudo / docker, so
    /// you always know which world a pane is in.
    pub context_tint: bool,
    /// Notify when a command that took longer than this finishes unfocused.
    /// Zero disables.
    pub notify_after_secs: u64,
    pub confirm_close: bool,
    /// Reopen the previous session (tabs, layout, working dirs, scrollback text)
    /// on start. The processes do not survive — only the layout and history do.
    pub restore_session: bool,
    /// Guard dangerous commands: when the command line about to run matches a known
    /// destructive pattern (rm -rf /, dd of=, mkfs, DROP TABLE, git push -f, fork
    /// bomb, …), pressing Enter opens a confirmation instead of running it blindly.
    pub command_guardian: bool,
    /// Animate scroll jumps (to top/bottom, jump-to-prompt) with an eased glide
    /// instead of teleporting.
    #[serde(default = "yes")]
    pub smooth_scroll: bool,
    /// Inject shell integration (OSC 133 prompt marks + OSC 7 cwd) into fish/zsh/bash
    /// automatically, without the user editing their rc files. Powers command
    /// navigation, the pass/fail gutter, and portable cwd tracking. Detection is
    /// best-effort: an unrecognised shell is spawned unchanged.
    #[serde(default = "yes")]
    pub shell_integration: bool,
    /// Restore the pane/tab layout last used for this project directory (keyed by the
    /// nearest git ancestor of the launch cwd) when runnir opens there. Off by
    /// default — a purely opt-in convenience, separate from `restore_session` (which
    /// restores the whole previous window regardless of directory). Only the split
    /// shape and each pane's cwd come back; scrollback and processes do not.
    #[serde(default)]
    pub session_restore: bool,
    /// When `session_restore` is on, also save the current project's layout
    /// automatically on exit, so you never have to remember to. Off by default;
    /// the palette command saves on demand regardless.
    #[serde(default)]
    pub session_auto_save: bool,
}

fn yes() -> bool {
    true
}

impl Default for Behaviour {
    fn default() -> Self {
        Self {
            copy_on_select: true,
            wheel_lines: 3.0,
            context_tint: true,
            notify_after_secs: 20,
            confirm_close: true,
            restore_session: true,
            command_guardian: true,
            smooth_scroll: true,
            shell_integration: true,
            session_restore: false,
            session_auto_save: false,
        }
    }
}

// ---- AI -------------------------------------------------------------------

/// How runnir reaches a model.
///
/// The two variants are genuinely different transports, not a detail: Claude Code
/// runs as a **subprocess** and bills against the user's subscription, while every
/// other provider is an HTTP API paid per token. Collapsing them into one shape
/// would mean pretending a subscription is an API key.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Provider {
    /// Spawns the Claude Code CLI. No API key, no per-token billing.
    ClaudeCode {
        /// Binary to run. Claude Code installs as `claude`.
        #[serde(default = "default_claude_command")]
        command: String,
        /// Arguments passed on every invocation, before the prompt.
        #[serde(default)]
        args: Vec<String>,
        /// Skips Claude Code's permission prompts. Only meaningful for agentic
        /// runs, and it lets the model act without asking — off unless you say so.
        #[serde(default)]
        dangerously_skip_permissions: bool,
    },
    /// Any OpenAI-compatible chat-completions endpoint.
    Api {
        base_url: String,
        model: String,
        /// Environment variable holding the key. The key itself never goes in the
        /// config file — that file ends up in dotfile repos.
        api_key_env: String,
    },
}

fn default_claude_command() -> String {
    "claude".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Ai {
    /// Which entry of `providers` to use by default.
    pub default: String,
    pub providers: HashMap<String, Provider>,
    /// Seconds before a request is abandoned.
    pub timeout_secs: u64,
}

impl Default for Ai {
    fn default() -> Self {
        let mut providers = HashMap::new();
        providers.insert(
            "claude".into(),
            Provider::ClaudeCode {
                command: default_claude_command(),
                args: Vec::new(),
                dangerously_skip_permissions: false,
            },
        );
        // A second Claude Code entry, pre-wired for agentic work. Present but not
        // default: it must be an explicit choice.
        providers.insert(
            "claude-yolo".into(),
            Provider::ClaudeCode {
                command: default_claude_command(),
                args: Vec::new(),
                dangerously_skip_permissions: true,
            },
        );
        providers.insert(
            "openai".into(),
            Provider::Api {
                base_url: "https://api.openai.com/v1".into(),
                model: "gpt-4o".into(),
                api_key_env: "OPENAI_API_KEY".into(),
            },
        );
        providers.insert(
            "gemini".into(),
            Provider::Api {
                base_url: "https://generativelanguage.googleapis.com/v1beta/openai".into(),
                model: "gemini-2.0-flash".into(),
                api_key_env: "GEMINI_API_KEY".into(),
            },
        );
        providers.insert(
            "deepseek".into(),
            Provider::Api {
                base_url: "https://api.deepseek.com/v1".into(),
                model: "deepseek-chat".into(),
                api_key_env: "DEEPSEEK_API_KEY".into(),
            },
        );
        providers.insert(
            "zai".into(),
            Provider::Api {
                base_url: "https://api.z.ai/api/paas/v4".into(),
                model: "glm-4.6".into(),
                api_key_env: "ZAI_API_KEY".into(),
            },
        );
        Self { default: "claude".into(), providers, timeout_secs: 120 }
    }
}

// ---- Theme ----------------------------------------------------------------

/// `"#rrggbb"` on the wire, packed RGB in memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Serialize for Rgb {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2))
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        parse_hex(&s).ok_or_else(|| serde::de::Error::custom(format!("bad colour: {s:?}")))
    }
}

fn parse_hex(s: &str) -> Option<Rgb> {
    let s = s.strip_prefix('#').unwrap_or(s);
    // Reject non-ASCII before slicing by byte offset below: a multibyte string of
    // byte-length 3 or 6 would otherwise slice through a char boundary and panic,
    // taking the whole terminal down over a typo in a colour.
    if !s.is_ascii() {
        return None;
    }
    match s.len() {
        6 => Some(Rgb(
            u8::from_str_radix(&s[0..2], 16).ok()?,
            u8::from_str_radix(&s[2..4], 16).ok()?,
            u8::from_str_radix(&s[4..6], 16).ok()?,
        )),
        // #abc is shorthand for #aabbcc.
        3 => {
            let d = |i: usize| u8::from_str_radix(&s[i..i + 1], 16).ok().map(|v| v * 17);
            Some(Rgb(d(0)?, d(1)?, d(2)?))
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub selection: Rgb,
    /// The 16 ANSI colours: 0-7 normal, 8-15 bright.
    pub ansi: Vec<Rgb>,
    /// Accent used by runnir's own UI (tab bar, palette, panels).
    pub accent: Rgb,
    pub dim: Rgb,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            foreground: Rgb(0xd4, 0xd6, 0xd9),
            background: Rgb(0x0d, 0x0d, 0x0f),
            cursor: Rgb(0xd4, 0xd6, 0xd9),
            selection: Rgb(0x33, 0x44, 0x66),
            accent: Rgb(0x4c, 0x9f, 0xd4),
            dim: Rgb(0x6a, 0x6d, 0x74),
            ansi: vec![
                Rgb(0x18, 0x18, 0x1a),
                Rgb(0xcd, 0x31, 0x31),
                Rgb(0x0d, 0xbc, 0x79),
                Rgb(0xe5, 0xe5, 0x10),
                Rgb(0x24, 0x72, 0xc8),
                Rgb(0xbc, 0x3f, 0xbc),
                Rgb(0x11, 0xa8, 0xcd),
                Rgb(0xe5, 0xe5, 0xe5),
                Rgb(0x66, 0x66, 0x66),
                Rgb(0xf1, 0x4c, 0x4c),
                Rgb(0x23, 0xd1, 0x8b),
                Rgb(0xf5, 0xf5, 0x43),
                Rgb(0x3b, 0x8e, 0xea),
                Rgb(0xd6, 0x70, 0xd6),
                Rgb(0x29, 0xb8, 0xdb),
                Rgb(0xff, 0xff, 0xff),
            ],
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("runnir/runnir.toml")
    }

    /// The JSON config the settings panel reads and writes. Preferred over the TOML
    /// file when it exists.
    pub fn json_path() -> PathBuf {
        dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("runnir/runnir.json")
    }

    /// The config file actually in effect: the JSON one if present, else the TOML.
    /// Hot-reload watches this so an external edit to whichever file exists applies.
    pub fn active_path() -> PathBuf {
        let json = Self::json_path();
        if json.exists() { json } else { Self::path() }
    }

    /// Loads the config, falling back to defaults. A broken file is reported and
    /// ignored rather than fatal: you should never be locked out of your terminal
    /// by a typo in it.
    pub fn load() -> Self {
        Self::try_load().unwrap_or_default()
    }

    /// Loads and validates the config, or `None` if the file is missing or invalid.
    /// Prefers the JSON file (settings panel) over the TOML one. Hot-reload uses this
    /// to keep the running config on a parse error rather than snapping to defaults.
    pub fn try_load() -> Option<Self> {
        let json = Self::json_path();
        if json.exists() {
            let text = std::fs::read_to_string(&json).ok()?;
            return match serde_json::from_str::<Self>(&text) {
                Ok(mut cfg) => {
                    cfg.validate();
                    Some(cfg)
                }
                Err(err) => {
                    eprintln!("runnir: {} is invalid, keeping current config\n{err}", json.display());
                    None
                }
            };
        }
        let path = Self::path();
        let text = std::fs::read_to_string(&path).ok()?;
        match toml::from_str::<Self>(&text) {
            Ok(mut cfg) => {
                cfg.validate();
                Some(cfg)
            }
            Err(err) => {
                eprintln!("runnir: {} is invalid, keeping current config\n{err}", path.display());
                None
            }
        }
    }

    /// Writes the config as pretty JSON to the JSON path (settings-panel save).
    pub fn save_json(&self) -> std::io::Result<()> {
        let path = Self::json_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, text)
    }

    /// Clamps values that would render the terminal unusable. A zero font size or
    /// a 3-entry ANSI palette is a mistake, not a preference.
    fn validate(&mut self) {
        self.font.size = self.font.size.clamp(4.0, 200.0);
        self.window.opacity = self.window.opacity.clamp(0.1, 1.0);
        self.window.padding = self.window.padding.clamp(0.0, 200.0);
        self.behaviour.wheel_lines = self.behaviour.wheel_lines.clamp(1.0, 50.0);
        self.scrollback.lines = self.scrollback.lines.min(1_000_000);
        if self.cursor.blink_interval < 50 {
            self.cursor.blink_interval = 50;
        }
        let defaults = Theme::default().ansi;
        if self.ansi_incomplete() {
            eprintln!("runnir: theme.ansi needs 16 colours, using the defaults");
            self.theme.ansi = defaults;
        }
        if !self.ai.providers.contains_key(&self.ai.default) {
            eprintln!("runnir: ai.default {:?} is not a provider", self.ai.default);
        }
    }

    fn ansi_incomplete(&self) -> bool {
        self.theme.ansi.len() != 16
    }

    /// Writes a fully-populated config file. Used by `runnir --write-config` and by
    /// the command palette, so the file that documents the options is generated
    /// from the options themselves and cannot drift from them.
    pub fn write_default(path: &std::path::Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = toml::to_string_pretty(&Self::default())?;
        let header = "\
# runnir configuration.
#
# Every value here is the built-in default: deleting the file changes nothing,
# and deleting any single line falls back to the value shown.
#
# API keys are never stored here — only the name of the environment variable
# holding them. This file is meant to be safe to commit to a dotfiles repo.

";
        std::fs::write(path, format!("{header}{body}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_through_toml() {
        // The generated file must parse back, or `--write-config` writes something
        // that the loader then rejects.
        let text = toml::to_string_pretty(&Config::default()).unwrap();
        let parsed: Config = toml::from_str(&text).expect("default config must re-parse");
        assert_eq!(parsed.font.family, Config::default().font.family);
        assert_eq!(parsed.theme.ansi.len(), 16);
    }

    #[test]
    fn empty_file_yields_defaults() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.font.size, 16.0);
        assert_eq!(cfg.ai.default, "claude");
    }

    #[test]
    fn partial_file_keeps_other_defaults() {
        let cfg: Config = toml::from_str("[font]\nsize = 20.0\n").unwrap();
        assert_eq!(cfg.font.size, 20.0);
        assert_eq!(cfg.font.family, Config::default().font.family, "family must survive");
        assert!(cfg.behaviour.copy_on_select);
    }

    #[test]
    fn colours_parse_both_hex_forms() {
        #[derive(Deserialize)]
        struct T {
            c: Rgb,
        }
        assert_eq!(toml::from_str::<T>(r##"c = "#ff8000""##).unwrap().c, Rgb(255, 128, 0));
        assert_eq!(toml::from_str::<T>(r##"c = "#f80""##).unwrap().c, Rgb(255, 136, 0));
        assert_eq!(toml::from_str::<T>(r#"c = "ff8000""#).unwrap().c, Rgb(255, 128, 0));
        assert!(toml::from_str::<T>(r#"c = "nope""#).is_err());
        // Regression: a multibyte string of byte-length 3 or 6 used to panic on a
        // mid-char slice, crashing the terminal at startup.
        assert!(toml::from_str::<T>(r#"c = "€""#).is_err(), "3-byte non-ascii must not panic");
        assert!(toml::from_str::<T>(r#"c = "aa€""#).is_err(), "6-byte non-ascii must not panic");
    }

    #[test]
    fn claude_code_defaults_to_asking_permission() {
        let cfg = Config::default();
        match &cfg.ai.providers["claude"] {
            Provider::ClaudeCode { dangerously_skip_permissions, command, .. } => {
                assert!(!dangerously_skip_permissions, "skipping permissions must be opt-in");
                assert_eq!(command, "claude");
            }
            _ => panic!("claude must be a ClaudeCode provider, never an API one"),
        }
    }

    #[test]
    fn claude_yolo_exists_but_is_not_the_default() {
        let cfg = Config::default();
        assert_eq!(cfg.ai.default, "claude");
        match &cfg.ai.providers["claude-yolo"] {
            Provider::ClaudeCode { dangerously_skip_permissions, .. } => {
                assert!(dangerously_skip_permissions);
            }
            _ => panic!("claude-yolo must be a ClaudeCode provider"),
        }
    }

    #[test]
    fn api_providers_reference_a_key_env_not_a_key() {
        let cfg = Config::default();
        for (name, p) in &cfg.ai.providers {
            if let Provider::Api { api_key_env, .. } = p {
                assert!(!api_key_env.is_empty(), "{name} must name a key variable");
                // A literal key would leak the moment this file is committed.
                assert!(
                    !api_key_env.starts_with("sk-"),
                    "{name} stores a key, not a variable name"
                );
            }
        }
    }

    #[test]
    fn validate_clamps_unusable_values() {
        let mut cfg = Config::default();
        cfg.font.size = 0.0;
        cfg.window.opacity = 5.0;
        cfg.theme.ansi = vec![Rgb(0, 0, 0)];
        cfg.validate();
        assert_eq!(cfg.font.size, 4.0);
        assert_eq!(cfg.window.opacity, 1.0);
        assert_eq!(cfg.theme.ansi.len(), 16, "a short palette falls back to the default");
    }

    #[test]
    fn unknown_keys_are_rejected_rather_than_ignored() {
        // Silently swallowing a typo means the setting never takes effect and the
        // user has no way to tell.
        let err = toml::from_str::<Config>("[font]\nfamly = \"x\"\n");
        assert!(err.is_err(), "a misspelled key must be reported");
    }
}
