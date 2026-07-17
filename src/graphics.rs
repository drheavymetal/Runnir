//! The kitty graphics protocol: inline images.
//!
//! Images arrive as APC sequences — `ESC _ G <key=val,...> ; <base64 payload> ESC \`.
//! vte discards APC, so the byte stream is pre-scanned (see `split`) to pull these
//! out before the rest goes to the VT parser. Only the transmit-and-display subset
//! that `icat` uses is handled; enough to show an image, not the whole spec.

use std::collections::HashMap;

use base64::Engine;

/// A parsed graphics command: its key/value controls and the payload chunk.
#[derive(Debug, Default, Clone)]
pub struct Command {
    pub keys: HashMap<char, String>,
    pub payload: Vec<u8>,
}

impl Command {
    fn get(&self, k: char) -> Option<&str> {
        self.keys.get(&k).map(|s| s.as_str())
    }

    fn num(&self, k: char, default: i64) -> i64 {
        self.get(k).and_then(|v| v.parse().ok()).unwrap_or(default)
    }

    /// Action: `a=t` transmit, `a=T` transmit+display, `a=p` display, `a=d` delete.
    pub fn action(&self) -> char {
        self.get('a').and_then(|s| s.chars().next()).unwrap_or('t')
    }

    /// Whether more chunks follow (`m=1`).
    pub fn more(&self) -> bool {
        self.num('m', 0) == 1
    }

    pub fn id(&self) -> u32 {
        self.num('i', 0) as u32
    }
}

/// Parses the control block `key=val,key=val` and decodes the base64 payload.
fn parse(body: &[u8]) -> Option<Command> {
    // body is everything between `ESC _ G` and `ESC \`, i.e. `<controls>;<payload>`.
    let (controls, payload_b64) = match body.iter().position(|&b| b == b';') {
        Some(i) => (&body[..i], &body[i + 1..]),
        None => (body, &[][..]),
    };
    let controls = std::str::from_utf8(controls).ok()?;

    let mut keys = HashMap::new();
    for pair in controls.split(',') {
        if let Some((k, v)) = pair.split_once('=') {
            if let Some(kc) = k.chars().next() {
                keys.insert(kc, v.to_string());
            }
        }
    }

    let payload = base64::engine::general_purpose::STANDARD
        .decode(payload_b64)
        .unwrap_or_default();
    Some(Command { keys, payload })
}

/// Splits a byte stream into VT bytes (everything else) and graphics commands, in
/// order. The VT bytes go to the parser; the commands to the image store. APC
/// sequences that are incomplete at the end of the chunk are returned as a
/// remainder to prepend to the next chunk, so an image split across reads still
/// parses.
pub fn split(input: &[u8]) -> (Vec<u8>, Vec<Command>, Vec<u8>) {
    let mut vt = Vec::with_capacity(input.len());
    let mut cmds = Vec::new();
    let mut i = 0;
    while i < input.len() {
        // A graphics APC starts with ESC _ G.
        if input[i] == 0x1b
            && input.get(i + 1) == Some(&b'_')
            && input.get(i + 2) == Some(&b'G')
        {
            // Find the terminating ST (ESC \).
            if let Some(end) = find_st(&input[i + 3..]) {
                let body = &input[i + 3..i + 3 + end];
                if let Some(cmd) = parse(body) {
                    cmds.push(cmd);
                }
                i += 3 + end + 2; // skip body + ESC \
                continue;
            } else {
                // Incomplete: hand the tail back to be retried next chunk.
                return (vt, cmds, input[i..].to_vec());
            }
        }
        // The 3-byte introducer itself can straddle a read boundary. If the chunk
        // ends inside it (a trailing `ESC` or `ESC _`), carry those bytes so the
        // next chunk can complete the match. Otherwise they go to vte, which then
        // reassembles the APC and silently discards the image.
        if input[i] == 0x1b {
            let next = input.get(i + 1);
            let partial = next.is_none() || (next == Some(&b'_') && input.get(i + 2).is_none());
            if partial {
                return (vt, cmds, input[i..].to_vec());
            }
        }
        vt.push(input[i]);
        i += 1;
    }
    (vt, cmds, Vec::new())
}

/// Position of the `ESC \` (ST) that ends an APC, if present.
fn find_st(s: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i + 1 < s.len() {
        if s[i] == 0x1b && s[i + 1] == b'\\' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Reassembles chunked transmissions (`m=1`) and decodes a completed image to
/// RGBA. Fed one command at a time; returns a decoded image when the last chunk of
/// a display action arrives.
#[derive(Default)]
pub struct Decoder {
    /// Payload accumulated for the in-progress image, plus its first command's
    /// controls (the continuation chunks carry only `m` and payload).
    pending: Option<Command>,
}

/// A decoded image ready to place: RGBA pixels and its size in pixels, plus the
/// requested cell footprint (0 = derive from pixels).
pub struct Image {
    pub id: u32,
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub cols: u32,
    pub rows: u32,
}

pub enum Event {
    /// An image is ready to place at the cursor.
    Show(Image),
    /// Delete images: `all` clears everything, else by id.
    Delete { all: bool, id: u32 },
    /// A support query (`a=q`); the caller must write `respond()` back to the PTY.
    Query(u32),
    /// Nothing user-visible yet (a mid-transmission chunk).
    None,
}

/// The response to a graphics support query: `OK` for the given image id, so tools
/// like `icat` know inline images are supported and proceed to send them.
pub fn respond(id: u32) -> Vec<u8> {
    format!("\x1b_Gi={id};OK\x1b\\").into_bytes()
}

impl Decoder {
    pub fn feed(&mut self, cmd: Command) -> Event {
        if cmd.action() == 'q' {
            return Event::Query(cmd.id());
        }
        if cmd.action() == 'd' {
            let id = cmd.id();
            return Event::Delete { all: id == 0, id };
        }

        // Accumulate payload across chunks.
        match self.pending.as_mut() {
            Some(acc) => acc.payload.extend_from_slice(&cmd.payload),
            None => self.pending = Some(cmd.clone()),
        }
        if cmd.more() {
            return Event::None; // Wait for the final chunk.
        }

        let full = self.pending.take().unwrap_or(cmd);
        if !matches!(full.action(), 'T' | 'p') {
            return Event::None; // Transmit-only; nothing to show.
        }
        match decode(&full) {
            Some(img) => Event::Show(img),
            None => Event::None,
        }
    }
}

fn decode(cmd: &Command) -> Option<Image> {
    let format = cmd.num('f', 32);
    let (rgba, w, h) = match format {
        100 => {
            // PNG (or any format image can sniff).
            let img = image::load_from_memory(&cmd.payload).ok()?.to_rgba8();
            let (w, h) = img.dimensions();
            (img.into_raw(), w, h)
        }
        32 => {
            let w = cmd.num('s', 0) as u32;
            let h = cmd.num('v', 0) as u32;
            // Compute the byte count in u64: `s`/`v` are attacker-controlled and
            // `w * h * 4` overflows u32 (panicking in debug) well before it exceeds
            // any real payload.
            let need = w as u64 * h as u64 * 4;
            if w == 0 || h == 0 || (cmd.payload.len() as u64) < need {
                return None;
            }
            (cmd.payload.clone(), w, h)
        }
        24 => {
            let w = cmd.num('s', 0) as u32;
            let h = cmd.num('v', 0) as u32;
            let need = w as u64 * h as u64 * 3;
            if w == 0 || h == 0 || (cmd.payload.len() as u64) < need {
                return None;
            }
            // Expand RGB to RGBA.
            let mut rgba = Vec::with_capacity((w as usize) * (h as usize) * 4);
            for px in cmd.payload.chunks_exact(3) {
                rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
            (rgba, w, h)
        }
        _ => return None,
    };
    Some(Image {
        id: cmd.id(),
        rgba,
        width: w,
        height: h,
        cols: cmd.num('c', 0).max(0) as u32,
        rows: cmd.num('r', 0).max(0) as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apc(controls: &str, payload: &str) -> Vec<u8> {
        let mut v = vec![0x1b, b'_', b'G'];
        v.extend_from_slice(controls.as_bytes());
        v.push(b';');
        v.extend_from_slice(payload.as_bytes());
        v.extend_from_slice(&[0x1b, b'\\']);
        v
    }

    #[test]
    fn split_separates_graphics_from_text() {
        let mut input = b"hi ".to_vec();
        input.extend_from_slice(&apc("a=T,f=100", "Zm9v"));
        input.extend_from_slice(b" bye");
        let (vt, cmds, rem) = split(&input);
        assert_eq!(vt, b"hi  bye");
        assert_eq!(cmds.len(), 1);
        assert!(rem.is_empty());
        assert_eq!(cmds[0].action(), 'T');
        assert_eq!(cmds[0].payload, b"foo"); // base64 "Zm9v" -> "foo"
    }

    #[test]
    fn an_incomplete_apc_is_returned_as_remainder() {
        // No ST: the whole APC tail should come back to retry next chunk.
        let mut input = b"x".to_vec();
        input.extend_from_slice(b"\x1b_Ga=T,f=100;Zm9v"); // no ESC \
        let (vt, cmds, rem) = split(&input);
        assert_eq!(vt, b"x");
        assert!(cmds.is_empty());
        assert_eq!(&rem[..3], b"\x1b_G");
    }

    #[test]
    fn an_introducer_split_across_reads_is_carried_not_lost() {
        // The read boundary falls inside `ESC _ G`. Both the trailing `ESC` and
        // `ESC _` cases must come back as remainder; feeding them to vte would let
        // it swallow the reassembled APC and drop the image.
        for cut in [1usize, 2] {
            let mut full = b"hi".to_vec();
            full.extend_from_slice(&apc("a=T,f=100", "Zm9v"));
            let esc = full.iter().position(|&b| b == 0x1b).unwrap();
            let (head, tail) = full.split_at(esc + cut);

            let (vt1, cmds1, rem1) = split(head);
            assert_eq!(vt1, b"hi");
            assert!(cmds1.is_empty());
            assert_eq!(rem1, &head[esc..], "the partial introducer is carried");

            // Next chunk: prepend the carry, as the reader thread does.
            let mut next = rem1;
            next.extend_from_slice(tail);
            let (_vt2, cmds2, rem2) = split(&next);
            assert_eq!(cmds2.len(), 1, "the image parses once reassembled");
            assert!(rem2.is_empty());
        }
    }

    #[test]
    fn chunked_transmission_reassembles() {
        let mut dec = Decoder::default();
        // Two chunks of raw RGBA (m=1 then m=0), 1x1 red pixel needs 4 bytes.
        let c1 = parse(b"a=T,f=32,s=1,v=1,m=1;\xff\x00").map(|mut c| {
            c.payload = vec![0xff, 0x00];
            c
        });
        // Build via feed with explicit payloads.
        let mut first = Command::default();
        first.keys.insert('a', "T".into());
        first.keys.insert('f', "32".into());
        first.keys.insert('s', "1".into());
        first.keys.insert('v', "1".into());
        first.keys.insert('m', "1".into());
        first.payload = vec![255, 0];
        let mut second = Command::default();
        second.keys.insert('m', "0".into());
        second.payload = vec![0, 255];
        let _ = c1;

        assert!(matches!(dec.feed(first), Event::None), "mid-chunk shows nothing");
        match dec.feed(second) {
            Event::Show(img) => {
                assert_eq!((img.width, img.height), (1, 1));
                assert_eq!(img.rgba, vec![255, 0, 0, 255]);
            }
            _ => panic!("final chunk must yield an image"),
        }
    }

    #[test]
    fn a_support_query_is_answered_ok() {
        let mut dec = Decoder::default();
        let mut cmd = Command::default();
        cmd.keys.insert('a', "q".into());
        cmd.keys.insert('i', "7".into());
        match dec.feed(cmd) {
            Event::Query(7) => {}
            _ => panic!("a=q must produce a Query event"),
        }
        // The response tools expect.
        assert_eq!(respond(7), b"\x1b_Gi=7;OK\x1b\\");
    }

    #[test]
    fn oversized_raw_dimensions_are_rejected_without_overflow() {
        // Regression: `w * h * 4` was computed in u32 and overflowed (panicking in
        // debug) for attacker-supplied s/v before the payload-length check ran.
        let mut cmd = Command::default();
        cmd.keys.insert('a', "T".into());
        cmd.keys.insert('f', "32".into());
        cmd.keys.insert('s', "70000".into()); // 70000*70000*4 overflows u32
        cmd.keys.insert('v', "70000".into());
        cmd.payload = vec![0u8; 16];
        assert!(
            matches!(Decoder::default().feed(cmd), Event::None),
            "an impossible size must be rejected, not overflow"
        );
    }

    #[test]
    fn delete_all_is_recognised() {
        let mut dec = Decoder::default();
        let mut cmd = Command::default();
        cmd.keys.insert('a', "d".into());
        assert!(matches!(dec.feed(cmd), Event::Delete { all: true, .. }));
    }

    #[test]
    fn png_decodes_to_rgba() {
        // A 1x1 red PNG.
        use image::{ImageEncoder, codecs::png::PngEncoder};
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&[255, 0, 0, 255], 1, 1, image::ExtendedColorType::Rgba8)
            .unwrap();
        let mut cmd = Command::default();
        cmd.keys.insert('a', "T".into());
        cmd.keys.insert('f', "100".into());
        cmd.payload = png;
        match Decoder::default().feed(cmd) {
            Event::Show(img) => {
                assert_eq!((img.width, img.height), (1, 1));
                assert_eq!(&img.rgba[..3], &[255, 0, 0]);
            }
            _ => panic!("PNG must decode"),
        }
    }
}
