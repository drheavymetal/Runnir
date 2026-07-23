use std::collections::HashMap;

use swash::FontRef;
use swash::scale::image::Content;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::shape::ShapeContext;
use swash::text::Script;
use swash::zeno::Format;

use crate::grid::Flags;

const ATLAS_SIZE: u32 = 1024;

/// Default family. The "Mono" Nerd Font variant squeezes every icon into one
/// cell; the plain variant makes them double-width, which needs wide-char support
/// the grid does not have yet.
const DEFAULT_FAMILY: &str = "JetBrainsMono Nerd Font Mono";

/// Tried in order when the main face has no glyph for a codepoint. CJK matters
/// here: no Latin monospace face covers it, so without an entry every kanji is a
/// box.
const FALLBACK_FAMILIES: &[&str] = &[
    "Noto Sans Mono",
    "DejaVu Sans Mono",
    "Noto Sans Symbols 2",
    // The "Mono CJK" variant, not plain "Sans CJK": a terminal wants fixed
    // advances even in the fallback.
    "Noto Sans Mono CJK HK",
    "Noto Color Emoji",
    // Elder Futhark (U+16A0..), for the map's rune rain. No monospace face on a
    // normal system carries the Runic block at all — checked: of everything
    // installed here only this and FreeMono cover the 24 letters — so the choice is
    // a proportional fallback or empty boxes. Last in the chain, so it is reached
    // only by codepoints nothing above it can draw.
    "Noto Sans Runic",
];

/// Which of the four faces a cell needs. Bold and italic are separate files, not
/// a transform of the regular outline.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Style(u8);

impl Style {
    pub const REGULAR: Self = Self(0);

    pub fn from_flags(flags: Flags) -> Self {
        let mut bits = 0;
        if flags.contains(Flags::BOLD) {
            bits |= 1;
        }
        if flags.contains(Flags::ITALIC) {
            bits |= 2;
        }
        Self(bits)
    }

    fn index(self) -> usize {
        self.0 as usize
    }
}

/// Where a rasterized glyph lives in the atlas, and how it sits inside its cell.
#[derive(Clone, Copy, Debug, Default)]
pub struct Glyph {
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    /// Pixel offset of the bitmap from the cell's top-left corner.
    pub offset: [f32; 2],
    pub size: [f32; 2],
    /// Emoji carry their own colour and ignore the cell's foreground.
    pub color: bool,
}

struct Face {
    data: Vec<u8>,
    index: u32,
}

impl Face {
    fn font(&self) -> Option<FontRef<'_>> {
        FontRef::from_index(&self.data, self.index as usize)
    }

    fn has(&self, ch: char) -> bool {
        self.font().is_some_and(|f| f.charmap().map(ch) != 0)
    }
}

pub struct FontAtlas {
    data: Vec<u8>,
    /// Indexed by `Style`: regular, bold, italic, bold-italic. Slot 0 always
    /// exists; the others fall back to it when the family ships no such face.
    faces: Vec<Face>,
    fallbacks: Vec<Face>,
    ctx: ScaleContext,
    shape_ctx: ShapeContext,
    glyphs: HashMap<(char, u8), Glyph>,
    /// Shaped glyphs are keyed by id, not codepoint: a ligature has no codepoint.
    shaped_glyphs: HashMap<(u16, u8), Glyph>,
    pub ligatures: bool,
    px: f32,
    // Shelf packer: fill a row left to right, then start a new row below the
    // tallest glyph seen. Good enough for monospace, where glyphs are uniform.
    shelf_x: u32,
    shelf_y: u32,
    shelf_h: u32,
    pub cell_w: f32,
    pub cell_h: f32,
    ascent: f32,
    /// Distance from the cell top to the underline / strikeout, and the stroke
    /// thickness. Taken from the face rather than guessed.
    pub underline_y: f32,
    pub strike_y: f32,
    pub stroke: f32,
    pub dirty: bool,
}

impl FontAtlas {
    pub fn new(px: f32) -> anyhow::Result<Self> {
        Self::new_with(DEFAULT_FAMILY, px)
    }

    /// Builds an atlas for a specific family. The `RUNNIR_FONT` env var still wins,
    /// so a quick override does not need a config edit.
    pub fn new_with(family: &str, px: f32) -> anyhow::Result<Self> {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();

        let family = std::env::var("RUNNIR_FONT").unwrap_or_else(|_| family.to_string());
        // The system's copy always wins. The bundled faces are only registered once
        // nothing installed can answer, so a user who has their own JetBrains Mono —
        // or has deliberately replaced it — keeps theirs, and nobody carries two
        // copies of the same outlines in memory.
        let mut faces = load_family(&db, &family).or_else(|| load_family(&db, DEFAULT_FAMILY));
        if faces.is_none() {
            embed_default_family(&mut db);
            faces = load_family(&db, DEFAULT_FAMILY);
        }
        let faces = faces
            .or_else(|| load_family(&db, "monospace"))
            .ok_or_else(|| anyhow::anyhow!("no monospace font found (tried {family})"))?;

        let mut fallbacks: Vec<Face> =
            FALLBACK_FAMILIES.iter().filter_map(|f| load_one(&db, f)).collect();
        // Runic is the one block that is genuinely absent rather than merely
        // different: no monospace face on a normal system has it, so without this the
        // map's rain draws as empty boxes. Same rule as above — only if nothing
        // installed covers it.
        if !fallbacks.iter().any(|f| f.has('\u{16A0}')) {
            if let Some(face) = embed_runic(&mut db) {
                fallbacks.push(face);
            }
        }

        let (cell_w, cell_h, ascent, underline_y, strike_y, stroke) = {
            let font = faces[0].font().ok_or_else(|| anyhow::anyhow!("unusable font"))?;
            let m = font.metrics(&[]).scale(px);
            let gm = font.glyph_metrics(&[]).scale(px);
            // Every advance is the same in a monospace face; 'M' is just a probe.
            let w = gm.advance_width(font.charmap().map('M')).ceil();
            let h = (m.ascent + m.descent + m.leading).ceil();
            let ascent = m.ascent.ceil();
            // Both offsets count upward from the baseline, which sits `ascent`
            // below the cell top.
            let underline = (ascent - m.underline_offset).round().min(h - 1.0);
            let strike = (ascent - m.strikeout_offset).round();
            (w, h, ascent, underline, strike, m.stroke_size.max(1.0).round())
        };

        eprintln!(
            "runnir: {family} @ {px}px -> cell {cell_w}x{cell_h} ({} faces, {} fallbacks)",
            faces.len(),
            fallbacks.len()
        );

        Ok(Self {
            data: vec![0; (ATLAS_SIZE * ATLAS_SIZE * 4) as usize],
            faces,
            fallbacks,
            ctx: ScaleContext::new(),
            shape_ctx: ShapeContext::new(),
            glyphs: HashMap::new(),
            shaped_glyphs: HashMap::new(),
            ligatures: std::env::var("RUNNIR_NO_LIGATURES").is_err(),
            px,
            shelf_x: 0,
            shelf_y: 0,
            shelf_h: 0,
            cell_w,
            cell_h,
            ascent,
            underline_y,
            strike_y,
            stroke,
            dirty: true,
        })
    }

    pub const fn atlas_size() -> u32 {
        ATLAS_SIZE
    }

    pub fn pixels(&self) -> &[u8] {
        &self.data
    }

    /// Shapes an **ASCII** run into glyph ids, one per source column.
    ///
    /// ASCII-only by design: every ligature in a coding face is an ASCII sequence
    /// (`!=`, `->`, `=>`), so restricting the shaper here means one byte is always
    /// one cell and the returned glyphs line up one-to-one with columns. Runs with
    /// CJK or emoji keep the per-character path, where they belong. A ligature
    /// shows up as blank leading glyphs followed by the real one (see the caller);
    /// the positional alignment is what makes that detectable, so no per-glyph
    /// source span is needed.
    pub fn shape(&mut self, text: &str, style: Style) -> Vec<u16> {
        let face = &self.faces[style.index().min(self.faces.len() - 1)];
        let Some(font) = face.font() else { return Vec::new() };

        let mut shaper = self
            .shape_ctx
            .builder(font)
            .script(Script::Latin)
            .size(self.px)
            // calt drives most of what people call ligatures in coding faces.
            .features(&[("liga", 1), ("calt", 1)])
            .build();
        shaper.add_str(text);

        let mut out = Vec::new();
        shaper.shape_with(|cluster| {
            if let Some(g) = cluster.glyphs.first() {
                out.push(g.id);
            }
        });
        out
    }

    /// Rasterizes a glyph the shaper produced. Keyed by id because a ligature has
    /// no character to key on.
    pub fn shaped_glyph(&mut self, id: u16, style: Style) -> Glyph {
        let key = (id, style.0);
        if let Some(g) = self.shaped_glyphs.get(&key) {
            return *g;
        }
        let g = self.rasterize_id(id, style).unwrap_or_default();
        self.shaped_glyphs.insert(key, g);
        g
    }

    /// Rasterizes `ch` in `style` on first use and caches it.
    pub fn glyph(&mut self, ch: char, style: Style) -> Glyph {
        let key = (ch, style.0);
        if let Some(g) = self.glyphs.get(&key) {
            return *g;
        }
        let g = self.rasterize(ch, style).unwrap_or_default();
        self.glyphs.insert(key, g);
        g
    }

    /// Picks the face that actually has the codepoint: the requested style first,
    /// then regular, then the fallback chain. Returns notdef from the regular face
    /// if nothing has it, so a missing glyph shows a box rather than vanishing.
    fn face_for(&self, ch: char, style: Style) -> usize {
        let want = style.index().min(self.faces.len() - 1);
        if self.faces[want].has(ch) {
            return want;
        }
        if self.faces[0].has(ch) {
            return 0;
        }
        for (i, f) in self.fallbacks.iter().enumerate() {
            if f.has(ch) {
                return self.faces.len() + i;
            }
        }
        want
    }

    fn rasterize(&mut self, ch: char, style: Style) -> Option<Glyph> {
        if crate::boxdraw::owns(ch) {
            return self.rasterize_box(ch);
        }
        let which = self.face_for(ch, style);
        let face = if which < self.faces.len() {
            &self.faces[which]
        } else {
            &self.fallbacks[which - self.faces.len()]
        };
        let glyph_id = face.font()?.charmap().map(ch);
        self.rasterize_glyph(which, glyph_id)
    }

    /// Shaped glyphs always come from the main faces: the shaper only ever runs
    /// against those, so a fallback index would be meaningless here.
    fn rasterize_id(&mut self, glyph_id: u16, style: Style) -> Option<Glyph> {
        self.rasterize_glyph(style.index().min(self.faces.len() - 1), glyph_id)
    }

    fn rasterize_glyph(&mut self, which: usize, glyph_id: u16) -> Option<Glyph> {
        let face = if which < self.faces.len() {
            &self.faces[which]
        } else {
            &self.fallbacks[which - self.faces.len()]
        };

        let font = face.font()?;
        let mut scaler = self.ctx.builder(font).size(self.px).hint(true).build();

        let image = Render::new(&[
            Source::ColorOutline(0),
            Source::ColorBitmap(StrikeWith::BestFit),
            Source::Outline,
        ])
        .format(Format::Alpha)
        .render(&mut scaler, glyph_id)?;

        let (w, h) = (image.placement.width, image.placement.height);
        if w == 0 || h == 0 {
            // Space and friends: no pixels, but still a valid cached result.
            return Some(Glyph::default());
        }

        let color = image.content == Content::Color;
        let (x, y) = self.alloc(w, h)?;
        for row in 0..h {
            for px in 0..w {
                let dst = (((y + row) * ATLAS_SIZE + x + px) * 4) as usize;
                let texel: [u8; 4] = if color {
                    let src = ((row * w + px) * 4) as usize;
                    image.data[src..src + 4].try_into().ok()?
                } else {
                    // Store a mask as white with the coverage in alpha, so one
                    // shader path handles both: the cell's fg multiplies the white.
                    [255, 255, 255, image.data[(row * w + px) as usize]]
                };
                self.data[dst..dst + 4].copy_from_slice(&texel);
            }
        }
        self.dirty = true;

        let s = ATLAS_SIZE as f32;
        Some(Glyph {
            uv_min: [x as f32 / s, y as f32 / s],
            uv_max: [(x + w) as f32 / s, (y + h) as f32 / s],
            // The bitmap hangs off the baseline, which sits `ascent` below the
            // cell top. `placement.top` counts upward from the baseline.
            offset: [image.placement.left as f32, self.ascent - image.placement.top as f32],
            size: [w as f32, h as f32],
            color,
        })
    }

    /// Box-drawing glyphs are drawn at exactly cell size and placed at the cell
    /// origin, which is what makes neighbouring cells join with no seam.
    fn rasterize_box(&mut self, ch: char) -> Option<Glyph> {
        let (w, h) = (self.cell_w as u32, self.cell_h as u32);
        let mask = crate::boxdraw::draw(ch, w, h, self.stroke)?;
        let (x, y) = self.alloc(w, h)?;
        for row in 0..h {
            for px in 0..w {
                let dst = (((y + row) * ATLAS_SIZE + x + px) * 4) as usize;
                let a = mask[(row * w + px) as usize];
                self.data[dst..dst + 4].copy_from_slice(&[255, 255, 255, a]);
            }
        }
        self.dirty = true;

        let s = ATLAS_SIZE as f32;
        Some(Glyph {
            uv_min: [x as f32 / s, y as f32 / s],
            uv_max: [(x + w) as f32 / s, (y + h) as f32 / s],
            offset: [0.0, 0.0],
            size: [w as f32, h as f32],
            color: false,
        })
    }

    fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        // A glyph bigger than the atlas can never be placed; bail rather than
        // return a position that would write out of bounds. Unreachable at real
        // cell sizes, but a missing bound is a missing bound.
        if w > ATLAS_SIZE || h > ATLAS_SIZE {
            return None;
        }
        if self.shelf_x + w > ATLAS_SIZE {
            self.shelf_x = 0;
            self.shelf_y += self.shelf_h + 1;
            self.shelf_h = 0;
        }
        if self.shelf_y + h > ATLAS_SIZE {
            return None; // Atlas full. Growing/evicting is future work.
        }
        let pos = (self.shelf_x, self.shelf_y);
        self.shelf_x += w + 1;
        self.shelf_h = self.shelf_h.max(h);
        Some(pos)
    }
}

/// Loads regular/bold/italic/bold-italic for `family`. A family with no regular
/// face is not usable; missing bold or italic just reuse the regular one.
/// The four faces of the default family, compiled in.
///
/// Bundled so a machine that has just run `install.sh` looks the way the terminal was
/// designed to look, rather than the way its distro's default monospace looks. Licence
/// notes — these are multi-licence, the outlines and the patched-in icons differing —
/// are in `assets/fonts/NOTICE.md`.
const EMBEDDED_FAMILY: &[&[u8]] = &[
    include_bytes!("../assets/fonts/JetBrainsMonoNerdFontMono-Regular.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMonoNerdFontMono-Bold.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMonoNerdFontMono-Italic.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMonoNerdFontMono-BoldItalic.ttf"),
];

/// Elder Futhark, compiled in. 9.6 KB, and the only way the rune rain is runes.
const EMBEDDED_RUNIC: &[u8] = include_bytes!("../assets/fonts/NotoSansRunic-Regular.ttf");

/// The 24 letters of Elder Futhark, which is what the map's rain falls in.
///
/// Elder Futhark and not the later rows: it is the 24-letter one, it is the one whose
/// shapes read as "runes" at a glance, and the block holds later additions that would
/// look like noise mixed in. Lives here because this module owns the question of which
/// codepoints have to be drawable at all.
pub const RUNES: [char; 24] = [
    '\u{16A0}', '\u{16A2}', '\u{16A6}', '\u{16A8}', '\u{16B1}', '\u{16B2}',
    '\u{16B7}', '\u{16B9}', '\u{16BB}', '\u{16BE}', '\u{16C1}', '\u{16C3}',
    '\u{16C7}', '\u{16C8}', '\u{16C9}', '\u{16CA}', '\u{16CF}', '\u{16D2}',
    '\u{16D6}', '\u{16D7}', '\u{16DA}', '\u{16DC}', '\u{16DE}', '\u{16DF}',
];

/// Registers the bundled default family, so the ordinary query path finds it.
fn embed_default_family(db: &mut fontdb::Database) {
    for data in EMBEDDED_FAMILY {
        db.load_font_data(data.to_vec());
    }
}

/// Registers the bundled runic face and hands it back, or `None` if it will not load —
/// in which case the rain simply has no runes, which is the same soft failure every
/// other missing glyph gets.
fn embed_runic(db: &mut fontdb::Database) -> Option<Face> {
    db.load_font_data(EMBEDDED_RUNIC.to_vec());
    load_one(db, "Noto Sans Runic")
}

fn load_family(db: &fontdb::Database, family: &str) -> Option<Vec<Face>> {
    let regular = load(db, family, fontdb::Weight::NORMAL, fontdb::Style::Normal)?;
    let bold = load(db, family, fontdb::Weight::BOLD, fontdb::Style::Normal);
    let italic = load(db, family, fontdb::Weight::NORMAL, fontdb::Style::Italic);
    let bold_italic = load(db, family, fontdb::Weight::BOLD, fontdb::Style::Italic);

    let same_as_regular = || Face { data: regular.data.clone(), index: regular.index };
    Some(vec![
        same_as_regular(),
        bold.unwrap_or_else(same_as_regular),
        italic.unwrap_or_else(same_as_regular),
        bold_italic.unwrap_or_else(same_as_regular),
    ])
}

/// Loads a fallback face. Unlike the main family this accepts whatever weight the
/// query lands on: a fallback only has to *have* the glyph. Being strict here
/// silently drops whole families — Noto Sans CJK ships as a `.ttc` whose faces
/// report weights that never match an exact request, so demanding NORMAL leaves
/// every kanji as a box.
fn load_one(db: &fontdb::Database, family: &str) -> Option<Face> {
    query(db, family, fontdb::Weight::NORMAL, fontdb::Style::Normal, false)
}

fn load(
    db: &fontdb::Database,
    family: &str,
    weight: fontdb::Weight,
    style: fontdb::Style,
) -> Option<Face> {
    query(db, family, weight, style, true)
}

fn query(
    db: &fontdb::Database,
    family: &str,
    weight: fontdb::Weight,
    style: fontdb::Style,
    strict: bool,
) -> Option<Face> {
    let id = db.query(&fontdb::Query {
        families: &[fontdb::Family::Name(family)],
        weight,
        style,
        stretch: fontdb::Stretch::Normal,
    })?;
    let info = db.face(id)?;
    // fontdb's query returns *some* face rather than failing, so the main family
    // must confirm the match really is the weight/style asked for. Otherwise
    // "bold" resolves to the regular face and nothing ever looks bold.
    if strict && (info.weight != weight || info.style != style) {
        return None;
    }
    if std::env::var("RUNNIR_FONT_DEBUG").is_ok() {
        eprintln!(
            "  load({family}, {weight:?}, {style:?}, strict={strict}) -> {:?} idx {}",
            info.source, info.index
        );
    }
    db.with_face_data(id, |data, index| Face { data: data.to_vec(), index })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every Elder Futhark letter the rain draws has to be IN the bundled face.
    ///
    /// Bundled precisely because no monospace face on a normal system carries the
    /// Runic block, so this is the only thing standing between the rain and a screen
    /// of empty boxes. It reads the compiled-in bytes and nothing else, so it says the
    /// same thing on a machine with no fonts installed at all — which is exactly the
    /// machine the bundle exists for.
    #[test]
    fn the_bundled_runic_face_has_every_rune_the_rain_uses() {
        let face = Face { data: EMBEDDED_RUNIC.to_vec(), index: 0 };
        assert!(face.font().is_some(), "the bundled runic face does not parse");
        let missing: Vec<char> = RUNES.iter().copied().filter(|c| !face.has(*c)).collect();
        assert!(missing.is_empty(), "runes the bundled face cannot draw: {missing:?}");
        assert_eq!(RUNES.len(), 24, "Elder Futhark is 24 letters");
    }

    /// The default family is bundled too, and a corrupt or truncated copy would only
    /// show up on the machine that has nothing installed — the one nobody tests on.
    #[test]
    fn every_bundled_face_of_the_default_family_parses() {
        for (i, data) in EMBEDDED_FAMILY.iter().enumerate() {
            let face = Face { data: data.to_vec(), index: 0 };
            assert!(face.font().is_some(), "bundled face {i} does not parse");
            assert!(face.has('M'), "bundled face {i} cannot draw an M");
        }
    }

    /// Not an assertion, a lens: prints what fontdb actually indexed for CJK.
    /// fontconfig and fontdb disagree about `.ttc` collections often enough that
    /// guessing family names is a waste of time.
    #[test]
    #[ignore = "diagnostic"]
    fn list_cjk_families_fontdb_sees() {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let mut names: Vec<String> = db
            .faces()
            .flat_map(|f| f.families.iter().map(|(n, _)| n.clone()))
            .filter(|n| n.contains("CJK"))
            .collect();
        names.sort();
        names.dedup();
        eprintln!("fontdb CJK families ({}):", names.len());
        for n in names.iter().take(12) {
            eprintln!("  {n:?}");
        }
        eprintln!("total faces indexed: {}", db.len());

        // Walk the same path `load_one` takes and report which step drops it.
        for family in ["Noto Sans Mono CJK HK", "Noto Sans CJK HK"] {
            let id = db.query(&fontdb::Query {
                families: &[fontdb::Family::Name(family)],
                weight: fontdb::Weight::NORMAL,
                style: fontdb::Style::Normal,
                stretch: fontdb::Stretch::Normal,
            });
            match id {
                None => eprintln!("{family}: query -> None"),
                Some(id) => {
                    let info = db.face(id);
                    eprintln!(
                        "{family}: query -> ok, face={:?}, weight={:?}, style={:?}",
                        info.map(|i| i.index),
                        info.map(|i| i.weight),
                        info.map(|i| i.style)
                    );
                    let data = db.with_face_data(id, |d, i| (d.len(), i));
                    eprintln!("  with_face_data -> {data:?}");
                    let loaded = load_one(&db, family);
                    eprintln!("  load_one -> {:?}", loaded.map(|f| f.data.len()));
                }
            }
        }
    }

    /// The definitive ligature check: a ligated sequence must come back as ONE
    /// glyph covering N source bytes. Eyeballing a render cannot tell a ligature
    /// from kerning.
    #[test]
    #[ignore = "diagnostic"]
    fn report_ligature_shaping() {
        let mut atlas = FontAtlas::new(16.0).unwrap();
        // Sanity first: what does the charmap say each character's glyph is?
        // If shaping disagrees with this for a lone character, the shaper is being
        // driven wrong — not the font's fault.
        for ch in ['a', 'b', '!', '-', '=', '>'] {
            let db_id = {
                let f = atlas.faces[0].font().unwrap();
                f.charmap().map(ch)
            };
            let shaped = atlas.shape(&ch.to_string(), Style::REGULAR);
            eprintln!("  {ch:?}: charmap={db_id} shaped={shaped:?}");
        }
        eprintln!("--");
        for seq in ["ab", "!=", "->", "=>", "===", "<=", "|>", "::", "//"] {
            let shaped = atlas.shape(seq, Style::REGULAR);
            // A ligature shows as a blank leading glyph (size 0) followed by a real
            // one; print the ids so a mismatch points at the font, not the parser.
            let blanks: Vec<bool> =
                shaped.iter().map(|&id| atlas.shaped_glyph(id, Style::REGULAR).size[0] == 0.0).collect();
            eprintln!("{seq:5} -> {shaped:?} blank={blanks:?}");
        }
    }

    /// Diagnoses which file backs each face and which one owns a codepoint. Prints
    /// on failure so a missing glyph points at the font, not at the parser.
    #[test]
    fn faces_cover_the_prompt_glyphs() {
        let db = {
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            db
        };
        let faces = load_family(&db, DEFAULT_FAMILY).expect("default family must load");
        let fallbacks: Vec<Face> =
            FALLBACK_FAMILIES.iter().filter_map(|f| load_one(&db, f)).collect();

        // Powerline and material-design glyphs live in the user's starship prompt;
        // the kanji catches a fallback family silently failing to load.
        for ch in ['A', '\u{e0b6}', '\u{f0372}', '世', 'ハ'] {
            let in_main = faces[0].has(ch);
            let in_fallback = fallbacks.iter().any(|f| f.has(ch));
            assert!(
                in_main || in_fallback,
                "U+{:04X} has no face: main={in_main} fallback={in_fallback}",
                ch as u32
            );
        }
    }
}
