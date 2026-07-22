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
    #[serde(default)]
    pub clipboard: ClipboardCfg,
    pub ai: Ai,
    /// Auto-preview of images dropped into a watched directory.
    pub watch: Watch,
    /// Now-playing media overlay (album art, transport, waveform).
    #[serde(default)]
    pub media: Media,
    /// Learning this project's real verbs from what is typed here.
    #[serde(default)]
    pub verbs: VerbsCfg,
    /// The ZSA keyboard's lights.
    #[serde(default)]
    pub keyboard: Keyboard,
    /// The file explorer sidebar.
    #[serde(default)]
    pub explorer: Explorer,
    /// Extra keybindings, merged over the built-in ones. `"ctrl+shift+t" = "new_tab"`.
    /// A `"leader+v"` key binds on the leader layer instead of as a plain chord.
    pub keys: HashMap<String, String>,
    /// Chord that arms the leader layer (`"alt+shift+space"` by default). Set it to an
    /// empty string to turn the layer off. The leader exists because compositors
    /// win every modifier race — see `actions::default_leader_bindings`.
    #[serde(default = "default_leader")]
    pub leader: String,
    /// Seconds the leader layer stays armed waiting for the next key, per step.
    /// `0` means it never lapses, tmux-style: it then leaves only on an action, a
    /// miss, or Escape. The default is generous because the which-key panel is on
    /// screen the whole time — you are reading it, not stalling.
    #[serde(default = "default_leader_timeout")]
    pub leader_timeout: u64,
    /// Named workspace layouts. Each opens a fresh tab split into one pane per
    /// command. Launch from the palette (Launch layout) — e.g. a `servers` layout
    /// that ssh's into .3/.7/.9/.188 at once.
    #[serde(default)]
    pub layouts: Vec<LayoutDef>,
    /// Named command snippets. Pick one from the palette (Insert command snippet) and
    /// it is typed at the prompt for you to review — never run for you, unless the
    /// snippet sets `run_now = true`.
    #[serde(default)]
    pub snippets: Vec<SnippetDef>,
}

/// A named layout: a tab split into one pane per command. An empty command opens a
/// plain shell pane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutDef {
    pub name: String,
    #[serde(default)]
    pub commands: Vec<String>,
}

/// A named command snippet (a bookmark). Picked from the palette, its `command` is
/// typed at the focused prompt for you to review and run yourself — the same safety
/// model as the AI command-writer. Set `run_now = true` to submit it immediately.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnippetDef {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub description: String,
    /// Submit the command straight away instead of leaving it at the prompt to
    /// review. Off by default — a snippet is inserted, not executed, unless you opt in.
    #[serde(default)]
    pub run_now: bool,
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
            clipboard: ClipboardCfg::default(),
            ai: Ai::default(),
            watch: Watch::default(),
            media: Media::default(),
            verbs: VerbsCfg::default(),
            keyboard: Keyboard::default(),
            explorer: Explorer::default(),
            keys: HashMap::new(),
            leader: default_leader(),
            leader_timeout: default_leader_timeout(),
            layouts: Vec::new(),
            snippets: Vec::new(),
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

fn default_leader() -> String {
    crate::actions::DEFAULT_LEADER.to_string()
}

fn default_leader_timeout() -> u64 {
    10
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

/// Clipboard history: an in-memory ring of recent copies you can re-paste from the
/// picker (palette: Clipboard history, or Alt+Shift+V). Never written to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClipboardCfg {
    /// How many recent copies the history keeps (newest first); the oldest drops
    /// once this is exceeded.
    pub capacity: usize,
    /// Whether copies are recorded into the history at all.
    pub enabled: bool,
}

impl Default for ClipboardCfg {
    fn default() -> Self {
        Self { capacity: 50, enabled: true }
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
    /// Restore the window you closed last, when this is the only window running.
    ///
    /// The snapshot (tabs, layout, directories, scrollback) belongs to the NEXT
    /// window you open, not to one opened beside a live one: a second window
    /// inheriting the layout of a window still on screen is a copy nobody asked
    /// for. Off means every launch starts with one fresh tab.
    ///
    /// Saving a layout on purpose is a different thing and is not affected by this:
    /// see `session_restore` for per-project layouts.
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

// ---- Watch (image auto-preview) -------------------------------------------

/// Watches a directory and previews newly generated images inline in the focused
/// pane. Built for a local media pipeline (SDXL / ComfyUI / Wan) that drops PNG /
/// JPG / WebP files into an output folder: a new file appears, runnir shows it.
///
/// A missing `[watch]` block is fine — every field defaults, and the watcher is off
/// until `enabled = true` (or you toggle it from the palette at runtime).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Watch {
    /// Arm the watcher at startup. Off by default; the palette can also toggle it on
    /// the focused pane's working directory without editing the config.
    pub enabled: bool,
    /// Directory to watch. `None` means "no directory yet" — set one here, or point
    /// the watch at the current pane's cwd from the palette.
    pub directory: Option<String>,
    /// Extensions to preview, without the dot, matched case-insensitively. An empty
    /// list previews any file that lands in the directory.
    pub extensions: Vec<String>,
    /// The widest a preview is drawn, in terminal cells. A larger image is scaled
    /// down to this; a smaller one is shown at its own size.
    pub max_width: usize,
}

impl Default for Watch {
    fn default() -> Self {
        Self {
            enabled: false,
            directory: None,
            extensions: vec!["png".into(), "jpg".into(), "jpeg".into(), "webp".into()],
            max_width: 40,
        }
    }
}

// ---- Media (now playing) --------------------------------------------------

/// The now-playing overlay: album art, transport controls and a live waveform.
/// Metadata and control shell out to `playerctl` (Linux) or `nowplaying-cli` /
/// AppleScript (macOS); the waveform shells out to `cava` (Linux). All are optional
/// at runtime — a missing tool simply degrades gracefully.
///
/// A missing `[media]` block is fine: every field defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Media {
    /// Draw a live audio waveform in the overlay, driven by `cava`. On by default; it
    /// automatically shows nothing when cava is not installed, so it is safe to leave
    /// enabled everywhere.
    pub waveform: bool,
    /// How many waveform bars cava computes and the overlay draws.
    pub bars: usize,
    /// Width of the album-art thumbnail in the overlay, in terminal cells (the height
    /// is derived to keep the cover roughly square).
    pub art_cells: usize,
}

impl Default for Media {
    fn default() -> Self {
        Self { waveform: true, bars: 24, art_cells: 18 }
    }
}

// ---- verbs ------------------------------------------------------------------

/// Learning the commands a repository is actually worked with.
///
/// OFF by default and per-machine: this watches what you type. Only the verb is ever
/// stored — never arguments — and never inside the repo, but even so it is not the
/// kind of thing to switch on for someone.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VerbsCfg {
    pub enabled: bool,
    /// How many successful runs before a command counts as a verb. Two is an
    /// experiment; the default is three.
    pub threshold: u32,
}

impl Default for VerbsCfg {
    fn default() -> Self {
        Self { enabled: false, threshold: crate::verbs::DEFAULT_THRESHOLD }
    }
}

// ---- keyboard -------------------------------------------------------------

/// A programmable keyboard runnir can signal on (ZSA, through Keymapp's API).
///
/// Only whole-board flashes: what the board can carry is a colour, not a key. The
/// lit-leader idea died on opaque keycaps — the LED lights the gap around the cap,
/// not the legend — and the DEVLOG entry for 2026-07-22 has the measurements.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Keyboard {
    /// Flash the board when something wants attention: the guardian asking, a long
    /// command finishing, a watched word appearing. Off unless asked for — a keyboard
    /// changing colour on its own is startling if you did not go looking for it.
    pub ambient: bool,
    /// How long a flash lasts. Also its own cleanup: the board restores itself when
    /// this elapses, so runnir dying mid-flash cannot leave it coloured.
    pub flash_ms: u32,
    /// Light the leader layer on the keys: every key that does something at the
    /// current level, groups and leaves in the which-key's own colours.
    ///
    /// Off by default because it needs SHINE-THROUGH keycaps to be worth anything.
    /// With opaque caps the LED lights the gap around the cap rather than the legend,
    /// so what reaches the eye is a glow in a region, not a key you can name — measured
    /// on a Moonlander, see the DEVLOG for 2026-07-22.
    pub leader_lights: bool,
}

impl Default for Keyboard {
    fn default() -> Self {
        Self { ambient: false, flash_ms: 1200, leader_lights: false }
    }
}

// ---- explorer -------------------------------------------------------------

/// The file explorer sidebar (leader e). Chrome beside the panes, not an overlay.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Explorer {
    /// Which edge it sits on: `"left"` (the default, where every editor puts it) or
    /// `"right"`.
    pub side: String,
    /// Width in COLUMNS, not a fraction of the window: a fraction on an ultrawide
    /// gives a 90-column tree. Clamped against the window when it is drawn.
    pub width: usize,
    /// Show dotfiles. Off by default; `.` toggles it live.
    pub show_hidden: bool,
    /// Open the sidebar on start, in every tab.
    pub open_on_start: bool,
}

impl Default for Explorer {
    fn default() -> Self {
        Self {
            side: "left".to_string(),
            width: 30,
            show_hidden: false,
            open_on_start: false,
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
    /// Anthropic's own Messages API, which is NOT OpenAI-compatible: different
    /// path, `x-api-key` instead of a bearer token, a required version header, a
    /// required `max_tokens`, and a different response shape. Kept as its own
    /// variant rather than bent into `Api`, because pretending one shape fits both
    /// is how you get a config that looks right and fails at request time.
    Anthropic {
        #[serde(default = "default_anthropic_url")]
        base_url: String,
        model: String,
        #[serde(default = "default_anthropic_key_env")]
        api_key_env: String,
        /// Required by the API — there is no default on the wire. Generous by
        /// default: a truncated answer costs a whole round trip to notice.
        #[serde(default = "default_anthropic_max_tokens")]
        max_tokens: u32,
    },
}

fn default_anthropic_url() -> String {
    "https://api.anthropic.com/v1".into()
}

fn default_anthropic_key_env() -> String {
    "ANTHROPIC_API_KEY".into()
}

fn default_anthropic_max_tokens() -> u32 {
    4096
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
    /// Per-task provider overrides, keyed by task name: `panel`, `command`, `fix`,
    /// `explain`, `summarize`, `whisper`.
    ///
    /// The point is economics, not preference. Summarising a whole session is long
    /// and cheap on a flat-rate subscription; translating one sentence into a command
    /// wants the lowest latency you can get. Anything not named here uses `default`.
    #[serde(default)]
    pub tasks: HashMap<String, String>,
}

/// The tasks `ai.tasks` can override, and the only names it accepts.
///
/// A typo in a config file must not silently route a task to the default forever —
/// so an unknown key is reported once at load rather than ignored.
pub const AI_TASKS: [&str; 6] = ["panel", "command", "fix", "explain", "summarize", "whisper"];

impl Ai {
    /// The provider for `task`: its override when one is configured AND names a
    /// provider that exists, else the default.
    ///
    /// An override pointing at a deleted provider falls back rather than failing the
    /// request: the assistant going quiet is a worse answer to a stale config line
    /// than quietly using the default one.
    pub fn provider_for(&self, task: &str) -> String {
        self.tasks
            .get(task)
            .filter(|name| self.providers.contains_key(*name))
            .cloned()
            .unwrap_or_else(|| self.default.clone())
    }

    /// Task keys that are not real tasks, for the warning at load time.
    pub fn unknown_tasks(&self) -> Vec<&String> {
        self.tasks.keys().filter(|k| !AI_TASKS.contains(&k.as_str())).collect()
    }
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
        // Claude over the API, for people who have a key rather than the CLI.
        providers.insert(
            "claude-api".into(),
            Provider::Anthropic {
                base_url: default_anthropic_url(),
                model: "claude-opus-4-8".into(),
                api_key_env: default_anthropic_key_env(),
                max_tokens: default_anthropic_max_tokens(),
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
        Self { default: "claude".into(), providers, timeout_secs: 120, tasks: HashMap::new() }
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

/// The colours the leader layer speaks in: the which-key panel draws with these
/// today, and the ZSA keyboard's LEDs will paint the same tree with the same ones.
///
/// Derived from the theme rather than configured. Two surfaces showing the same tree
/// in different colours is worse than only having one of them, and an option that can
/// be set to disagree with itself is an option that eventually will be.
pub struct LeaderPalette {
    /// A key that opens another level.
    pub group: Rgb,
    /// A key that runs something.
    pub leaf: Rgb,
    /// The description beside a leaf key.
    pub text: Rgb,
    /// The header, and anything that is context rather than choice.
    pub dim: Rgb,
    /// The panel's own background: the terminal's, lifted just off it so the panel
    /// reads as a surface sitting on top rather than a hole in the output.
    pub background: Rgb,
}

impl Rgb {
    /// Blends towards `other` by `t` (0.0 = self, 1.0 = other).
    fn mix(self, other: Rgb, t: f32) -> Rgb {
        let f = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8;
        Rgb(f(self.0, other.0), f(self.1, other.1), f(self.2, other.2))
    }
}

impl Theme {
    pub fn leader_palette(&self) -> LeaderPalette {
        LeaderPalette {
            // The accent is what the rest of runnir's chrome already uses to mean
            // "this is ours, and it leads somewhere".
            group: self.accent,
            // Bright yellow (ANSI 11) is the key cap colour every which-key uses; fall
            // back to the foreground on a theme that somehow ships a short palette.
            leaf: self.ansi.get(11).copied().unwrap_or(self.foreground),
            text: self.foreground,
            dim: self.dim,
            background: self.background.mix(self.foreground, 0.07),
        }
    }
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
        // Say it once, at load. A mistyped task key would otherwise route that task
        // to the default silently and for ever — the failure has no symptom.
        for key in self.ai.unknown_tasks() {
            eprintln!(
                "runnir: [ai.tasks] has no task {key:?} (known: {})",
                AI_TASKS.join(", ")
            );
        }
        for (task, provider) in &self.ai.tasks {
            if !self.ai.providers.contains_key(provider) {
                eprintln!(
                    "runnir: [ai.tasks] {task} points at provider {provider:?}, which is not \
                     configured — that task falls back to {:?}",
                    self.ai.default
                );
            }
        }
        self.font.size = self.font.size.clamp(4.0, 200.0);
        self.window.opacity = self.window.opacity.clamp(0.1, 1.0);
        self.window.padding = self.window.padding.clamp(0.0, 200.0);
        self.behaviour.wheel_lines = self.behaviour.wheel_lines.clamp(1.0, 50.0);
        self.scrollback.lines = self.scrollback.lines.min(1_000_000);
        // A zero-width preview draws nothing; an absurd one would fill the pane.
        self.watch.max_width = self.watch.max_width.clamp(1, 1000);
        // Keep the media overlay's bar count and art size sane (cava rejects extremes,
        // and a huge art box would overflow the panel).
        self.media.bars = self.media.bars.clamp(1, 512);
        self.media.art_cells = self.media.art_cells.clamp(4, 40);
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

    /// The palette is DERIVED, so the theme is the only place a colour is chosen.
    /// The which-key panel and (next) the keyboard both read this one.
    #[test]
    fn the_leader_palette_comes_from_the_theme() {
        let mut t = Theme::default();
        t.accent = Rgb(1, 2, 3);
        t.foreground = Rgb(200, 201, 202);
        t.dim = Rgb(9, 9, 9);
        t.ansi[11] = Rgb(250, 240, 10);
        let p = t.leader_palette();
        assert_eq!(p.group, Rgb(1, 2, 3), "groups take the chrome accent");
        assert_eq!(p.leaf, Rgb(250, 240, 10), "leaves take bright yellow (ANSI 11)");
        assert_eq!(p.text, t.foreground);
        assert_eq!(p.dim, t.dim);
    }

    /// The panel background has to sit just off the terminal's, or the panel reads as
    /// a hole punched in the output rather than a surface on top of it.
    #[test]
    fn the_panel_background_is_lifted_off_the_terminal_background() {
        let t = Theme::default();
        let p = t.leader_palette();
        assert_ne!(p.background, t.background, "identical would be invisible");
        let lift = |a: u8, b: u8| b as i32 - a as i32;
        assert!(
            lift(t.background.0, p.background.0) > 0,
            "a dark theme lifts towards its foreground: {:?} -> {:?}",
            t.background,
            p.background
        );

        // And on a LIGHT theme it has to move the other way, or the panel vanishes.
        let light = Theme { background: Rgb(250, 250, 250), foreground: Rgb(20, 20, 20), ..Theme::default() };
        assert!(light.leader_palette().background.0 < 250);
    }

    /// A theme supplied by hand can carry a short `ansi` list; asking for colour 11
    /// must not panic on it.
    #[test]
    fn a_short_ansi_palette_falls_back_instead_of_panicking() {
        let t = Theme { ansi: vec![Rgb(0, 0, 0)], foreground: Rgb(7, 7, 7), ..Theme::default() };
        assert_eq!(t.leader_palette().leaf, Rgb(7, 7, 7));
    }

    #[test]
    fn leader_timeout_zero_means_the_layer_never_lapses() {
        let mut cfg = Config::default();
        assert_eq!(crate::leader_timeout(&cfg), Some(std::time::Duration::from_secs(10)));
        cfg.leader_timeout = 0;
        assert_eq!(crate::leader_timeout(&cfg), None);
        cfg.leader_timeout = 45;
        assert_eq!(crate::leader_timeout(&cfg), Some(std::time::Duration::from_secs(45)));
    }

    /// The lapse is one question asked in two places (the deadline wake and the
    /// half-second sweep), and both have to answer it the same way — a layer that
    /// one of them thinks is still armed is a keyboard still painted.
    #[test]
    fn a_leader_lapses_only_when_it_is_armed_and_its_step_ran_out() {
        use std::time::{Duration, Instant};
        let long_ago = Instant::now() - Duration::from_secs(30);
        assert!(crate::leader_lapsed(Some(long_ago), Some(Duration::from_secs(10))));
        // Nothing armed cannot lapse…
        assert!(!crate::leader_lapsed(None, Some(Duration::from_secs(10))));
        // …and with the timeout off the layer waits as long as the user does.
        assert!(!crate::leader_lapsed(Some(long_ago), None));
        assert!(!crate::leader_lapsed(Some(Instant::now()), Some(Duration::from_secs(10))));
    }

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
    fn watch_block_round_trips_through_toml_and_json() {
        // The auto-preview settings must survive a full serialize→parse cycle in both
        // formats, and a missing block must fall back to the (disabled) defaults.
        let mut cfg = Config::default();
        cfg.watch.enabled = true;
        cfg.watch.directory = Some("~/comfyui/output".into());
        cfg.watch.extensions = vec!["png".into(), "webp".into()];
        cfg.watch.max_width = 60;

        let toml_text = toml::to_string_pretty(&cfg).unwrap();
        let from_toml: Config = toml::from_str(&toml_text).expect("watch must re-parse from toml");
        assert!(from_toml.watch.enabled);
        assert_eq!(from_toml.watch.directory.as_deref(), Some("~/comfyui/output"));
        assert_eq!(from_toml.watch.extensions, vec!["png".to_string(), "webp".to_string()]);
        assert_eq!(from_toml.watch.max_width, 60);

        let json_text = serde_json::to_string(&cfg).unwrap();
        let from_json: Config = serde_json::from_str(&json_text).expect("watch must re-parse from json");
        assert_eq!(from_json.watch.max_width, 60);

        // A file with no [watch] block leaves the watcher off.
        let none: Config = toml::from_str("[font]\nsize = 15.0\n").unwrap();
        assert!(!none.watch.enabled, "absent block defaults to disabled");
        assert_eq!(none.watch.max_width, 40);
    }

    #[test]
    fn media_block_round_trips_and_defaults() {
        // The now-playing settings survive both formats, and a missing block defaults.
        let mut cfg = Config::default();
        assert!(cfg.media.waveform, "waveform is on by default");
        assert_eq!(cfg.media.bars, 24);
        assert_eq!(cfg.media.art_cells, 18);

        cfg.media.waveform = false;
        cfg.media.bars = 32;
        cfg.media.art_cells = 24;
        let toml_text = toml::to_string_pretty(&cfg).unwrap();
        let from_toml: Config = toml::from_str(&toml_text).expect("media must re-parse from toml");
        assert!(!from_toml.media.waveform);
        assert_eq!(from_toml.media.bars, 32);
        let json_text = serde_json::to_string(&cfg).unwrap();
        let from_json: Config = serde_json::from_str(&json_text).expect("media must re-parse from json");
        assert_eq!(from_json.media.art_cells, 24);

        let none: Config = toml::from_str("[font]\nsize = 15.0\n").unwrap();
        assert!(none.media.waveform, "absent block defaults to waveform on");
        assert_eq!(none.media.bars, 24);
    }

    #[test]
    fn media_values_are_clamped() {
        let mut cfg = Config::default();
        cfg.media.bars = 0;
        cfg.media.art_cells = 0;
        cfg.validate();
        assert_eq!(cfg.media.bars, 1);
        assert_eq!(cfg.media.art_cells, 4);
        cfg.media.bars = 100_000;
        cfg.media.art_cells = 100_000;
        cfg.validate();
        assert_eq!(cfg.media.bars, 512);
        assert_eq!(cfg.media.art_cells, 40);
    }

    #[test]
    fn watch_max_width_is_clamped() {
        let mut cfg = Config::default();
        cfg.watch.max_width = 0;
        cfg.validate();
        assert_eq!(cfg.watch.max_width, 1, "a zero-width preview is unusable");
        cfg.watch.max_width = 100_000;
        cfg.validate();
        assert_eq!(cfg.watch.max_width, 1000, "an absurd width is capped");
    }

    #[test]
    fn empty_file_yields_defaults() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.font.size, 16.0);
        assert_eq!(cfg.ai.default, "claude");
        // Clipboard-history defaults: on, capacity 50.
        assert!(cfg.clipboard.enabled);
        assert_eq!(cfg.clipboard.capacity, 50);
    }

    #[test]
    fn clipboard_cfg_round_trips_json_and_toml() {
        let mut cfg = Config::default();
        cfg.clipboard.capacity = 7;
        cfg.clipboard.enabled = false;

        let json = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.clipboard.capacity, 7);
        assert!(!back.clipboard.enabled);

        let toml_text = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&toml_text).unwrap();
        assert_eq!(back.clipboard.capacity, 7);
        assert!(!back.clipboard.enabled);

        // An absent block falls back to defaults, like every other section.
        let partial: Config = toml::from_str("[font]\nsize = 18.0\n").unwrap();
        assert_eq!(partial.clipboard.capacity, 50);
        assert!(partial.clipboard.enabled);
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


    /// Per-task providers exist for economics: a session summary is long and cheap on
    /// a flat-rate subscription, while a one-line command translation wants latency.
    #[test]
    fn a_task_override_wins_over_the_default() {
        let mut cfg = Config::default();
        cfg.ai.default = "claude".into();
        cfg.ai.tasks.insert("command".into(), "claude-api".into());
        assert_eq!(cfg.ai.provider_for("command"), "claude-api");
        assert_eq!(cfg.ai.provider_for("summarize"), "claude", "unnamed tasks keep the default");
    }

    /// An override naming a provider that no longer exists must fall back, not fail:
    /// a silent assistant is a worse answer to a stale config line than the default.
    #[test]
    fn an_override_pointing_nowhere_falls_back() {
        let mut cfg = Config::default();
        cfg.ai.default = "claude".into();
        cfg.ai.tasks.insert("fix".into(), "deleted-provider".into());
        assert_eq!(cfg.ai.provider_for("fix"), "claude");
    }

    /// A typo in a task key has no symptom at request time — it just uses the
    /// default for ever — so it has to be caught at load.
    #[test]
    fn an_unknown_task_key_is_reported() {
        let mut cfg = Config::default();
        cfg.ai.tasks.insert("sumarize".into(), "claude".into());
        assert_eq!(cfg.ai.unknown_tasks(), vec![&"sumarize".to_string()]);
        cfg.ai.tasks.clear();
        for task in AI_TASKS {
            cfg.ai.tasks.insert(task.into(), "claude".into());
        }
        assert!(cfg.ai.unknown_tasks().is_empty(), "every real task name is accepted");
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
    fn snippets_default_to_empty_and_round_trip() {
        // Absent = empty list, never an error.
        let cfg: Config = toml::from_str("").unwrap();
        assert!(cfg.snippets.is_empty());

        // A full entry survives TOML round-trip, defaults and all.
        let text = r#"
[[snippets]]
name = "deploy"
command = "git push && ssh server deploy.sh"
description = "ship the current branch"
run_now = true

[[snippets]]
name = "logs"
command = "journalctl -fu runnir"
"#;
        let cfg: Config = toml::from_str(text).unwrap();
        assert_eq!(cfg.snippets.len(), 2);
        assert_eq!(cfg.snippets[0].name, "deploy");
        assert_eq!(cfg.snippets[0].command, "git push && ssh server deploy.sh");
        assert_eq!(cfg.snippets[0].description, "ship the current branch");
        assert!(cfg.snippets[0].run_now);
        // description + run_now are optional: missing means empty / false.
        assert_eq!(cfg.snippets[1].description, "");
        assert!(!cfg.snippets[1].run_now, "run_now must default to false");

        // The generated file must re-parse (both JSON and TOML), so --write-config
        // never emits something the loader rejects.
        let toml_text = toml::to_string_pretty(&cfg).unwrap();
        let via_toml: Config = toml::from_str(&toml_text).unwrap();
        assert_eq!(via_toml.snippets.len(), 2);
        let json_text = serde_json::to_string(&cfg).unwrap();
        let via_json: Config = serde_json::from_str(&json_text).unwrap();
        assert_eq!(via_json.snippets[0].name, "deploy");
        assert!(via_json.snippets[0].run_now);
    }

    #[test]
    fn unknown_keys_are_rejected_rather_than_ignored() {
        // Silently swallowing a typo means the setting never takes effect and the
        // user has no way to tell.
        let err = toml::from_str::<Config>("[font]\nfamly = \"x\"\n");
        assert!(err.is_err(), "a misspelled key must be reported");
    }
}
