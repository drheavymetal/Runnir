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

/// How the board is actually painted: one `kontroll` process per call.
///
/// A trait so the failure modes below can be tested without a keyboard on the desk.
/// Every one of them was seen for real in one session, and none of them is reachable
/// from a test that needs hardware.
trait Runner: Send {
    /// Runs `kontroll <args>`, returning its output or the reason it failed.
    fn run(&mut self, args: &[&str]) -> Result<String, String>;
    /// Whether Keymapp is up at all. Its socket appearing and vanishing mid-session
    /// is normal: Keymapp exits on its own, and it is the user's app, not ours.
    fn alive(&self) -> bool;
}

struct Kontroll {
    bin: PathBuf,
    socket: PathBuf,
}

impl Runner for Kontroll {
    fn run(&mut self, args: &[&str]) -> Result<String, String> {
        let out = std::process::Command::new(&self.bin)
            .args(args)
            .output()
            .map_err(|e| format!("kontroll: {e}"))?;
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        // kontroll reports its failures on stdout with a zero exit code, so the exit
        // status alone would call every one of them a success.
        if !out.status.success() || text.starts_with("Failed") || text.contains("not found") {
            return Err(if text.is_empty() { err } else { text });
        }
        Ok(text)
    }

    fn alive(&self) -> bool {
        self.socket.exists()
    }
}

/// What the keyboard is asked to do. Only the LAST one queued matters, which is what
/// makes a repaint per keystroke affordable: descending three levels quickly paints
/// once, at the level you stopped on.
enum Cmd {
    /// `dim` for every key, then `leds` on top. One level of the leader layer.
    Paint { leds: Vec<(u8, crate::config::Rgb)>, dim: crate::config::Rgb, sustain_ms: u32 },
    Restore,
    Stop,
}

/// A handle to the keyboard, if there is one. Every method returns immediately: the
/// work happens on a worker thread, because a paint is ~4 ms per key of process spawn
/// and the UI thread cannot spend that.
pub struct Board {
    tx: std::sync::mpsc::Sender<Cmd>,
}

impl Board {
    /// Starts the worker, or `None` when this machine has no keyboard integration —
    /// no `kontroll` on PATH. Absent tools are not errors here, the same rule docker
    /// and the credential helper follow: the feature simply does not exist.
    pub fn start() -> Option<Board> {
        let bin = kontroll_path()?;
        let socket = dirs::config_dir()?.join(".keymapp/keymapp.sock");
        Some(Board::with_runner(Box::new(Kontroll { bin, socket })))
    }

    fn with_runner(runner: Box<dyn Runner>) -> Board {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || worker(rx, runner));
        Board { tx }
    }

    /// Lights `leds` over a `dim` background. `sustain_ms` is a DEAD-MAN SWITCH: after
    /// it elapses Keymapp restores the whole board on its own, so a `kill -9`, a panic
    /// or a crash cannot leave the keyboard wrong. Measured, because it is documented
    /// nowhere: sustain expires the ENTIRE board, not the single LED it was passed with.
    pub fn paint(&self, leds: Vec<(u8, crate::config::Rgb)>, dim: crate::config::Rgb, sustain_ms: u32) {
        let _ = self.tx.send(Cmd::Paint { leds, dim, sustain_ms });
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

fn worker(rx: std::sync::mpsc::Receiver<Cmd>, mut runner: Box<dyn Runner>) {
    let mut connected = false;
    while let Ok(first) = rx.recv() {
        // Coalesce: a burst of paints while a level is being walked collapses to the
        // last one. Repainting 72 keys per keystroke is a race that cannot be won.
        let mut cmd = first;
        while let Ok(next) = rx.try_recv() {
            cmd = next;
        }
        match cmd {
            Cmd::Stop => {
                restore(&mut *runner, &mut connected);
                return;
            }
            Cmd::Restore => restore(&mut *runner, &mut connected),
            Cmd::Paint { leds, dim, sustain_ms } => {
                if !ensure_connected(&mut *runner, &mut connected) {
                    continue;
                }
                let s = sustain_ms.to_string();
                let dim_hex = hex(dim);
                // The background first, so a key that is no longer part of this level
                // goes dark in the same pass that lights the ones that are.
                let _ = attempt(&mut *runner, &mut connected, &["set-rgb-all", "-c", &dim_hex, "-s", &s]);
                for (led, colour) in &leds {
                    let (n, c) = (led.to_string(), hex(*colour));
                    if !attempt(&mut *runner, &mut connected, &["set-rgb", "-l", &n, "-c", &c, "-s", &s]) {
                        // The board went away mid-paint (unplugged, Keymapp exited).
                        // Finishing the loop would be 70 more processes for nothing.
                        break;
                    }
                }
            }
        }
    }
}

fn restore(runner: &mut dyn Runner, connected: &mut bool) {
    if runner.alive() {
        attempt(runner, connected, &["restore-rgb-leds"]);
    }
}

/// Runs one command, recovering from the two failures that are not really failures.
///
/// `no keyboard is connected` after an unplug: Keymapp does NOT reconnect by itself,
/// so connect and try again. `keyboard requires an updated firmware` immediately after
/// connecting: transient, and the very next call succeeds — seen live, with the retry
/// working every time.
fn attempt(runner: &mut dyn Runner, connected: &mut bool, args: &[&str]) -> bool {
    if !runner.alive() {
        *connected = false;
        return false;
    }
    match runner.run(args) {
        Ok(_) => true,
        Err(e) if is_transient(&e) => {
            if e.to_lowercase().contains("no keyboard") {
                *connected = false;
                if !ensure_connected(runner, connected) {
                    return false;
                }
            }
            runner.run(args).is_ok()
        }
        Err(_) => {
            // Anything else: give up on this command and re-check the connection
            // before the next paint rather than hammering a board that said no.
            *connected = false;
            false
        }
    }
}

fn is_transient(err: &str) -> bool {
    let e = err.to_lowercase();
    e.contains("no keyboard") || e.contains("updated firmware")
}

fn ensure_connected(runner: &mut dyn Runner, connected: &mut bool) -> bool {
    if *connected {
        return true;
    }
    if !runner.alive() {
        return false;
    }
    *connected = match runner.run(&["connect-any"]) {
        Ok(_) => true,
        // `Failed to connect: keyboard already connected` is a SUCCESS wearing the
        // word Failed. Reading it as an error meant the first call of every paint
        // failed, the paint was dropped whole, and — the feature being silent by
        // design — nothing anywhere said so. The board simply never lit.
        Err(e) => e.to_lowercase().contains("already connected"),
    };
    *connected
}

fn hex(c: crate::config::Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        replies: std::collections::VecDeque<Result<String, String>>,
    }

    impl Fake {
        fn new(replies: Vec<Result<String, String>>) -> (Fake, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
            let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            (Fake { alive: true, log: log.clone(), replies: replies.into() }, log)
        }
    }

    impl Runner for Fake {
        fn run(&mut self, args: &[&str]) -> Result<String, String> {
            self.log.lock().unwrap().push(args.join(" "));
            self.replies.pop_front().unwrap_or_else(|| Ok(String::new()))
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
    fn a_paint_dims_everything_then_lights_the_keys_of_the_level() {
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
                "connect-any",
                // The background goes down first, so a key that left this level goes
                // dark in the same pass that lights the ones that arrived.
                "set-rgb-all -c #050510 -s 10000",
                "set-rgb -l 19 -c #f5d543 -s 10000",
                "set-rgb -l 12 -c #6bb1ff -s 10000",
            ]
        );
    }

    /// The dead-man switch has to be on EVERY call, not just the first: sustain is
    /// what puts the board back when runnir is killed before it can restore.
    #[test]
    fn every_call_of_a_paint_carries_the_sustain() {
        let (fake, _) = Fake::new(vec![]);
        let log = drive(
            vec![Cmd::Paint { leds: vec![(1, rgb(1, 2, 3))], dim: rgb(0, 0, 0), sustain_ms: 4242 }],
            fake,
        );
        assert!(log.iter().filter(|c| c.starts_with("set-rgb")).all(|c| c.ends_with("-s 4242")), "{log:?}");
    }

    /// Unplug and replug: Keymapp keeps its socket but drops the keyboard, and does
    /// NOT reconnect on its own. Seen live, and it is why `connect-any` is not just a
    /// startup step.
    #[test]
    fn a_disconnected_keyboard_is_reconnected_and_the_call_retried() {
        let (fake, _) = Fake::new(vec![
            Ok("connected".into()),                                  // connect-any
            Err("Failed to set rgb: no keyboard is connected".into()), // set-rgb-all
            Ok("connected".into()),                                  // connect-any again
            Ok("ok".into()),                                         // the retry
        ]);
        let log = drive(
            vec![Cmd::Paint { leds: vec![], dim: rgb(0, 0, 0), sustain_ms: 0 }],
            fake,
        );
        assert_eq!(
            log,
            vec!["connect-any", "set-rgb-all -c #000000 -s 0", "connect-any", "set-rgb-all -c #000000 -s 0"]
        );
    }

    /// The bug that made the whole feature look dead: Keymapp answers
    /// `Failed to connect: keyboard already connected` when the board is ALREADY
    /// connected. Read as an error, it failed the first call of every paint, dropped
    /// the paint whole, and — the feature being silent by design — said nothing. The
    /// keyboard just never lit, for an hour.
    #[test]
    fn already_connected_is_a_success_wearing_the_word_failed() {
        let (fake, _) = Fake::new(vec![
            Err("Failed to connect: keyboard already connected".into()),
            Ok("dimmed".into()),
            Ok("lit".into()),
        ]);
        let log = drive(
            vec![Cmd::Paint { leds: vec![(19, rgb(1, 2, 3))], dim: rgb(0, 0, 0), sustain_ms: 500 }],
            fake,
        );
        assert_eq!(
            log,
            vec!["connect-any", "set-rgb-all -c #000000 -s 500", "set-rgb -l 19 -c #010203 -s 500"],
            "the paint has to go ahead"
        );
    }

    /// The transient one: right after connecting, the first call can answer
    /// "keyboard requires an updated firmware" and the next one succeeds. Believing
    /// the first answer would turn the feature off for the whole session.
    #[test]
    fn the_firmware_error_right_after_connecting_is_retried_once() {
        let (fake, _) = Fake::new(vec![
            Err("Failed to set rgb: keyboard requires an updated firmware".into()),
            Ok("ok".into()),
        ]);
        let log = drive(vec![Cmd::Restore], fake);
        // No connect-any: the board is connected, it just was not ready yet. A restore
        // does not force a connection either — if it turns out there is no keyboard,
        // the "no keyboard" branch handles that, and this is not that.
        assert_eq!(log, vec!["restore-rgb-leds", "restore-rgb-leds"]);
    }

    /// Keymapp exits on its own — it did, mid-session. With no socket, nothing runs at
    /// all: no processes spawned, no errors, no keyboard integration.
    #[test]
    fn with_keymapp_gone_nothing_is_spawned_at_all() {
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
    /// walk stopped on. 72 keys at ~4 ms a process is a race that cannot be won by
    /// painting every step of the way.
    #[test]
    fn a_burst_of_paints_collapses_to_the_last_one() {
        let (fake, _) = Fake::new(vec![]);
        let log = drive(
            vec![
                Cmd::Paint { leds: vec![(1, rgb(1, 1, 1))], dim: rgb(0, 0, 0), sustain_ms: 1 },
                Cmd::Paint { leds: vec![(2, rgb(2, 2, 2))], dim: rgb(0, 0, 0), sustain_ms: 1 },
                Cmd::Paint { leds: vec![(3, rgb(3, 3, 3))], dim: rgb(0, 0, 0), sustain_ms: 1 },
            ],
            fake,
        );
        assert_eq!(log.iter().filter(|c| c.starts_with("set-rgb ")).count(), 1, "{log:?}");
        assert!(log.iter().any(|c| c.starts_with("set-rgb -l 3")), "the LAST level, not the first: {log:?}");
    }

    /// Stopping restores. A terminal that exits leaving the board in its colours is
    /// the bug that gets the whole integration switched off.
    #[test]
    fn stopping_puts_the_board_back() {
        let (fake, _) = Fake::new(vec![]);
        let log = drive(vec![Cmd::Stop], fake);
        assert_eq!(log, vec!["restore-rgb-leds"]);
    }

    /// The board going away DURING a paint stops the paint: the alternative is 70 more
    /// processes spawned at 4 ms each for a keyboard that is not there.
    #[test]
    fn a_board_that_vanishes_mid_paint_does_not_finish_the_level() {
        let (fake, _) = Fake::new(vec![
            Ok("connected".into()),
            Ok("dimmed".into()),
            Err("Failed to set rgb: some other problem".into()),
        ]);
        let leds = (0..20).map(|i| (i, rgb(1, 1, 1))).collect();
        let log = drive(vec![Cmd::Paint { leds, dim: rgb(0, 0, 0), sustain_ms: 0 }], fake);
        assert_eq!(log.len(), 3, "stopped at the first refusal: {log:?}");
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
