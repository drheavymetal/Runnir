//! Optical transfer: a file leaves runnir as animated QR codes and arrives on a
//! phone through its camera. No network, no pairing, no account.
//!
//! This is a Rust port of the wire format of `decimen-optical-transfer`
//! (<https://github.com/bashalarmistalt/decimen-optical-transfer>, MIT), so that
//! runnir can be a sender for the receiver that ships on runnir's own website.
//! The MIT terms travel with this file; runnir as a whole stays GPL-3.0-only,
//! which MIT permits.
//!
//! # This module IS the wire format
//!
//! Sender and receiver derive every frame's block subset independently and never
//! compare notes. A one-ulp disagreement in [`dlog`] or [`soliton_cdf`] does not
//! fail loudly — it shifts a CDF boundary, flips a sampled degree, and corrupts
//! the transfer silently, surfacing only as a checksum failure after the whole
//! thing has run. That is why `dlog` exists at all instead of `f64::ln`, and why
//! the tests at the bottom are golden vectors recorded against the JavaScript
//! implementation rather than behavioural tests. If one fails, the format
//! changed, and every saved standalone receiver in the world disagrees with us.
//!
//! # Layout
//!
//! Each QR frame carries a self-describing 20-byte header (little-endian) plus
//! `block_len` payload bytes:
//!
//! ```text
//!  0  u8   0xD1
//!  1  u8   0x0C
//!  2  u16  session id   random per sender start
//!  4  u32  seq          drives the fountain PRNG
//!  8  u16  k            source block count
//! 10  u16  block_len    payload bytes per frame
//! 12  u32  total_len    container length in bytes
//! 16  u32  payload_fnv  FNV-1a of the whole container
//! ```
//!
//! The payload those frames reconstruct is a `DCF2` container: the file's bytes
//! (optionally gzipped), its name, its media type, and the SHA-256 of the
//! ORIGINAL bytes — see [`pack_file`].

use std::collections::{HashMap, HashSet};

use sha2::{Digest, Sha256};

// Blocks are XORed as u32 words and handed to the QR encoder as bytes, so the
// two views have to agree. Every target runnir builds for is little-endian; a
// big-endian one would need its own golden vectors, which is a problem worth
// having rather than a silent wrong answer.
#[cfg(target_endian = "big")]
compile_error!("the optical wire format is little-endian; big-endian needs its own vectors");

pub const HEADER_LEN: usize = 20;
pub const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;
/// `k` is a u16 on the wire, so a big payload at a small frame size runs out of
/// block numbers long before it runs out of file size.
pub const MAX_SOURCE_BLOCKS: usize = 0xffff;

const MAGIC0: u8 = 0xd1;
const MAGIC1: u8 = 0x0c;
const FILE_MAGIC: [u8; 4] = *b"DCF2";
const FILE_HEADER_LEN: usize = 49;

// ---------------------------------------------------------------- primitives

/// splitmix32 — the frame PRNG. Integer ops only, so it is exact everywhere.
pub struct SplitMix32 {
    state: u32,
}

impl SplitMix32 {
    pub fn new(seed: u32) -> Self {
        Self { state: seed }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u32 {
        self.state = self.state.wrapping_add(0x9e37_79b9);
        let mut t = self.state ^ (self.state >> 16);
        t = t.wrapping_mul(0x21f0_aaad);
        t ^= t >> 15;
        t = t.wrapping_mul(0x735a_2d97);
        t ^= t >> 15;
        t
    }
}

pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for &b in bytes {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

const LN2: f64 = 0.693_147_180_559_945_3;
const SOLITON_C: f64 = 0.1;
const SOLITON_DELTA: f64 = 0.5;

/// Deterministic natural log: exact-ops range reduction plus an atanh series.
///
/// Not a convenience wrapper — this is wire format. `f64::ln` is
/// implementation-approximated (as is JavaScript's `Math.log`), and the two ends
/// of an optical transfer never get to compare notes, so a one-ulp difference on
/// either side of a CDF boundary desynchronises the streams. Only exactly
/// specified IEEE-754 operations appear below.
pub fn dlog(x: f64) -> f64 {
    let mut e: f64 = 0.0;
    let mut m = x;
    while m >= 1.5 {
        m /= 2.0;
        e += 1.0;
    }
    while m < 0.75 {
        m *= 2.0;
        e -= 1.0;
    }
    let z = (m - 1.0) / (m + 1.0);
    let z2 = z * z;
    let mut term = z;
    let mut sum = 0.0;
    let mut n = 1u32;
    while n <= 21 {
        sum += term / f64::from(n);
        term *= z2;
        n += 2;
    }
    e * LN2 + 2.0 * sum
}

/// Robust-soliton degree CDF for `k` source blocks.
///
/// The arithmetic order is load-bearing: floating-point addition is not
/// associative, so regrouping any of these expressions changes the last bit of a
/// CDF entry and, eventually, a frame that two ends disagree about.
pub fn soliton_cdf(k: usize) -> Vec<f64> {
    let mut cdf = vec![0.0f64; k];
    if k == 1 {
        cdf[0] = 1.0;
        return cdf;
    }
    let kf = k as f64;
    let r = (SOLITON_C * dlog(kf / SOLITON_DELTA) * kf.sqrt()).max(1.0);
    let spike = (kf / r).ceil().min(kf);
    let mut total = 0.0f64;
    for d in 1..=k {
        let df = d as f64;
        let rho = if d == 1 { 1.0 / kf } else { 1.0 / (df * (df - 1.0)) };
        let tau = if df < spike {
            r / (df * kf)
        } else if df == spike {
            (r * dlog(r / SOLITON_DELTA).max(0.0)) / kf
        } else {
            0.0
        };
        total += rho + tau;
        cdf[d - 1] = total;
    }
    for value in cdf.iter_mut() {
        *value /= total;
    }
    cdf[k - 1] = 1.0;
    cdf
}

fn frame_seed(session_id: u16, seq: u32) -> u32 {
    let mut h = u32::from(session_id)
        .wrapping_add(1)
        .wrapping_mul(0x9e37_79b1)
        ^ seq.wrapping_add(0x85eb_ca6b);
    h = (h ^ (h >> 13)).wrapping_mul(0xc2b2_ae35);
    h ^ (h >> 16)
}

/// The block indices XORed into frame `seq`.
///
/// Both ends derive this and never compare, so any change here is a breaking
/// wire-format change. Insertion order is preserved because the golden vectors
/// pin it; XOR itself does not care.
pub fn frame_indices(k: usize, cdf: &[f64], session_id: u16, seq: u32) -> Vec<u32> {
    let mut rnd = SplitMix32::new(frame_seed(session_id, seq));
    // Inverse-CDF sample of the degree. The divisor is a power of two, so the
    // division is exact and matches JavaScript's `rnd() * 2 ** -32`.
    let u = f64::from(rnd.next()) / 4_294_967_296.0;
    let mut lo = 0usize;
    let mut hi = k - 1;
    while lo < hi {
        let mid = (lo + hi) >> 1;
        if cdf[mid] >= u {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    let d = std::cmp::min(k, lo + 1);

    if d > (k >> 3) {
        // Large degree: a partial Fisher-Yates over an identity array beats
        // rejection sampling, which would spend most of its draws on collisions.
        let mut scratch: Vec<u32> = (0..k as u32).collect();
        let mut out = Vec::with_capacity(d);
        for i in 0..d {
            let j = i + (rnd.next() as usize % (k - i));
            scratch.swap(i, j);
            out.push(scratch[i]);
        }
        return out;
    }

    let mut out: Vec<u32> = Vec::with_capacity(d);
    let mut seen: HashSet<u32> = HashSet::with_capacity(d * 2);
    while out.len() < d {
        // A draw is consumed even when it collides — the JavaScript `Set.add`
        // does the same, and skipping it here would shift the whole stream.
        let candidate = rnd.next() % k as u32;
        if seen.insert(candidate) {
            out.push(candidate);
        }
    }
    out
}

// ------------------------------------------------------------------ fountain

/// LT (Luby transform) encoder: an endless stream of frames, any `k * 1.15` of
/// which rebuild the payload, in any order.
pub struct LtEncoder {
    pub k: usize,
    pub block_len: usize,
    pub session_id: u16,
    words: usize,
    blocks: Vec<u32>,
    cdf: Vec<f64>,
}

impl LtEncoder {
    pub fn new(payload: &[u8], block_len: usize, session_id: u16) -> Self {
        assert!(block_len > 0, "block_len must be positive");
        let k = std::cmp::max(1, payload.len().div_ceil(block_len));
        let words = block_len.div_ceil(4);
        let mut blocks = vec![0u32; k * words];
        {
            let bytes: &mut [u8] = bytemuck::cast_slice_mut(&mut blocks);
            for b in 0..k {
                let start = b * block_len;
                let end = std::cmp::min(start + block_len, payload.len());
                if start >= end {
                    continue;
                }
                let dst = b * words * 4;
                bytes[dst..dst + (end - start)].copy_from_slice(&payload[start..end]);
            }
        }
        let cdf = soliton_cdf(k);
        Self { k, block_len, session_id, words, blocks, cdf }
    }

    /// Always exactly `block_len` bytes: the sender pins the QR version off the
    /// first frame, so a short tail frame would make every later code undecodable.
    pub fn encode(&self, seq: u32) -> Vec<u8> {
        let idx = frame_indices(self.k, &self.cdf, self.session_id, seq);
        let mut out = vec![0u32; self.words];
        for b in idx {
            let off = b as usize * self.words;
            for w in 0..self.words {
                out[w] ^= self.blocks[off + w];
            }
        }
        let bytes: &[u8] = bytemuck::cast_slice(&out);
        bytes[..self.block_len].to_vec()
    }

    /// A ready-to-encode QR frame: header plus block.
    pub fn frame(&self, seq: u32, total_len: usize, payload_fnv: u32) -> Vec<u8> {
        let header = FrameHeader {
            session_id: self.session_id,
            seq,
            k: self.k as u16,
            block_len: self.block_len as u16,
            total_len: total_len as u32,
            payload_fnv,
        };
        pack_frame(&header, &self.encode(seq))
    }
}

struct PendingFrame {
    idx: HashSet<u32>,
    words: Vec<u32>,
}

/// LT decoder. runnir only ever sends, so this exists for the round-trip tests —
/// which is the point: the encoder is the half that has to be right, and a
/// decoder that was ported from the same source is the cheapest way to say so
/// without a browser in the loop.
pub struct LtDecoder {
    pub k: usize,
    pub block_len: usize,
    pub total_len: usize,
    pub frames_new: usize,
    pub frames_dup: usize,
    words: usize,
    cdf: Vec<f64>,
    session_id: u16,
    solved: Vec<Option<Vec<u32>>>,
    solved_count: usize,
    pending: Vec<Option<PendingFrame>>,
    by_block: HashMap<u32, HashSet<usize>>,
    seen: HashSet<u32>,
}

impl LtDecoder {
    pub fn new(k: usize, block_len: usize, session_id: u16, total_len: usize) -> Self {
        Self {
            k,
            block_len,
            total_len,
            frames_new: 0,
            frames_dup: 0,
            words: block_len.div_ceil(4),
            cdf: soliton_cdf(k),
            session_id,
            solved: vec![None; k],
            solved_count: 0,
            pending: Vec::new(),
            by_block: HashMap::new(),
            seen: HashSet::new(),
        }
    }

    pub fn is_complete(&self) -> bool {
        self.solved_count >= self.k
    }

    pub fn add_frame(&mut self, seq: u32, block: &[u8]) {
        if !self.seen.insert(seq) {
            self.frames_dup += 1;
            return;
        }
        self.frames_new += 1;
        if self.is_complete() {
            return;
        }

        let mut idx: HashSet<u32> =
            frame_indices(self.k, &self.cdf, self.session_id, seq).into_iter().collect();
        let mut words = vec![0u32; self.words];
        {
            let bytes: &mut [u8] = bytemuck::cast_slice_mut(&mut words);
            bytes[..self.block_len].copy_from_slice(&block[..self.block_len]);
        }
        for b in idx.iter().copied().collect::<Vec<_>>() {
            if let Some(solved) = &self.solved[b as usize] {
                xor_into(&mut words, solved);
                idx.remove(&b);
            }
        }
        match idx.len() {
            0 => {} // fully redundant
            1 => {
                let b = *idx.iter().next().unwrap();
                self.resolve(b, words);
            }
            _ => {
                let slot = self.pending.len();
                for &b in &idx {
                    self.by_block.entry(b).or_default().insert(slot);
                }
                self.pending.push(Some(PendingFrame { idx, words }));
            }
        }
    }

    /// Peeling cascade: solve a block, reduce every frame waiting on it, repeat.
    fn resolve(&mut self, b0: u32, w0: Vec<u32>) {
        let mut queue: Vec<(u32, Vec<u32>)> = vec![(b0, w0)];
        while let Some((b, w)) = queue.pop() {
            if self.solved[b as usize].is_some() {
                continue;
            }
            self.solved[b as usize] = Some(w.clone());
            self.solved_count += 1;
            let Some(waiting) = self.by_block.remove(&b) else { continue };
            for slot in waiting {
                // A frame reduced to a single unknown block IS that block, so it
                // leaves the pending set entirely and joins the queue.
                let remaining = {
                    let Some(pf) = self.pending[slot].as_mut() else { continue };
                    xor_into(&mut pf.words, &w);
                    pf.idx.remove(&b);
                    if pf.idx.len() != 1 {
                        continue;
                    }
                    *pf.idx.iter().next().unwrap()
                };
                if let Some(set) = self.by_block.get_mut(&remaining) {
                    set.remove(&slot);
                }
                let pf = self.pending[slot].take().unwrap();
                if self.solved[remaining as usize].is_none() {
                    queue.push((remaining, pf.words));
                }
            }
        }
    }

    pub fn assemble(&self) -> Option<Vec<u8>> {
        if !self.is_complete() {
            return None;
        }
        let mut out = vec![0u8; self.total_len];
        for b in 0..self.k {
            let start = b * self.block_len;
            if start >= self.total_len {
                break;
            }
            let len = std::cmp::min(self.block_len, self.total_len - start);
            let words = self.solved[b].as_ref().unwrap();
            let bytes: &[u8] = bytemuck::cast_slice(words);
            out[start..start + len].copy_from_slice(&bytes[..len]);
        }
        Some(out)
    }
}

fn xor_into(dst: &mut [u32], src: &[u32]) {
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d ^= *s;
    }
}

// -------------------------------------------------------------- frame header

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    pub session_id: u16,
    pub seq: u32,
    pub k: u16,
    pub block_len: u16,
    pub total_len: u32,
    pub payload_fnv: u32,
}

pub fn pack_frame(h: &FrameHeader, block: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + block.len());
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.extend_from_slice(&h.session_id.to_le_bytes());
    out.extend_from_slice(&h.seq.to_le_bytes());
    out.extend_from_slice(&h.k.to_le_bytes());
    out.extend_from_slice(&h.block_len.to_le_bytes());
    out.extend_from_slice(&h.total_len.to_le_bytes());
    out.extend_from_slice(&h.payload_fnv.to_le_bytes());
    out.extend_from_slice(block);
    out
}

pub fn parse_frame(bytes: &[u8]) -> Option<(FrameHeader, &[u8])> {
    if bytes.len() <= HEADER_LEN || bytes[0] != MAGIC0 || bytes[1] != MAGIC1 {
        return None;
    }
    let u16at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let u32at = |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    let header = FrameHeader {
        session_id: u16at(2),
        seq: u32at(4),
        k: u16at(8),
        block_len: u16at(10),
        total_len: u32at(12),
        payload_fnv: u32at(16),
    };
    if header.k == 0 || header.block_len == 0 || header.total_len == 0 {
        return None;
    }
    if bytes.len() != HEADER_LEN + header.block_len as usize {
        return None;
    }
    Some((header, &bytes[HEADER_LEN..]))
}

/// Payload bytes per frame, once the header has taken its cut.
pub fn block_length(frame_bytes: usize) -> usize {
    frame_bytes - HEADER_LEN
}

/// The smallest frame size that can carry this payload at all, given that `k` is
/// a u16. At 500 bytes per frame the real ceiling is about 30 MB, not 64.
pub fn minimum_frame_bytes(payload_bytes: usize) -> usize {
    payload_bytes.div_ceil(MAX_SOURCE_BLOCKS) + HEADER_LEN
}

pub fn fits_in_one_stream(payload_bytes: usize, frame_bytes: usize) -> bool {
    payload_bytes.div_ceil(block_length(frame_bytes)) <= MAX_SOURCE_BLOCKS
}

// ----------------------------------------------------------------- container

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    Gzip,
}

pub struct PackedFile {
    pub container: Vec<u8>,
    pub compression: Compression,
    /// SHA-256 of the ORIGINAL bytes, which is what the receiver verifies and
    /// what [`verification_code`] is derived from.
    pub sha256: [u8; 32],
    pub original_size: usize,
    pub transmitted_size: usize,
    pub payload_fnv: u32,
}

impl PackedFile {
    /// The short code a human compares between the two screens. See
    /// [`verification_code`].
    pub fn verification_code(&self) -> String {
        verification_code(&self.sha256)
    }
}

/// A six-digit code derived from the file's SHA-256, for a person to compare
/// between the sending screen and the receiving phone.
///
/// The sender knows it before the first frame is drawn, because the hash is
/// computed while packing; the receiver knows it only after the fountain has
/// rebuilt the file. Matching codes mean the phone holds the file that was on
/// screen — which also rules out having locked onto a different sender nearby.
///
/// This adds no security and nothing to the wire: it is a projection of the
/// SHA-256 that already travels inside the container, and the receiver still
/// verifies that hash in full, automatically. Six digits are for the eye, not
/// for an adversary. Deriving it rather than sending it is what keeps runnir
/// compatible with receivers that have never heard of it.
pub fn verification_code(sha256: &[u8; 32]) -> String {
    let head = u32::from_be_bytes([sha256[0], sha256[1], sha256[2], sha256[3]]);
    format!("{:06}", head % 1_000_000)
}

/// Media types whose bytes are already entropy-coded — gzip on these costs a
/// full-size allocation and a pass over every byte to learn it cannot help.
fn is_precompressed_type(media_type: &str) -> bool {
    let media = media_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    if media.starts_with("video/") {
        return true;
    }
    if let Some(sub) = media.strip_prefix("image/") {
        return !matches!(
            sub,
            "bmp" | "x-ms-bmp" | "svg+xml" | "tiff" | "x-icon" | "vnd.microsoft.icon"
        );
    }
    if let Some(sub) = media.strip_prefix("audio/") {
        return !matches!(sub, "wav" | "x-wav" | "wave" | "vnd.wave" | "aiff" | "x-aiff" | "basic" | "l16");
    }
    // The OOXML and OpenDocument families are zip containers.
    if media.starts_with("application/vnd.openxmlformats-officedocument.")
        || media.starts_with("application/vnd.oasis.opendocument.")
        || media.ends_with("+zip")
    {
        return true;
    }
    matches!(
        media.as_str(),
        "application/gzip"
            | "application/java-archive"
            | "application/vnd.rar"
            | "application/x-7z-compressed"
            | "application/x-brotli"
            | "application/x-bzip"
            | "application/x-bzip2"
            | "application/x-gzip"
            | "application/x-lzma"
            | "application/x-rar-compressed"
            | "application/x-xz"
            | "application/x-zip-compressed"
            | "application/zip"
            | "application/zstd"
    )
}

/// Reduce a name to a bare basename, with control characters stripped.
///
/// The receiver does this too, and there it is the part that matters — the name
/// it unpacks arrived over the optical channel. Doing it here is courtesy.
fn safe_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control() && *c != '\u{7f}')
        .collect::<String>()
        .trim()
        .to_string();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "transfer.bin".to_string()
    } else {
        cleaned
    }
}

/// Wrap a file in the `DCF2` container the receiver expects.
///
/// gzip is attempted only when it can plausibly win: below 768 bytes the header
/// eats the gain, and a JPEG or a zip never wins at all. A wrong "skip" costs a
/// few percent of transfer time; a wrong "try" costs a whole extra copy of the
/// file in memory.
pub fn pack_file(name: &str, media_type: &str, bytes: &[u8]) -> anyhow::Result<PackedFile> {
    if bytes.is_empty() {
        anyhow::bail!("nothing to send: the file is empty");
    }
    if bytes.len() > MAX_FILE_BYTES {
        anyhow::bail!(
            "the optical channel carries at most {} MB; this is {:.1} MB",
            MAX_FILE_BYTES / 1024 / 1024,
            bytes.len() as f64 / 1024.0 / 1024.0
        );
    }

    let name_bytes = safe_file_name(name).into_bytes();
    let type_str = if media_type.is_empty() { "application/octet-stream" } else { media_type };
    let type_bytes = type_str.as_bytes();
    if name_bytes.len() > 0xffff || type_bytes.len() > 0xffff {
        anyhow::bail!("the file name or media type is too long");
    }

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let sha256: [u8; 32] = hasher.finalize().into();

    let try_gzip = bytes.len() >= 768 && !is_precompressed_type(type_str);
    let compressed = if try_gzip { Some(gzip(bytes)?) } else { None };
    // The 64-byte margin keeps a transfer from paying for a decompression step
    // that saved it a rounding error.
    let use_gzip = compressed.as_ref().is_some_and(|c| c.len() + 64 < bytes.len());
    let transmitted: &[u8] = if use_gzip { compressed.as_ref().unwrap() } else { bytes };
    let compression = if use_gzip { Compression::Gzip } else { Compression::None };

    let mut out = Vec::with_capacity(
        FILE_HEADER_LEN + name_bytes.len() + type_bytes.len() + transmitted.len(),
    );
    out.extend_from_slice(&FILE_MAGIC);
    out.push(u8::from(use_gzip));
    out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(&(type_bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&(transmitted.len() as u32).to_le_bytes());
    out.extend_from_slice(&sha256);
    out.extend_from_slice(&name_bytes);
    out.extend_from_slice(type_bytes);
    out.extend_from_slice(transmitted);

    let payload_fnv = fnv1a(&out);
    Ok(PackedFile {
        container: out,
        compression,
        sha256,
        original_size: bytes.len(),
        transmitted_size: transmitted.len(),
        payload_fnv,
    })
}

fn gzip(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    use std::io::Write;
    let mut encoder =
        flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes)?;
    Ok(encoder.finish()?)
}

/// Unpack a `DCF2` container. runnir does not receive, so this is here to close
/// the round trip in tests — and to keep the two halves of the format written
/// down in one place.
pub struct UnpackedFile {
    pub name: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
    pub sha256: [u8; 32],
    pub compression: Compression,
}

impl UnpackedFile {
    /// Recompute the code from the bytes that actually arrived. Equal to the
    /// sender's only if the file is the one that was on screen.
    pub fn verification_code(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(&self.bytes);
        let actual: [u8; 32] = hasher.finalize().into();
        verification_code(&actual)
    }

    pub fn verify(&self) -> bool {
        let mut hasher = Sha256::new();
        hasher.update(&self.bytes);
        let actual: [u8; 32] = hasher.finalize().into();
        actual == self.sha256
    }
}

pub fn unpack_file(container: &[u8]) -> anyhow::Result<UnpackedFile> {
    if container.len() < FILE_HEADER_LEN || container[..4] != FILE_MAGIC {
        anyhow::bail!("the recovered file header is invalid");
    }
    let compression = match container[4] {
        0 => Compression::None,
        1 => Compression::Gzip,
        _ => anyhow::bail!("the recovered file uses unsupported compression"),
    };
    let name_len = u16::from_le_bytes([container[5], container[6]]) as usize;
    let type_len = u16::from_le_bytes([container[7], container[8]]) as usize;
    let file_len =
        u32::from_le_bytes([container[9], container[10], container[11], container[12]]) as usize;
    let transmitted_len =
        u32::from_le_bytes([container[13], container[14], container[15], container[16]]) as usize;
    let data_offset = FILE_HEADER_LEN + name_len + type_len;
    if file_len == 0
        || file_len > MAX_FILE_BYTES
        || transmitted_len == 0
        || transmitted_len > MAX_FILE_BYTES
        || data_offset + transmitted_len != container.len()
    {
        anyhow::bail!("the recovered file length does not match its header");
    }
    let mut sha256 = [0u8; 32];
    sha256.copy_from_slice(&container[17..49]);

    let transmitted = &container[data_offset..];
    let bytes = match compression {
        Compression::None => transmitted.to_vec(),
        // The declared length is a hint, never a bound: it arrived over the same
        // optical channel as everything else.
        Compression::Gzip => gunzip(transmitted, file_len)?,
    };
    if bytes.len() != file_len {
        anyhow::bail!("the decompressed file length does not match its header");
    }

    Ok(UnpackedFile {
        name: safe_file_name(&String::from_utf8_lossy(
            &container[FILE_HEADER_LEN..FILE_HEADER_LEN + name_len],
        )),
        media_type: {
            let t = String::from_utf8_lossy(&container[FILE_HEADER_LEN + name_len..data_offset])
                .to_string();
            if t.is_empty() { "application/octet-stream".to_string() } else { t }
        },
        bytes,
        sha256,
        compression,
    })
}

fn gunzip(bytes: &[u8], max_bytes: usize) -> anyhow::Result<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    let mut decoder = flate2::read::GzDecoder::new(bytes).take(max_bytes as u64 + 1);
    decoder.read_to_end(&mut out)?;
    if out.len() > max_bytes {
        anyhow::bail!("the recovered file expands past its declared length");
    }
    Ok(out)
}

// --------------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic filler. The fingerprints below are recorded against it, and
    /// against the JavaScript implementation producing the same bytes.
    fn test_payload(len: usize) -> Vec<u8> {
        (0..len).map(|i| ((i * 37 + (i >> 8) * 11) & 0xff) as u8).collect()
    }

    fn hex32(v: u32) -> String {
        format!("0x{v:08x}")
    }

    // ------------------------------------------------------------------ dlog

    #[test]
    fn dlog_is_bit_exact_against_the_javascript_vectors() {
        let golden: &[(f64, f64)] = &[
            (1.0, 0.0),
            (1.5, 0.4054651081081644),
            (2.0, 0.6931471805599453),
            (2.718281828459045, 1.0),
            (10.0, 2.3025850929940455),
            (20.0, 2.995732273553991),
            (200.0, 5.298317366548036),
            (2000.0, 7.600902459542082),
            (2986.0, 8.001689978099137),
            (44000.0, 10.691944912900398),
            (131070.0, 11.78348681061359),
        ];
        for &(x, expected) in golden {
            assert_eq!(dlog(x), expected, "dlog({x}) drifted");
        }
    }

    #[test]
    fn dlog_is_bit_exact_across_every_input_the_degree_distribution_reaches() {
        // Eleven spot values are readable but sparse: shortening the series from
        // 21 terms to 19 changes only 0.2% of outputs. soliton_cdf only ever
        // calls dlog(k/DELTA) and dlog(R/DELTA), so sweep both domains whole.
        let mut values: Vec<f64> = Vec::with_capacity(65535 + 64 * 4096);
        for k in 1..=65535u32 {
            values.push(dlog(f64::from(2 * k)));
        }
        for i in 64..(64 * 4096) {
            values.push(dlog(f64::from(i) / 64.0));
        }
        let bytes: &[u8] = bytemuck::cast_slice(&values);
        assert_eq!(hex32(fnv1a(bytes)), "0x27b0f3cc", "dlog changed");
    }

    #[test]
    fn dlog_is_close_to_but_not_the_same_as_the_library_log() {
        // The whole reason dlog exists. This fails if someone "simplifies" it
        // into f64::ln, which is exactly the change that desyncs two ends.
        let mut differing = 0;
        let mut worst_ulp = 0.0f64;
        for k in 2..=20000u32 {
            for x in [f64::from(k), f64::from(k) / 0.5] {
                let ours = dlog(x);
                let native = x.ln();
                if ours != native {
                    differing += 1;
                }
                let ulp = (ours - native).abs() / (native.abs() * f64::EPSILON);
                worst_ulp = worst_ulp.max(ulp);
            }
        }
        assert!(worst_ulp <= 2.0, "dlog drifted {worst_ulp:.2} ulp from ln");
        assert!(differing > 0, "dlog now matches ln bit for bit — did it become ln?");
    }

    // -------------------------------------------------------- degree sampling

    #[test]
    fn the_soliton_cdf_is_a_well_formed_distribution() {
        for k in [1usize, 2, 17, 179, 716, 22000] {
            let cdf = soliton_cdf(k);
            assert_eq!(cdf.len(), k);
            assert_eq!(cdf[k - 1], 1.0, "k={k} CDF must terminate at exactly 1");
            for i in 1..k {
                assert!(cdf[i] >= cdf[i - 1], "k={k} CDF is not monotonic at {i}");
            }
            assert!(cdf[0] > 0.0, "k={k} degree 1 needs mass or peeling never starts");
        }
    }

    #[test]
    fn the_soliton_cdf_is_bit_identical_to_the_javascript_fingerprints() {
        // Sampling cannot guard this: a one-ulp shift moves a boundary by ~1e-16,
        // so no finite number of sampled degrees will land in the gap — yet two
        // ends that disagree there WILL hit it eventually, mid-transfer.
        let golden: &[(usize, &str)] = &[
            (1, "0x8c6a9878"),
            (2, "0x2417b297"),
            (17, "0x2ba41e3c"),
            (179, "0xe8b6340a"),
            (716, "0x28d31438"),
            (5000, "0x357a4c9a"),
            (22000, "0xfc512a92"),
        ];
        for &(k, expected) in golden {
            let cdf = soliton_cdf(k);
            let bytes: &[u8] = bytemuck::cast_slice(&cdf);
            assert_eq!(
                hex32(fnv1a(bytes)),
                expected,
                "k={k} degree distribution changed — senders and receivers will desync"
            );
        }
    }

    #[test]
    fn frame_indices_matches_the_javascript_subsets() {
        let golden: &[(usize, [&[u32]; 5])] = &[
            (1, [&[0], &[0], &[0], &[0], &[0]]),
            (2, [&[1], &[1], &[1], &[0], &[1]]),
            (17, [&[3, 14], &[12, 0], &[6, 8], &[15, 16, 13], &[11, 2, 16]]),
            (179, [&[27, 39], &[30, 55], &[155, 125], &[28, 132, 88], &[39, 75, 24]]),
            (716, [&[27, 397], &[567, 592], &[155, 304], &[386, 311, 625], &[39, 433, 382]]),
        ];
        let seqs = [0u32, 1, 2, 41, 1000];
        for (k, expected) in golden {
            let cdf = soliton_cdf(*k);
            for (i, seq) in seqs.iter().enumerate() {
                assert_eq!(
                    frame_indices(*k, &cdf, 4242, *seq),
                    expected[i],
                    "k={k} seq={seq} subset changed — breaking wire-format change"
                );
            }
        }
    }

    #[test]
    fn frame_indices_always_yields_distinct_in_range_blocks() {
        for k in [1usize, 2, 17, 179, 4096] {
            let cdf = soliton_cdf(k);
            for seq in 0..3000u32 {
                let idx = frame_indices(k, &cdf, 9, seq);
                assert!(!idx.is_empty() && idx.len() <= k, "k={k} seq={seq} degree {}", idx.len());
                let unique: HashSet<u32> = idx.iter().copied().collect();
                assert_eq!(unique.len(), idx.len(), "k={k} seq={seq} repeated a block index");
                assert!(idx.iter().all(|&b| (b as usize) < k), "k={k} seq={seq} index out of range");
            }
        }
    }

    #[test]
    fn a_different_session_picks_a_different_subset_for_the_same_seq() {
        let cdf = soliton_cdf(179);
        assert_ne!(frame_indices(179, &cdf, 1, 0), frame_indices(179, &cdf, 2, 0));
    }

    // ---------------------------------------------------- full encoder stream

    #[test]
    fn the_encoded_stream_is_byte_identical_to_the_javascript_fingerprint() {
        // The end-to-end pin: dlog, soliton_cdf, frame_seed, splitmix32,
        // frame_indices, the block padding and the XOR order, in one hash.
        let golden: &[(usize, usize, u16, &str)] = &[
            (1, 64, 1, "k=1 fnv=0xf6a115c5"),
            (23, 64, 7, "k=23 fnv=0x2aafe48d"),
            (179, 2933, 4242, "k=179 fnv=0x83bbd1d7"),
            (716, 1445, 65535, "k=716 fnv=0x15e10360"),
        ];
        for &(k, block_len, session_id, expected) in golden {
            let encoder = LtEncoder::new(&test_payload(k * block_len - 7), block_len, session_id);
            let mut stream = Vec::with_capacity(64 * block_len);
            for seq in 0..64u32 {
                stream.extend_from_slice(&encoder.encode(seq));
            }
            let actual = format!("k={} fnv={}", encoder.k, hex32(fnv1a(&stream)));
            assert_eq!(actual, expected, "stream for k={k}/{block_len}/{session_id} changed");
        }
    }

    #[test]
    fn every_frame_is_exactly_block_len_bytes() {
        // The sender pins the QR version off the first frame, so a short tail
        // frame would silently make every later code undecodable.
        let block_len = 1445;
        let encoder = LtEncoder::new(&test_payload(block_len * 5 + 1), block_len, 3);
        assert_eq!(encoder.k, 6);
        for seq in 0..200u32 {
            assert_eq!(encoder.encode(seq).len(), block_len);
        }
    }

    // ------------------------------------------------------------ round trip

    fn round_trip(len: usize, block_len: usize, session_id: u16, drop_rate: f64) -> (usize, Option<Vec<u8>>) {
        let payload = test_payload(len);
        let encoder = LtEncoder::new(&payload, block_len, session_id);
        let mut decoder = LtDecoder::new(encoder.k, block_len, session_id, len);
        let mut rnd = SplitMix32::new(u32::from(session_id));
        let mut seq = 0u32;
        let ceiling = (encoder.k * 80 + 5000) as u32;
        while !decoder.is_complete() && seq < ceiling {
            if f64::from(rnd.next()) / 4_294_967_296.0 >= drop_rate {
                decoder.add_frame(seq, &encoder.encode(seq));
            }
            seq += 1;
        }
        (decoder.frames_new, decoder.assemble())
    }

    #[test]
    fn a_payload_survives_the_fountain_exactly() {
        for &(len, block_len) in &[(7usize, 2933usize), (2933, 2933), (50_000, 1445), (512 * 1024, 2933)] {
            let (_, recovered) = round_trip(len, block_len, 11, 0.0);
            assert_eq!(recovered.as_deref(), Some(test_payload(len).as_slice()), "{len}B did not survive");
        }
    }

    #[test]
    fn dropping_thirty_percent_of_frames_costs_time_never_correctness() {
        let len = 512 * 1024;
        let (frames, recovered) = round_trip(len, 2933, 23, 0.3);
        assert_eq!(recovered.as_deref(), Some(test_payload(len).as_slice()));
        let k = len.div_ceil(2933);
        let overhead = frames as f64 / k as f64;
        // The decoder only ever sees distinct frames, so loss must not inflate
        // the count it needs — only slow their arrival.
        assert!(overhead < 1.6, "unique-frame overhead {overhead:.2} is too high");
    }

    #[test]
    fn a_single_block_payload_completes_on_its_first_frame() {
        let payload = test_payload(900);
        let encoder = LtEncoder::new(&payload, 2933, 5);
        assert_eq!(encoder.k, 1);
        let mut decoder = LtDecoder::new(1, 2933, 5, 900);
        decoder.add_frame(0, &encoder.encode(0));
        assert!(decoder.is_complete());
        assert_eq!(decoder.assemble(), Some(payload));
    }

    #[test]
    fn repeated_frames_are_counted_but_never_corrupt_the_decode() {
        // The camera re-reads the same on-screen frame constantly.
        let len = 60_000;
        let payload = test_payload(len);
        let encoder = LtEncoder::new(&payload, 1445, 31);
        let mut decoder = LtDecoder::new(encoder.k, 1445, 31, len);
        let mut seq = 0u32;
        while !decoder.is_complete() {
            let block = encoder.encode(seq);
            decoder.add_frame(seq, &block);
            decoder.add_frame(seq, &block);
            seq += 1;
        }
        assert!(decoder.frames_dup >= decoder.frames_new - 1);
        assert_eq!(decoder.assemble(), Some(payload));
    }

    #[test]
    fn an_incomplete_decoder_assembles_nothing() {
        let encoder = LtEncoder::new(&test_payload(50_000), 1445, 13);
        let mut decoder = LtDecoder::new(encoder.k, 1445, 13, 50_000);
        decoder.add_frame(0, &encoder.encode(0));
        assert!(!decoder.is_complete());
        assert_eq!(decoder.assemble(), None);
    }

    // ------------------------------------------------------------ frame + file

    #[test]
    fn a_packed_frame_parses_back_to_its_header() {
        let header = FrameHeader {
            session_id: 4242,
            seq: 7,
            k: 179,
            block_len: 2933,
            total_len: 524_000,
            payload_fnv: 0xdead_beef,
        };
        let block = test_payload(2933);
        let bytes = pack_frame(&header, &block);
        assert_eq!(bytes.len(), HEADER_LEN + 2933);
        let (parsed, payload) = parse_frame(&bytes).expect("frame should parse");
        assert_eq!(parsed, header);
        assert_eq!(payload, block.as_slice());
    }

    #[test]
    fn a_frame_with_the_wrong_length_or_magic_is_rejected() {
        let header = FrameHeader {
            session_id: 1,
            seq: 0,
            k: 1,
            block_len: 64,
            total_len: 64,
            payload_fnv: 0,
        };
        let mut bytes = pack_frame(&header, &test_payload(64));
        assert!(parse_frame(&bytes).is_some());
        bytes.push(0);
        assert!(parse_frame(&bytes).is_none(), "a trailing byte must not parse");
        let mut wrong = pack_frame(&header, &test_payload(64));
        wrong[0] = 0x00;
        assert!(parse_frame(&wrong).is_none(), "the magic must be checked");
    }

    #[test]
    fn a_file_survives_the_container_and_the_fountain_together() {
        let bytes = test_payload(200_000);
        let packed = pack_file("notes.txt", "text/plain", &bytes).unwrap();
        let encoder = LtEncoder::new(&packed.container, 2933, 4242);
        let mut decoder =
            LtDecoder::new(encoder.k, 2933, 4242, packed.container.len());
        let mut seq = 0u32;
        while !decoder.is_complete() {
            decoder.add_frame(seq, &encoder.encode(seq));
            seq += 1;
        }
        let container = decoder.assemble().unwrap();
        assert_eq!(fnv1a(&container), packed.payload_fnv);
        let file = unpack_file(&container).unwrap();
        assert_eq!(file.name, "notes.txt");
        assert_eq!(file.media_type, "text/plain");
        assert_eq!(file.bytes, bytes);
        assert!(file.verify());
    }

    #[test]
    fn a_precompressed_file_is_not_gzipped_and_a_text_file_is() {
        let text = pack_file("a.txt", "text/plain", &vec![b'a'; 4096]).unwrap();
        assert_eq!(text.compression, Compression::Gzip);
        assert!(text.transmitted_size < text.original_size);

        // Random-looking bytes in a type gzip is told not to bother with.
        let jpeg = pack_file("a.jpg", "image/jpeg", &test_payload(4096)).unwrap();
        assert_eq!(jpeg.compression, Compression::None);
        assert_eq!(jpeg.transmitted_size, 4096);

        // Small enough that the gzip header would eat the gain.
        let tiny = pack_file("a.txt", "text/plain", &vec![b'a'; 700]).unwrap();
        assert_eq!(tiny.compression, Compression::None);
    }

    #[test]
    fn a_gzipped_container_round_trips() {
        let bytes: Vec<u8> = (0..100_000).map(|i| (i % 61) as u8).collect();
        let packed = pack_file("log.txt", "text/plain", &bytes).unwrap();
        assert_eq!(packed.compression, Compression::Gzip);
        let file = unpack_file(&packed.container).unwrap();
        assert_eq!(file.compression, Compression::Gzip);
        assert_eq!(file.bytes, bytes);
        assert!(file.verify());
    }

    #[test]
    fn a_path_in_the_name_never_reaches_the_wire() {
        let packed = pack_file("../../etc/passwd", "text/plain", b"x").unwrap();
        let file = unpack_file(&packed.container).unwrap();
        assert_eq!(file.name, "passwd");

        let odd = pack_file("bad\nname\u{0}.txt", "text/plain", b"x").unwrap();
        assert_eq!(unpack_file(&odd.container).unwrap().name, "badname.txt");
    }

    #[test]
    fn an_empty_or_oversized_file_is_refused_before_anything_is_drawn() {
        assert!(pack_file("a", "text/plain", b"").is_err());
        assert!(pack_file("a", "text/plain", &vec![0u8; MAX_FILE_BYTES + 1]).is_err());
    }

    #[test]
    fn a_truncated_or_lying_container_is_rejected() {
        let packed = pack_file("a.txt", "text/plain", b"hello there").unwrap();
        assert!(unpack_file(&packed.container[..40]).is_err(), "a short container must fail");

        let mut wrong_magic = packed.container.clone();
        wrong_magic[0] = b'X';
        assert!(unpack_file(&wrong_magic).is_err());

        let mut wrong_len = packed.container.clone();
        wrong_len[9] = 0xff;
        assert!(unpack_file(&wrong_len).is_err(), "a declared length that does not fit must fail");
    }

    // ------------------------------------------------------ verification code

    #[test]
    fn the_verification_code_is_six_digits_and_follows_the_bytes() {
        let packed = pack_file("a.txt", "text/plain", b"the file that was on screen").unwrap();
        let code = packed.verification_code();
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));

        // The receiver derives the same code from the bytes it rebuilt, which is
        // the whole point: sender knows it before the first frame, receiver only
        // after the last one.
        let file = unpack_file(&packed.container).unwrap();
        assert_eq!(file.verification_code(), code);
        assert!(file.verify());

        // A different file gives a different code, and the full hash still
        // disagrees even in the (1-in-a-million) case where the digits collide.
        let other = pack_file("a.txt", "text/plain", b"a different file entirely").unwrap();
        assert_ne!(other.sha256, packed.sha256);
    }

    #[test]
    fn a_corrupted_file_fails_verification_even_if_the_container_parses() {
        // An uncompressed container, so a flipped byte gets all the way past the
        // structural checks and only the hash can catch it. This is the case the
        // SHA-256 exists for: every length still agrees, nothing looks wrong.
        let packed = pack_file("a.jpg", "image/jpeg", &test_payload(4096)).unwrap();
        assert_eq!(packed.compression, Compression::None);
        let mut container = packed.container.clone();
        let last = container.len() - 1;
        container[last] ^= 0xff;
        let file = unpack_file(&container).unwrap();
        assert!(!file.verify(), "a flipped payload byte must fail the hash");
        assert_ne!(file.verification_code(), packed.verification_code());
    }

    #[test]
    fn a_corrupted_gzip_container_is_refused_before_it_is_ever_unpacked() {
        // gzip carries its own CRC, so a compressed transfer gets a second,
        // earlier line of defence — it fails at inflate rather than at the hash.
        let packed = pack_file("a.txt", "text/plain", &vec![b'a'; 4096]).unwrap();
        assert_eq!(packed.compression, Compression::Gzip);
        let mut container = packed.container.clone();
        let mid = container.len() - 12;
        container[mid] ^= 0xff;
        assert!(unpack_file(&container).is_err());
    }

    /// Emit real frames for the cross-language harness.
    ///
    /// The golden vectors above pin this port against recorded constants, which
    /// is necessary but not sufficient: recorded constants only prove we did not
    /// drift from what we transcribed. The claim that actually matters — a
    /// runnir sender is decodable by the JavaScript receiver that ships on the
    /// website — can only be settled by handing frames to that receiver.
    ///
    /// Ignored because it writes a file and the harness runs it deliberately:
    /// `RUNNIR_OPTICAL_VECTORS=/path/vectors.json cargo test -- --ignored
    /// emit_cross_check_vectors`, then `node cross-check.mjs /path/vectors.json`
    /// from a decimen checkout. See docs/DEVLOG.md.
    #[test]
    #[ignore = "writes vectors for the JavaScript cross-check; run it explicitly"]
    fn emit_cross_check_vectors() {
        let Ok(path) = std::env::var("RUNNIR_OPTICAL_VECTORS") else {
            panic!("set RUNNIR_OPTICAL_VECTORS to the output path");
        };

        // Three shapes on purpose: one block (the whole file in a single frame),
        // a compressible file that takes the gzip path, and a payload big enough
        // to exercise the peeling cascade over hundreds of blocks.
        // Text that gzip helps with but cannot collapse to a single block, so
        // the compressed case still exercises the peeling cascade.
        let log: Vec<u8> = (0..6000)
            .flat_map(|i| format!("2026-08-02 12:{:02}:{:02} worker {i} handled request {}\n", i % 60, (i * 7) % 60, i * 31 % 99991).into_bytes())
            .collect();
        let cases: Vec<(&str, &str, Vec<u8>, usize, u16)> = vec![
            ("hello.txt", "text/plain", b"a file that fits in one frame".to_vec(), 2933, 4242),
            ("log.txt", "text/plain", log, 2933, 7),
            ("noise.jpg", "image/jpeg", test_payload(300_000), 1445, 65535),
        ];

        let mut out = String::from("[\n");
        for (i, (name, media_type, bytes, block_len, session_id)) in cases.iter().enumerate() {
            let packed = pack_file(name, media_type, bytes).unwrap();
            let encoder = LtEncoder::new(&packed.container, *block_len, *session_id);
            // The harness drops a third of these to imitate a camera, so 2.5x
            // leaves about 1.7x arriving — comfortably above the ~1.15x the
            // fountain needs. Sizing this for the pre-drop count is how the
            // first run "failed": it starved the decoder instead of testing it.
            let count = (encoder.k as f64 * 2.5).ceil() as u32 + 16;
            let frames: Vec<String> = (0..count)
                .map(|seq| {
                    let frame = encoder.frame(seq, packed.container.len(), packed.payload_fnv);
                    frame.iter().map(|b| format!("{b:02x}")).collect()
                })
                .collect();
            out.push_str(&format!(
                "  {{\"name\":\"{name}\",\"mediaType\":\"{media_type}\",\"sessionId\":{session_id},\
                 \"blockLen\":{block_len},\"k\":{},\"totalLen\":{},\"payloadFnv\":{},\
                 \"originalSize\":{},\"compression\":\"{}\",\"sha256\":\"{}\",\"code\":\"{}\",\
                 \"frames\":[\"{}\"]}}{}\n",
                encoder.k,
                packed.container.len(),
                packed.payload_fnv,
                packed.original_size,
                if packed.compression == Compression::Gzip { "gzip" } else { "none" },
                packed.sha256.iter().map(|b| format!("{b:02x}")).collect::<String>(),
                packed.verification_code(),
                frames.join("\",\""),
                if i + 1 == cases.len() { "" } else { "," }
            ));
        }
        out.push_str("]\n");
        std::fs::write(&path, out).unwrap();
        eprintln!("wrote cross-check vectors to {path}");
    }

    // ---------------------------------------------------------- frame budget

    #[test]
    fn a_payload_too_large_for_a_frame_size_is_caught_before_streaming() {
        // k is a u16, so 500 bytes per frame tops out around 30 MB.
        assert!(fits_in_one_stream(1_000_000, 2953));
        assert!(!fits_in_one_stream(64 * 1024 * 1024, 520));
        assert!(minimum_frame_bytes(64 * 1024 * 1024) > 1024);
        assert_eq!(block_length(2953), 2933);
    }
}
