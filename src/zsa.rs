//! The ZSA keyboard's layout, read so runnir can say WHICH LED sits under a key it
//! has a binding for.
//!
//! Lighting the leader layer on the board needs one translation: `g` opens the git
//! panel, and the Keymapp API wants an LED index. That translation turned out to be
//! free — **the LED index is the key index in the Oryx layout** (measured on a
//! Moonlander MK1: `keys[19]` is `KC_G` and LED 19 is the `g` key) — so all this
//! module does is read the layout and look a code up in it.
//!
//! The layout comes from Keymapp's own database, READ-ONLY. It is another
//! application's file and runnir has no business writing to it. Chosen over pointing
//! at an exported file in runnir's config because it needs no setup and cannot go
//! stale when the board is reflashed; the price is that Keymapp may change its schema
//! one day, so every step here fails soft and the feature simply stops existing.
//!
//! Nothing in this module talks to the keyboard. It is pure lookup plus one read of a
//! file, which is what makes it testable with no hardware attached.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// LEDs on a Moonlander MK1, and therefore keys per layer in its layout.
///
/// Measured, because the API's `.proto` documents no indices: each half is numbered
/// on its own (0–35 left, 36–71 right) and runs left-to-right in PHYSICAL space, so
/// index 0 is the outer top-left key but index 36 is the INNER top-right one. Rows
/// are 7, 7, 7, 6, 5 keys plus a 4-key thumb cluster.
pub const LEDS: usize = 72;

/// A key that defers to the layer below it. Resolving a code has to walk down past
/// these or a layer of mostly-transparent keys looks empty.
const TRANSPARENT: &str = "KC_TRANSPARENT";

/// One flashed layout: the tap code of every key, per layer, indexed by key (= LED).
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    layers: Vec<Vec<Option<String>>>,
}

impl Layout {
    /// Parses the JSON blob Keymapp stores per revision.
    ///
    /// Tolerates a layer that is not `LEDS` long rather than rejecting the layout:
    /// a future board with a different key count should degrade to "some keys cannot
    /// be lit", not to "the feature is broken".
    pub fn from_json(json: &str) -> Option<Layout> {
        let v: serde_json::Value = serde_json::from_str(json).ok()?;
        let layers = v.get("layout")?.get("revision")?.get("layers")?.as_array()?;
        let layers: Vec<Vec<Option<String>>> = layers
            .iter()
            .map(|l| {
                l.get("keys")
                    .and_then(|k| k.as_array())
                    .map(|keys| {
                        keys.iter()
                            .map(|k| {
                                k.get("tap")
                                    .and_then(|t| t.get("code"))
                                    .and_then(|c| c.as_str())
                                    .map(str::to_string)
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .collect();
        (!layers.is_empty()).then_some(Layout { layers })
    }

    pub fn layers(&self) -> usize {
        self.layers.len()
    }

    /// The code every key emits with `layer` active, transparency resolved.
    ///
    /// QMK resolves a transparent key against the layer stack; runnir only learns the
    /// top layer from `GetStatus`, so this walks down towards layer 0. That is right
    /// whenever the stack is "base plus one", which is what a layer switch on this
    /// board is, and it is the only reading available from what the API reports.
    fn effective(&self, layer: usize) -> Vec<Option<&str>> {
        let top = layer.min(self.layers.len().saturating_sub(1));
        let mut out = vec![None; LEDS];
        for (i, slot) in out.iter_mut().enumerate() {
            for l in (0..=top).rev() {
                match self.layers[l].get(i).and_then(|c| c.as_deref()) {
                    None | Some(TRANSPARENT) => continue,
                    Some(code) => {
                        *slot = Some(code);
                        break;
                    }
                }
            }
        }
        out
    }

    /// The LED under the key that emits `spelling` (a config key spelling: `g`, `0`,
    /// `equal`, `left`) with `layer` active, or `None` when this board cannot type it.
    ///
    /// A binding whose key is not on the board is not an error worth reporting: on a
    /// layout without a `PgDn` key, the `pagedown` binding simply lights nothing.
    ///
    /// Test-only for now: painting works a whole level at a time (`leds_for`), and a
    /// public single-key lookup nothing calls is a public API nothing keeps honest.
    #[cfg(test)]
    pub fn led_for(&self, spelling: &str, layer: usize) -> Option<u8> {
        find(&Key::of(spelling)?, &self.effective(layer))
    }

    /// Every `(spelling, LED)` this board can light, for a whole leader level at once.
    /// Resolving the layer is the expensive half, so a level costs one pass, not one
    /// per key.
    pub fn leds_for<'a>(
        &self,
        spellings: impl IntoIterator<Item = &'a str>,
        layer: usize,
    ) -> Vec<(&'a str, u8)> {
        let eff = self.effective(layer);
        spellings.into_iter().filter_map(|s| Some((s, find(&Key::of(s)?, &eff)?))).collect()
    }
}

/// The LED for `want` on an already-resolved layer.
///
/// Exact code first, so a board that really has `KC_MINUS` never answers with a
/// same-family guess. Failing that, the earliest family that matches wins — `plus`
/// prefers a real `+` key and only falls back to the `=` key it is shifted from —
/// and within one family the lower LED, so a repeat of the same code (two spaces,
/// two shifts) always lights the same key instead of moving about.
fn find(want: &Key, eff: &[Option<&str>]) -> Option<u8> {
    if let Some(i) = eff.iter().position(|c| *c == Some(want.code.as_str())) {
        return Some(i as u8);
    }
    eff.iter()
        .enumerate()
        .filter_map(|(i, code)| Some((want.family_rank((*code)?)?, i as u8)))
        .min()
        .map(|(_, led)| led)
}

/// A key runnir wants to light, as the board might spell it.
///
/// QMK's locale headers rename the punctuation of every layout — this Spanish board
/// carries `ES_MINS` and `ES_PLUS` where a US one has `KC_MINUS` and no plus key at
/// all — so an exact code is not enough and enumerating locales would never end.
/// `family` is the part after the underscore, matched against any `XX_` prefix, which
/// is the one thing every locale header keeps.
struct Key {
    /// Owned rather than `&'static`: the letter and function-key codes are built per
    /// lookup, and leaking one string per repaint would grow for the whole session.
    code: String,
    family: &'static [&'static str],
}

impl Key {
    /// How good a match `code` is: `Some(0)` for the preferred spelling of this key,
    /// higher for the fallbacks, `None` for a different key entirely. `ES_MINS` ranks
    /// in the `MINUS`/`MINS` family; `KC_G` matches nothing but itself.
    fn family_rank(&self, code: &str) -> Option<usize> {
        let (prefix, rest) = code.split_once('_')?;
        (prefix.len() == 2).then(|| self.family.iter().position(|f| *f == rest))?
    }

    fn of(spelling: &str) -> Option<Key> {
        let s = spelling.trim();
        let mut chars = s.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            // Letters and digits are spelled the same in every locale header.
            if c.is_ascii_alphanumeric() {
                return Some(Key { code: format!("KC_{}", c.to_ascii_uppercase()), family: &[] });
            }
            // Punctuation typed as itself, which is how the leader's font-size keys
            // are bound (`+`, `-`, `=`).
            return match c {
                '-' => Some(Key { code: "KC_MINUS".to_string(), family: &["MINUS", "MINS"] }),
                // A US board has no `+` key at all: it is shift on the `=` key, so
                // that is where the light belongs when no real plus exists.
                '+' => Some(Key { code: "KC_PLUS".to_string(), family: &["PLUS", "EQUAL", "EQL"] }),
                '=' => Some(Key { code: "KC_EQUAL".to_string(), family: &["EQUAL", "EQL"] }),
                ',' => Some(Key { code: "KC_COMMA".to_string(), family: &["COMMA", "COMM"] }),
                '.' => Some(Key { code: "KC_DOT".to_string(), family: &["DOT"] }),
                '/' => Some(Key { code: "KC_SLASH".to_string(), family: &["SLASH", "SLSH"] }),
                ';' => Some(Key { code: "KC_SEMICOLON".to_string(), family: &["SEMICOLON", "SCLN"] }),
                '\'' => Some(Key { code: "KC_QUOTE".to_string(), family: &["QUOTE", "QUOT", "APOS"] }),
                '`' => Some(Key { code: "KC_GRAVE".to_string(), family: &["GRAVE", "GRV"] }),
                '\\' => Some(Key { code: "KC_BACKSLASH".to_string(), family: &["BACKSLASH", "BSLS"] }),
                '[' => Some(Key { code: "KC_LEFT_BRACKET".to_string(), family: &["LBRC"] }),
                ']' => Some(Key { code: "KC_RIGHT_BRACKET".to_string(), family: &["RBRC"] }),
                _ => None,
            };
        }
        // Named keys, in runnir's config spelling. Only what a binding can use: a
        // lookup table, not a transcription of QMK's keycode list.
        let key = match s.to_ascii_lowercase().as_str() {
            "enter" | "return" => Key { code: "KC_ENTER".to_string(), family: &["ENTER", "ENT"] },
            "space" => Key { code: "KC_SPACE".to_string(), family: &["SPACE", "SPC"] },
            "tab" => Key { code: "KC_TAB".to_string(), family: &["TAB"] },
            "escape" | "esc" => Key { code: "KC_ESCAPE".to_string(), family: &["ESCAPE", "ESC"] },
            "backspace" => Key { code: "KC_BSPC".to_string(), family: &["BSPC", "BACKSPACE"] },
            "delete" => Key { code: "KC_DELETE".to_string(), family: &["DELETE", "DEL"] },
            "left" => Key { code: "KC_LEFT".to_string(), family: &["LEFT"] },
            "right" => Key { code: "KC_RIGHT".to_string(), family: &["RIGHT", "RGHT"] },
            "up" => Key { code: "KC_UP".to_string(), family: &["UP"] },
            "down" => Key { code: "KC_DOWN".to_string(), family: &["DOWN", "DOWN"] },
            "home" => Key { code: "KC_HOME".to_string(), family: &["HOME"] },
            "end" => Key { code: "KC_END".to_string(), family: &["END"] },
            "pageup" => Key { code: "KC_PAGE_UP".to_string(), family: &["PGUP"] },
            "pagedown" => Key { code: "KC_PAGE_DOWN".to_string(), family: &["PGDN"] },
            "minus" => Key { code: "KC_MINUS".to_string(), family: &["MINUS", "MINS"] },
            "plus" => Key { code: "KC_PLUS".to_string(), family: &["PLUS", "EQUAL", "EQL"] },
            "equal" => Key { code: "KC_EQUAL".to_string(), family: &["EQUAL", "EQL"] },
            "comma" => Key { code: "KC_COMMA".to_string(), family: &["COMMA", "COMM"] },
            "period" | "dot" => Key { code: "KC_DOT".to_string(), family: &["DOT"] },
            "slash" => Key { code: "KC_SLASH".to_string(), family: &["SLASH", "SLSH"] },
            "semicolon" => Key { code: "KC_SEMICOLON".to_string(), family: &["SEMICOLON", "SCLN"] },
            "quote" => Key { code: "KC_QUOTE".to_string(), family: &["QUOTE", "QUOT", "APOS"] },
            "grave" => Key { code: "KC_GRAVE".to_string(), family: &["GRAVE", "GRV"] },
            "backslash" => Key { code: "KC_BACKSLASH".to_string(), family: &["BACKSLASH", "BSLS"] },
            "lbracket" => Key { code: "KC_LEFT_BRACKET".to_string(), family: &["LBRC"] },
            "rbracket" => Key { code: "KC_RIGHT_BRACKET".to_string(), family: &["RBRC"] },
            f if f.starts_with('f') && f[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) => {
                Key { code: format!("KC_{}", f.to_ascii_uppercase()), family: &[] }
            }
            _ => return None,
        };
        Some(key)
    }
}

/// Keymapp's database, where the flashed layouts live.
pub fn default_db() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join(".keymapp/keymapp.sqlite3"))
}

/// Reads one revision's layout out of Keymapp's database.
///
/// `revision` is the half after the slash in the firmware version `GetStatus` reports
/// (`L4g4A/Jad5YO` -> `Jad5YO`), so the layout read is the one actually ON the board.
/// Taking "the newest row" instead would light the keys of a layout that was never
/// flashed, which is worse than lighting nothing.
///
/// Shells out to `sqlite3` rather than linking a SQLite crate: one read of one blob
/// does not justify a C dependency in a terminal, and runnir already treats external
/// tools this way (`git`, `docker`, `kontroll`). No `sqlite3` on the machine means no
/// keyboard integration, silently — the same rule the whole feature follows.
pub fn read_layout(db: &Path, revision: &str) -> Option<Layout> {
    // Whitelisted rather than quoted: this string reaches a SQL statement, and the
    // ids Keymapp uses are short alphanumerics. Anything else is a bug or an attack,
    // and either way the answer is to not run the query.
    if revision.is_empty() || !revision.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let out = std::process::Command::new("sqlite3")
        .arg("-readonly")
        .arg(db)
        .arg(format!("select cast(data as text) from revision where revisionId = '{revision}';"))
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Layout::from_json(&String::from_utf8_lossy(&out.stdout))
}


// ---- driving the board ------------------------------------------------------

/// What the board is asked to do, in the board's own vocabulary.
///
/// Intent-shaped rather than command-line-shaped. The previous version spoke
/// `&["set-rgb", "-l", "19", ...]` because the only transport was a `kontroll`
/// subprocess, which meant every failure had to be recovered from ENGLISH PROSE on
/// stdout — the source of three separate bugs: a refusal that said `Failed` while
/// meaning success, a failure reported on stderr with a zero exit, and a blank first
/// line that swallowed a whole reply. None of them can exist against a byte protocol.
///
/// A trait so the failure modes can be tested with no keyboard on the desk.
trait Runner: Send {
    /// Hands the RGB matrix to the host, or back to the firmware. Nothing painted is
    /// visible until this is on, and turning it off IS the restore: the board goes
    /// back to the colours its own layout asks for, with nothing for us to replay.
    fn control(&mut self, on: bool) -> Result<(), String>;
    /// Every key one colour.
    fn set_all(&mut self, colour: crate::config::Rgb) -> Result<(), String>;
    /// One key, by LED index.
    fn set_led(&mut self, led: u8, colour: crate::config::Rgb) -> Result<(), String>;
    /// Whether the keyboard is there at all. Unplugging mid-session is normal.
    fn alive(&self) -> bool;
}

// ---- the raw HID transport --------------------------------------------------

/// Command codes of ZSA's `oryx` QMK module (`zsa/qmk_modules`, GPL-2.0) — the module
/// Keymapp itself drives, so these are what the board is already listening for.
///
/// Implemented from the published source, not copied from it: what is taken is a wire
/// protocol. Confirmed against the board rather than assumed — it answers
/// `fe 04 fe` to `ProtocolVersion`, which is
/// `[ORYX_EVT_GET_PROTOCOL_VERSION, ORYX_PROTOCOL_VERSION, ORYX_STOP_BIT]` and pins
/// this firmware to exactly this revision of the source.
#[repr(u8)]
enum Op {
    RgbControl = 5,
    SetRgbLed = 6,
    SetRgbLedAll = 9,
}

/// Report size the firmware expects (`RAW_EPSIZE`). Measured: exactly 32 bytes and NO
/// leading report-id byte, the detail Linux hidraw makes easy to get wrong either way.
const REPORT: usize = 32;

/// Usage page `0xFF60` — QMK's raw HID channel, as a report descriptor opens.
const RAW_USAGE_PAGE: [u8; 3] = [0x06, 0x60, 0xff];

/// ZSA's USB vendor id, as `uevent` spells it.
const ZSA_VENDOR: &str = "3297";

/// The board, spoken to directly: no Keymapp, no subprocess, no text to parse.
struct Hid {
    file: Option<std::fs::File>,
}

impl Hid {
    fn new() -> Hid {
        Hid { file: None }
    }

    /// Finds the raw endpoint by DESCRIPTOR, never by a remembered path: `hidrawN`
    /// numbering is handed out in plug order and moves the moment the keyboard is
    /// unplugged, so a cached `/dev/hidraw3` is a path to somebody else's device.
    fn find() -> Option<PathBuf> {
        let mut found: Vec<PathBuf> = Vec::new();
        for entry in std::fs::read_dir("/sys/class/hidraw").ok()?.flatten() {
            let dev = entry.path().join("device");
            if !std::fs::read_to_string(dev.join("uevent")).is_ok_and(|u| u.contains(ZSA_VENDOR)) {
                continue;
            }
            if std::fs::read(dev.join("report_descriptor"))
                .unwrap_or_default()
                .starts_with(&RAW_USAGE_PAGE)
            {
                found.push(PathBuf::from("/dev").join(entry.file_name()));
            }
        }
        // Sorted so a board exposing two raw endpoints is answered the same way every
        // run, rather than by whatever order the directory happened to be read in.
        found.sort();
        found.into_iter().next()
    }

    /// The open device, opening it on first use and again after it has gone away.
    fn file(&mut self) -> Result<&mut std::fs::File, String> {
        if self.file.is_none() {
            let path = Self::find().ok_or("no ZSA raw HID endpoint")?;
            self.file = Some(
                std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&path)
                    .map_err(|e| format!("{}: {e}", path.display()))?,
            );
        }
        self.file.as_mut().ok_or_else(|| "no device".to_string())
    }

    /// One report. A failed write drops the handle so the next call reopens, which is
    /// the whole reconnect story now that no daemon sits in the middle.
    fn send(&mut self, body: &[u8]) -> Result<(), String> {
        use std::io::Write;
        let mut report = [0u8; REPORT];
        let n = body.len().min(REPORT);
        report[..n].copy_from_slice(&body[..n]);
        let file = self.file()?;
        file.write_all(&report).map_err(|e| {
            self.file = None;
            format!("write: {e}")
        })
    }
}

impl Runner for Hid {
    fn control(&mut self, on: bool) -> Result<(), String> {
        self.send(&[Op::RgbControl as u8, u8::from(on)])
    }

    fn set_all(&mut self, c: crate::config::Rgb) -> Result<(), String> {
        self.send(&[Op::SetRgbLedAll as u8, c.0, c.1, c.2])
    }

    fn set_led(&mut self, led: u8, c: crate::config::Rgb) -> Result<(), String> {
        self.send(&[Op::SetRgbLed as u8, led, c.0, c.1, c.2])
    }

    fn alive(&self) -> bool {
        self.file.is_some() || Hid::find().is_some()
    }
}

/// What the keyboard is asked to do. Only the LAST one queued matters, which is what
/// makes a repaint per keystroke affordable: descending three levels quickly paints
/// once, at the level you stopped on.
enum Cmd {
    /// `dim` for every key, then `leds` on top. One level of the leader layer.
    Paint { leds: Vec<(u8, crate::config::Rgb)>, dim: crate::config::Rgb, sustain_ms: u32 },
    /// The whole board one colour for `ms`, then Keymapp hands it back on its own.
    Flash { colour: crate::config::Rgb, ms: u32 },
    Restore,
    Stop,
}

/// A handle to the keyboard, if there is one. Every method returns immediately: the
/// work happens on a worker thread, because a paint is ~4 ms per key of process spawn
/// and the UI thread cannot spend that.
pub struct Board {
    tx: std::sync::mpsc::Sender<Cmd>,
    /// The layer the board last said it was on. Kept here because asking costs a
    /// subprocess and the answer is wanted on a keystroke; see `refresh_layer`.
    layer: Arc<AtomicUsize>,
    /// Whether a reading of that is already out.
    asking: Arc<AtomicBool>,
    /// The flashed layout, once it has been read. `None` until Keymapp answers —
    /// which it cannot do while it is not running, and it is the USER's app: it
    /// starts after us, it exits on its own, and the keyboard gets unplugged. Held
    /// here rather than beside the window so that a failed reading is a state to
    /// retry, not a verdict for the life of the process.
    layout: Arc<Mutex<Option<Layout>>>,
    /// Whether a reading of THAT is already out.
    asking_layout: Arc<AtomicBool>,
}

impl Board {
    /// Starts the worker, or `None` when there is no ZSA board on this machine.
    ///
    /// Absence is not an error, the same rule docker and the credential helper follow:
    /// the feature simply does not exist. What is gone from this check is Keymapp —
    /// the board is reached through its own HID endpoint now, so nothing has to be
    /// running for the lights to work.
    pub fn start() -> Option<Board> {
        Hid::find()?;
        let board = Board::with_runner(Box::new(Hid::new()));
        // Take off whatever a previous runnir left on the board. It is the half of the
        // dead-man switch a timer of our own cannot cover: killed outright while the
        // leader was armed, nothing ran to put the colours back, and they would sit
        // there until something did. This is that something.
        board.restore();
        Some(board)
    }

    fn with_runner(runner: Box<dyn Runner>) -> Board {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || worker(rx, runner));
        Board {
            tx,
            layer: Arc::new(AtomicUsize::new(0)),
            asking: Arc::new(AtomicBool::new(false)),
            layout: Arc::new(Mutex::new(None)),
            asking_layout: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The flashed layout, if one has been read. Never blocks and never asks.
    pub fn known_layout(&self) -> Option<Layout> {
        self.layout.lock().ok()?.clone()
    }

    /// Starts a reading of the layout in the background, for the NEXT paint.
    ///
    /// Costs two blocking calls (`kontroll status`, then the sqlite read), so it can
    /// never be done inline: this runs on the leader's arming keystroke, and Keymapp
    /// wedges — its socket stays on disk while it answers nothing. Doing it here on
    /// the UI thread is the freeze the layer reading already had to be moved off.
    ///
    /// Retried rather than done once at startup, because "Keymapp was not up when
    /// this window opened" is the normal case, not a broken one: it is the user's
    /// app, so it starts late, quits on its own, and follows the keyboard in and out
    /// of its socket. A window that gave up at launch stayed dark for hours.
    pub fn refresh_layout(&self) {
        if self.layout.lock().is_ok_and(|l| l.is_some()) {
            return;
        }
        // No socket, no Keymapp: skip the subprocess entirely. This runs on every
        // arming of the leader while the board is dark, and spawning a `kontroll`
        // only to watch it fail is a cost paid on a keystroke for nothing.
        //
        // Said out loud under the debug flag: this is now the commonest reason the
        // board stays dark, and a silent early return here is exactly the blindness
        // that made the original bug take a session to see.
        if !socket_path().is_some_and(|p| p.exists()) {
            if std::env::var("RUNNIR_ZSA_DEBUG").is_ok() {
                eprintln!("zsa: no Keymapp socket yet; will retry on the next leader");
            }
            return;
        }
        ask_layout(self.layout.clone(), self.asking_layout.clone(), read_status);
    }

    /// The layer to resolve keys against right now, without asking the board.
    ///
    /// Layer 0 until something says otherwise, which is the base layer and also the
    /// right answer for any layer above it that leaves the letters transparent —
    /// `effective` walks down to it. That makes "we have not been told yet" and the
    /// ordinary case the same paint, so nothing has to wait to light correctly.
    pub fn known_layer(&self) -> usize {
        self.layer.load(Ordering::Relaxed)
    }

    /// Starts a reading of the current layer in the background, for the NEXT paint.
    ///
    /// `status` is a `kontroll` subprocess with no timeout, and Keymapp does wedge —
    /// its socket stays on disk while it answers nothing. Called from the leader's
    /// arming keystroke, so doing it inline froze the whole window on the keypress
    /// that was supposed to light the keyboard up.
    pub fn refresh_layer(&self) {
        ask_layer(self.layer.clone(), self.asking.clone(), read_status);
    }

    /// Lights `leds` over a `dim` background. `sustain_ms` is a DEAD-MAN SWITCH: after
    /// it elapses Keymapp restores the whole board on its own, so a `kill -9`, a panic
    /// or a crash cannot leave the keyboard wrong. Measured, because it is documented
    /// nowhere: sustain expires the ENTIRE board, not the single LED it was passed with.
    pub fn paint(&self, leds: Vec<(u8, crate::config::Rgb)>, dim: crate::config::Rgb, sustain_ms: u32) {
        let _ = self.tx.send(Cmd::Paint { leds, dim, sustain_ms });
    }

    /// The whole board one colour, briefly: a signal that needs no key to be
    /// identified, which is the only kind an opaque-keycap board can actually carry
    /// (see the DEVLOG entry on why the lit leader layer was abandoned).
    ///
    /// One call and no cleanup: sustain expires the WHOLE board, so the flash undoes
    /// itself even if runnir dies in the middle of it.
    pub fn flash(&self, colour: crate::config::Rgb, ms: u32) {
        let _ = self.tx.send(Cmd::Flash { colour, ms });
    }

    /// Gives the board back its own colours. Sent on disarm, on focus loss and on exit
    /// — the sustain above covers only the case where runnir never gets to do this.
    pub fn restore(&self) {
        let _ = self.tx.send(Cmd::Restore);
    }
}

impl Drop for Board {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

fn kontroll_path() -> Option<PathBuf> {
    // `cargo install` is how kontroll is usually got, and ~/.cargo/bin is often absent
    // from the environment a compositor hands a GUI app.
    let home = dirs::home_dir();
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .chain(home.map(|h| h.join(".cargo/bin")))
        .map(|dir| dir.join("kontroll"))
        .find(|p| p.is_file())
}

/// Drives the board, and owns the dead-man switch now that Keymapp does not.
///
/// `sustain` was a field on Keymapp's request and a timer inside Keymapp: the board
/// came back on its own even if runnir died mid-paint. The firmware has no such thing
/// — its opcode carries no time — so the deadline is kept here, and the wait for the
/// next command doubles as the wait for it to expire. What a timer of our own cannot
/// survive is runnir being killed outright, which is what the restore at startup is
/// for.
fn worker(rx: std::sync::mpsc::Receiver<Cmd>, mut runner: Box<dyn Runner>) {
    // Paint is on the board and must come off at this instant unless something takes
    // it off first. `None` means the board is showing its own colours.
    let mut deadline: Option<std::time::Instant> = None;
    loop {
        let next = match deadline {
            None => rx.recv().ok(),
            Some(at) => {
                let left = at.saturating_duration_since(std::time::Instant::now());
                match rx.recv_timeout(left) {
                    Ok(c) => Some(c),
                    // Nothing came and the paint outlived its welcome: take it off.
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        restore(&mut *runner);
                        deadline = None;
                        continue;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => None,
                }
            }
        };
        let Some(mut cmd) = next else { break };
        // Coalesce: a burst of paints while a level is being walked collapses to the
        // last one. Repainting 72 keys per keystroke is a race that cannot be won.
        while let Ok(later) = rx.try_recv() {
            cmd = later;
        }
        match cmd {
            Cmd::Stop => {
                restore(&mut *runner);
                return;
            }
            Cmd::Restore => {
                restore(&mut *runner);
                deadline = None;
            }
            Cmd::Flash { colour, ms } => {
                if take_control(&mut *runner) && runner.set_all(colour).is_ok() {
                    deadline = Some(after(ms));
                }
            }
            Cmd::Paint { leds, dim, sustain_ms } => {
                if !take_control(&mut *runner) {
                    continue;
                }
                // The background first, so a key that is no longer part of this level
                // goes dark in the same pass that lights the ones that are.
                if runner.set_all(dim).is_err() {
                    continue;
                }
                deadline = Some(after(sustain_ms));
                for (led, colour) in &leds {
                    if runner.set_led(*led, *colour).is_err() {
                        // The board went away mid-paint (unplugged). Finishing the
                        // loop would be seventy more writes to a handle that is gone.
                        break;
                    }
                }
            }
        }
    }
    // The channel closed because the handle was dropped: put the board back before the
    // thread ends, or the last paint outlives the window that asked for it.
    restore(&mut *runner);
}

fn after(ms: u32) -> std::time::Instant {
    std::time::Instant::now() + std::time::Duration::from_millis(ms.into())
}

/// Hands the RGB matrix back to the firmware. This IS the restore: the board returns
/// to the colours its own layout asks for, with nothing to remember or replay.
fn restore(runner: &mut dyn Runner) {
    if runner.alive() {
        let _ = runner.control(false);
    }
}

/// Takes the matrix, so that what is painted next is visible at all. Sent every time
/// rather than tracked: it is one report, and a remembered "we already own it" goes
/// stale the moment the keyboard is unplugged and re-enumerated.
fn take_control(runner: &mut dyn Runner) -> bool {
    runner.alive() && runner.control(true).is_ok()
}

/// What the board says it is running: the flashed layout's revision, and the layer
/// that is active right now.
///
/// The revision matters because Keymapp's database holds every revision ever
/// compiled; reading "the newest row" would light the keys of a layout that was never
/// put on the board. The firmware answers `L4g4A/Jad5YO`, and the half after the
/// slash is the revision id.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub revision: String,
    pub layer: usize,
}

/// Parses `kontroll status`. Its output is a handful of `Label:\tvalue` lines.
fn parse_status(out: &str) -> Option<Status> {
    let mut revision = None;
    let mut layer = 0;
    for line in out.lines() {
        // Skip, never bail: kontroll's output carries blank lines between blocks, and
        // a `?` here made one blank line discard the whole status — so the revision
        // was always None, the layout never loaded, and the board never lit. The test
        // that "covered" this used a hand-written sample with no blank lines in it.
        let Some((label, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        match label.trim() {
            "Firmware version" => {
                // `hashId/revisionId`; a board with no layout reports a bare `/`.
                let rev = value.rsplit('/').next().unwrap_or_default();
                if !rev.is_empty() {
                    revision = Some(rev.to_string());
                }
            }
            "Current layer" => layer = value.parse().unwrap_or(0),
            _ => {}
        }
    }
    Some(Status { revision: revision?, layer })
}

/// Reads one layer at a time into `layer`, on a thread, never twice at once.
///
/// The "never twice" is the point: with Keymapp wedged the read never returns, and
/// one thread per arming keystroke would pile up threads on a tool that has already
/// stopped answering. One is stuck, the layer keeps its last value, and the leader
/// lights carry on painting where they last knew the keys to be.
fn ask_layer(
    layer: Arc<AtomicUsize>,
    asking: Arc<AtomicBool>,
    read: impl FnOnce() -> Option<Status> + Send + 'static,
) {
    if asking.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        if let Some(status) = read() {
            layer.store(status.layer, Ordering::Relaxed);
        }
        asking.store(false, Ordering::SeqCst);
    });
}

/// Reads the flashed layout off Keymapp's database, on a thread of its own.
///
/// Mirrors `ask_layer`: one reading in flight at a time, and a failure leaves the
/// slot empty so the next arming of the leader tries again. That retry is the whole
/// point — the reading fails for as long as Keymapp is down, and then succeeds.
fn ask_layout(
    layout: Arc<Mutex<Option<Layout>>>,
    asking: Arc<AtomicBool>,
    read: impl FnOnce() -> Option<Status> + Send + 'static,
) {
    if asking.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let debug = std::env::var("RUNNIR_ZSA_DEBUG").is_ok();
        match read().and_then(|s| Some((default_db()?, s.revision))) {
            Some((db, revision)) => {
                let got = read_layout(&db, &revision);
                if debug {
                    match &got {
                        Some(l) => eprintln!("zsa: layout {revision} loaded, {} layers", l.layers()),
                        None => eprintln!("zsa: layout {revision} not in {}", db.display()),
                    }
                }
                if let Ok(mut slot) = layout.lock() {
                    *slot = got;
                }
            }
            None if debug => {
                eprintln!("zsa: kontroll status gave no revision (is Keymapp running?)")
            }
            None => {}
        }
        asking.store(false, Ordering::SeqCst);
    });
}

/// Where Keymapp listens on Linux. The port 50051 its own window names is Windows
/// only — see the wiki page; assuming otherwise cost a session.
pub fn socket_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join(".keymapp/keymapp.sock"))
}

/// Runs `kontroll status` and reads its answer. Blocking, with no timeout of its own.
fn read_status() -> Option<Status> {
    let bin = kontroll_path()?;
    let out = std::process::Command::new(&bin).arg("status").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    // kontroll reports a failed connection on STDERR with a non-zero exit, and
    // reading only stdout turned that into an empty string and a shrug. Worth
    // knowing that `XDG_CONFIG_HOME` decides where kontroll looks for Keymapp's
    // socket, so a sandbox that redirects it cannot reach the keyboard at all.
    if std::env::var("RUNNIR_ZSA_DEBUG").is_ok() {
        let err = String::from_utf8_lossy(&out.stderr);
        eprintln!(
            "zsa: {} status (exit {:?})\n  out: {}\n  err: {}",
            bin.display(),
            out.status.code(),
            text.trim_end(),
            err.trim_end()
        );
    }
    if !out.status.success() {
        return None;
    }
    parse_status(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A failed layout reading has to leave the slot EMPTY and the in-flight flag
    /// down, so the next arming of the leader asks again. This is the whole fix:
    /// Keymapp is the user's app, so "not running yet" is the normal state at the
    /// moment a window opens, and reading the layout once at startup meant such a
    /// window never lit the board again for as long as it lived.
    #[test]
    fn a_layout_reading_that_fails_is_retried_not_final() {
        let slot: Arc<Mutex<Option<Layout>>> = Arc::new(Mutex::new(None));
        let asking = Arc::new(AtomicBool::new(false));

        // Keymapp down: no status, so no layout — and nothing latched.
        ask_layout(slot.clone(), asking.clone(), || None);
        for _ in 0..200 {
            if !asking.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(slot.lock().unwrap().is_none(), "nothing to show for a failed read");
        assert!(!asking.load(Ordering::SeqCst), "the flag must come back down, or no retry ever runs");

        // Only one reading at a time: a second ask while one is in flight is dropped
        // rather than piling threads onto a tool that is already not answering.
        asking.store(true, Ordering::SeqCst);
        let ran = Arc::new(AtomicBool::new(false));
        let seen = ran.clone();
        ask_layout(slot.clone(), asking.clone(), move || {
            seen.store(true, Ordering::SeqCst);
            None
        });
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert!(!ran.load(Ordering::SeqCst), "a second reading must not start while one is out");
    }

    /// Builds a layout JSON the way Keymapp stores one: `codes[layer][key]`, where an
    /// entry of `None` is a key with no tap action at all.
    fn layout_json(codes: &[Vec<Option<&str>>]) -> String {
        let layers: Vec<serde_json::Value> = codes
            .iter()
            .map(|layer| {
                let keys: Vec<serde_json::Value> = (0..LEDS)
                    .map(|i| match layer.get(i).copied().flatten() {
                        Some(c) => serde_json::json!({ "tap": { "code": c } }),
                        None => serde_json::json!({ "tap": null }),
                    })
                    .collect();
                serde_json::json!({ "keys": keys })
            })
            .collect();
        serde_json::json!({ "layout": { "revision": { "layers": layers } } }).to_string()
    }

    fn one_layer(pairs: &[(usize, &str)]) -> Vec<Option<&'static str>> {
        let mut v = vec![None; LEDS];
        for (i, code) in pairs {
            v[*i] = Some(Box::leak(code.to_string().into_boxed_str()) as &'static str);
        }
        v
    }

    #[test]
    fn a_letter_resolves_to_the_led_of_its_key() {
        let json = layout_json(&[one_layer(&[(19, "KC_G"), (17, "KC_D"), (10, "KC_E")])]);
        let l = Layout::from_json(&json).unwrap();
        assert_eq!(l.led_for("g", 0), Some(19));
        assert_eq!(l.led_for("d", 0), Some(17));
        assert_eq!(l.led_for("e", 0), Some(10));
    }

    /// The board is what it is: a binding whose key is not on it lights nothing, and
    /// that is not an error anyone needs telling about.
    #[test]
    fn a_key_the_board_does_not_have_is_simply_absent() {
        let l = Layout::from_json(&layout_json(&[one_layer(&[(19, "KC_G")])])).unwrap();
        assert_eq!(l.led_for("q", 0), None);
        assert_eq!(l.led_for("pagedown", 0), None);
        assert_eq!(l.led_for("¡not a key!", 0), None);
    }

    /// A modifier does not move the key. `shift+h` lights the `h` key, so the caller
    /// strips modifiers and this only ever sees the base spelling.
    #[test]
    fn digits_named_keys_and_function_keys_all_map() {
        let l = Layout::from_json(&layout_json(&[one_layer(&[
            (5, "KC_0"),
            (6, "KC_EQUAL"),
            (7, "KC_LEFT"),
            (8, "KC_F13"),
            (9, "KC_SPACE"),
        ])]))
        .unwrap();
        assert_eq!(l.led_for("0", 0), Some(5));
        assert_eq!(l.led_for("equal", 0), Some(6));
        assert_eq!(l.led_for("plus", 0), Some(6), "plus is the same physical key as equal");
        assert_eq!(l.led_for("left", 0), Some(7));
        assert_eq!(l.led_for("f13", 0), Some(8), "F13-F24 are the whole point of the ZSA layer");
        assert_eq!(l.led_for("space", 0), Some(9));
        assert_eq!(l.led_for("f99", 0), None, "not a function key");
    }

    /// The layer that matters is the active one, and a transparent key on it defers
    /// to the layer below — that is how QMK reads it, and a layer of mostly
    /// transparent keys is the normal case (one here has 55 of 72).
    #[test]
    fn a_transparent_key_falls_through_to_the_layer_below() {
        let base = one_layer(&[(19, "KC_G"), (20, "KC_H")]);
        let top = one_layer(&[(19, "KC_TRANSPARENT"), (20, "KC_Z")]);
        let l = Layout::from_json(&layout_json(&[base, top])).unwrap();

        assert_eq!(l.led_for("g", 1), Some(19), "transparent on top: the base key still types g");
        assert_eq!(l.led_for("z", 1), Some(20), "the top layer overrides where it is opaque");
        assert_eq!(l.led_for("h", 1), None, "h is covered by z on the active layer");
        assert_eq!(l.led_for("h", 0), Some(20), "and is back on the layer that has it");
    }

    /// Asking for a layer the board does not have must not panic — `GetStatus` is a
    /// number from another process and the layout may be older than it.
    #[test]
    fn an_out_of_range_layer_clamps_to_the_top_one() {
        let l = Layout::from_json(&layout_json(&[one_layer(&[(19, "KC_G")])])).unwrap();
        assert_eq!(l.led_for("g", 99), Some(19));
    }

    /// A whole leader level in one pass, which is how it will actually be used.
    #[test]
    fn a_whole_level_resolves_at_once_and_skips_what_is_not_there() {
        let l = Layout::from_json(&layout_json(&[one_layer(&[
            (19, "KC_G"),
            (17, "KC_D"),
            (10, "KC_E"),
        ])]))
        .unwrap();
        let got = l.leds_for(["g", "d", "e", "pagedown"], 0);
        assert_eq!(got, vec![("g", 19), ("d", 17), ("e", 10)]);
    }

    /// The same code on two keys (both spaces, both shifts) resolves to one LED, and
    /// always the same one — otherwise a repaint moves the light around at random.
    #[test]
    fn a_duplicated_code_picks_the_lower_index_every_time() {
        let l = Layout::from_json(&layout_json(&[one_layer(&[(35, "KC_SPACE"), (71, "KC_SPACE")])]))
            .unwrap();
        assert_eq!(l.leds_for(["space"], 0), vec![("space", 35)]);
        assert_eq!(l.led_for("space", 0), Some(35));
    }

    /// Keymapp's schema is not ours. Anything unexpected has to come back as "no
    /// layout" so the caller turns the feature off, never as a panic.
    #[test]
    fn junk_and_missing_pieces_give_no_layout_rather_than_a_panic() {
        assert!(Layout::from_json("").is_none());
        assert!(Layout::from_json("[1,2,3]").is_none());
        assert!(Layout::from_json(r#"{"layout":{}}"#).is_none());
        assert!(Layout::from_json(r#"{"layout":{"revision":{"layers":[]}}}"#).is_none());
        // A layer without keys is not fatal: it resolves to nothing.
        let l = Layout::from_json(r#"{"layout":{"revision":{"layers":[{}]}}}"#).unwrap();
        assert_eq!(l.layers(), 1);
        assert_eq!(l.led_for("g", 0), None);
    }

    /// A keyboard that is not there, scripted: every failure below was seen for real
    /// in one session, and not one of them is reachable from a test that needs the
    /// hardware. `alive` is the socket, `replies` are what kontroll would answer.
    struct Fake {
        alive: bool,
        log: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        replies: std::collections::VecDeque<Result<(), String>>,
    }

    impl Fake {
        fn new(replies: Vec<Result<(), String>>) -> (Fake, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
            let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            (Fake { alive: true, log: log.clone(), replies: replies.into() }, log)
        }
    }

    impl Fake {
        fn say(&mut self, what: String) -> Result<(), String> {
            self.log.lock().unwrap().push(what);
            self.replies.pop_front().unwrap_or(Ok(()))
        }
    }

    impl Runner for Fake {
        fn control(&mut self, on: bool) -> Result<(), String> {
            self.say(format!("control {}", if on { "on" } else { "off" }))
        }
        fn set_all(&mut self, c: crate::config::Rgb) -> Result<(), String> {
            self.say(format!("all #{:02x}{:02x}{:02x}", c.0, c.1, c.2))
        }
        fn set_led(&mut self, led: u8, c: crate::config::Rgb) -> Result<(), String> {
            self.say(format!("led {led} #{:02x}{:02x}{:02x}", c.0, c.1, c.2))
        }
        fn alive(&self) -> bool {
            self.alive
        }
    }

    /// Drives the worker to completion on THIS thread: queue the commands, close the
    /// channel, run. No sleeps, no flakiness, and the log is complete when it returns.
    fn drive(cmds: Vec<Cmd>, fake: Fake) -> Vec<String> {
        let log = fake.log.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        for c in cmds {
            tx.send(c).unwrap();
        }
        drop(tx);
        worker(rx, Box::new(fake));
        let out = log.lock().unwrap().clone();
        out
    }

    fn rgb(r: u8, g: u8, b: u8) -> crate::config::Rgb {
        crate::config::Rgb(r, g, b)
    }

    #[test]
    fn a_paint_takes_the_matrix_then_dims_everything_then_lights_the_level() {
        let (fake, _) = Fake::new(vec![]);
        let log = drive(
            vec![Cmd::Paint {
                leds: vec![(19, rgb(0xf5, 0xd5, 0x43)), (12, rgb(0x6b, 0xb1, 0xff))],
                dim: rgb(5, 5, 16),
                sustain_ms: 10_000,
            }],
            fake,
        );
        assert_eq!(
            log,
            vec![
                // Nothing the host paints is visible until the matrix is taken.
                "control on",
                // The background goes down first, so a key that left this level goes
                // dark in the same pass that lights the ones that arrived.
                "all #050510",
                "led 19 #f5d543",
                "led 12 #6bb1ff",
                // The channel closed: put the board back rather than let the paint
                // outlive the window that asked for it.
                "control off",
            ]
        );
    }

    /// The dead-man switch, now that it is ours. Keymapp used to expire the board on
    /// its own timer; the firmware has no such thing, so an armed leader that nobody
    /// disarms has to come off here or the colours stay for good.
    #[test]
    fn a_paint_nobody_takes_off_comes_off_by_itself() {
        let (fake, log) = Fake::new(vec![]);
        let (tx, rx) = std::sync::mpsc::channel();
        // The sender is kept alive on purpose: closing it would restore for the wrong
        // reason and prove nothing about the deadline.
        tx.send(Cmd::Paint { leds: vec![(1, rgb(9, 9, 9))], dim: rgb(0, 0, 0), sustain_ms: 40 })
            .unwrap();
        let handle = std::thread::spawn(move || worker(rx, Box::new(fake)));
        for _ in 0..100 {
            if log.lock().unwrap().iter().any(|c| c == "control off") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let seen = log.lock().unwrap().clone();
        drop(tx);
        let _ = handle.join();
        assert!(
            seen.contains(&"control off".to_string()),
            "the paint has to expire with nobody asking: {seen:?}"
        );
    }

    /// No keyboard, nothing written. The integration does not exist on a machine
    /// without one, and it must not complain about that.
    #[test]
    fn with_no_keyboard_nothing_is_written_at_all() {
        let (mut fake, _) = Fake::new(vec![]);
        fake.alive = false;
        let log = drive(
            vec![
                Cmd::Paint { leds: vec![(1, rgb(9, 9, 9))], dim: rgb(0, 0, 0), sustain_ms: 100 },
                Cmd::Restore,
            ],
            fake,
        );
        assert!(log.is_empty(), "{log:?}");
    }

    /// Walking three levels of the leader quickly must paint ONCE, at the level the
    /// walk stopped on.
    #[test]
    fn a_burst_of_paints_collapses_to_the_last_one() {
        let (fake, _) = Fake::new(vec![]);
        let log = drive(
            vec![
                Cmd::Paint { leds: vec![(1, rgb(1, 1, 1))], dim: rgb(0, 0, 0), sustain_ms: 60_000 },
                Cmd::Paint { leds: vec![(2, rgb(2, 2, 2))], dim: rgb(0, 0, 0), sustain_ms: 60_000 },
                Cmd::Paint { leds: vec![(3, rgb(3, 3, 3))], dim: rgb(0, 0, 0), sustain_ms: 60_000 },
            ],
            fake,
        );
        assert_eq!(log.iter().filter(|c| c.starts_with("led ")).count(), 1, "{log:?}");
        assert!(log.iter().any(|c| c == "led 3 #030303"), "the LAST level, not the first: {log:?}");
    }

    /// Stopping restores. A terminal that exits leaving the board in its colours is
    /// the bug that gets the whole integration switched off.
    #[test]
    fn stopping_puts_the_board_back() {
        let (fake, _) = Fake::new(vec![]);
        let log = drive(vec![Cmd::Stop], fake);
        assert_eq!(log, vec!["control off"]);
    }

    /// The board going away DURING a paint stops the paint: the alternative is seventy
    /// more writes to a handle that is already gone.
    #[test]
    fn a_board_that_vanishes_mid_paint_does_not_finish_the_level() {
        let (fake, _) = Fake::new(vec![
            Ok(()),                       // control on
            Ok(()),                       // the dim background
            Err("write: No such device".into()), // the first key, and the last
        ]);
        let leds = (0..20).map(|i| (i, rgb(1, 1, 1))).collect();
        let log = drive(vec![Cmd::Paint { leds, dim: rgb(0, 0, 0), sustain_ms: 0 }], fake);
        assert_eq!(log.len(), 4, "stopped at the first refusal, then restored: {log:?}");
    }

    /// The status parser, against the shape kontroll really prints (measured on the
    /// machine: labels, a tab, the value).
    #[test]
    fn the_status_gives_the_revision_that_is_actually_flashed() {
        // Copied from the real thing, blank lines and all — the shape a hand-written
        // sample got wrong.
        let out = "Keymapp version:\t1.3.7\nKontroll version:\t1.0.3\n\nConnected keyboard:\tMoonlander MK1\nFirmware version:\tL4g4A/Jad5YO\nCurrent layer:\t\t3\n\n";
        let st = parse_status(out).unwrap();
        assert_eq!(st.revision, "Jad5YO", "the half AFTER the slash");
        assert_eq!(st.layer, 3);
    }

    /// A board with no layout on it reports a bare slash, and a disconnected one says
    /// so in prose. Neither is a revision, and guessing one would light a layout that
    /// is not on the board.
    #[test]
    fn a_board_without_a_layout_yields_no_revision() {
        assert!(parse_status("Firmware version:\t/\nCurrent layer:\t0\n").is_none());
        assert!(parse_status("No keyboard connected\n").is_none());
        assert!(parse_status("").is_none());
    }

    /// The real thing, against the real database — `cargo test -- --ignored zsa`.
    ///
    /// Ignored by default because it reads Keymapp's file on THIS machine: a test that
    /// needs a particular keyboard flashed with a particular layout tests the desk it
    /// runs on, not the code. It stays because the numbers in it were measured by
    /// lighting the keys one at a time and asking what lit up, and this is what proves
    /// the parser still agrees with the board.
    #[test]
    #[ignore]
    fn the_real_layout_on_this_machine_matches_what_was_measured_by_hand() {
        let db = default_db().expect("a config dir");
        if !db.exists() {
            eprintln!("no keymapp database at {}, skipping", db.display());
            return;
        }
        let l = read_layout(&db, "Jad5YO").expect("revision Jad5YO reads");
        assert_eq!(l.layers(), 5, "Windows, Numeric, Media, Linux, RiderC#");
        // Measured live: these three LEDs lit these three keys.
        assert_eq!(l.led_for("g", 0), Some(19));
        assert_eq!(l.led_for("d", 0), Some(17));
        assert_eq!(l.led_for("e", 0), Some(10));
        // And the anchors of the index map: LED 0 is the outer top-left key (escape),
        // LED 36 the INNER top-right one.
        assert_eq!(l.led_for("escape", 0), Some(0));
        assert_eq!(l.led_for("6", 0), Some(36));
    }

    /// The leader lights need the active layer, and the only way to learn it is a
    /// `kontroll` subprocess with no timeout — on a keystroke, with Keymapp wedged,
    /// that is the window frozen under the user's hand. So the arming key reads what
    /// the board last said and asks for the next reading in the background, and a
    /// second arming while the first is still out asks nothing at all.
    #[test]
    fn arming_the_leader_never_waits_for_the_keyboard_to_answer() {
        let layer = Arc::new(AtomicUsize::new(3));
        let asking = Arc::new(AtomicBool::new(false));
        // Stands in for a wedged Keymapp: it answers when the test lets it, not before.
        let (release, held) = std::sync::mpsc::channel::<()>();
        let (answered, done) = std::sync::mpsc::channel::<()>();
        ask_layer(layer.clone(), asking.clone(), move || {
            held.recv().ok()?;
            let _ = answered.send(());
            Some(Status { revision: "Jad5YO".into(), layer: 1 })
        });

        // The arming key is already through: it paints on the layer it knew.
        assert_eq!(layer.load(Ordering::SeqCst), 3, "the paint uses the last known layer");
        // And every further arming while that one hangs starts nothing new.
        for _ in 0..5 {
            ask_layer(layer.clone(), asking.clone(), || panic!("a second reading was started"));
        }

        release.send(()).unwrap();
        done.recv_timeout(std::time::Duration::from_secs(5)).expect("the reading finishes");
        // The answer lands for the next paint, not for the one that asked.
        while asking.load(Ordering::SeqCst) {
            std::thread::yield_now();
        }
        assert_eq!(layer.load(Ordering::SeqCst), 1, "the next paint uses what the board said");
    }

    /// A revision id reaches a SQL statement, so anything that is not what Keymapp
    /// actually uses is refused before the query is built.
    #[test]
    fn a_revision_id_that_is_not_alphanumeric_is_never_queried() {
        let db = Path::new("/nonexistent/keymapp.sqlite3");
        assert!(read_layout(db, "Jad5YO' or '1'='1").is_none());
        assert!(read_layout(db, "").is_none());
        assert!(read_layout(db, "../../etc/passwd").is_none());
    }
}
