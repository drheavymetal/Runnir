//! Procedural box-drawing and block-element glyphs.
//!
//! Fonts ship these characters, but their strokes are sized for the font's own
//! metrics, not for the terminal's cell. Drawn from the font they leave visible
//! gaps wherever two cells meet, which is exactly where a box needs to connect.
//! Drawing them ourselves at exactly cell size makes every join seamless — this
//! is why kitty and Ghostty do the same.

/// Arm weights, clockwise from the top: `[up, right, down, left]`.
/// 0 = none, 1 = light, 2 = heavy, 3 = double.
type Arms = [u8; 4];

pub struct Canvas {
    w: usize,
    h: usize,
    data: Vec<u8>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Self {
        Self { w, h, data: vec![0; w * h] }
    }

    /// Pixel-snapped fill. Box drawing must stay crisp: antialiased edges are
    /// what make adjacent cells look like they do not touch.
    fn rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let x0 = (x0.round().max(0.0) as usize).min(self.w);
        let x1 = (x1.round().max(0.0) as usize).min(self.w);
        let y0 = (y0.round().max(0.0) as usize).min(self.h);
        let y1 = (y1.round().max(0.0) as usize).min(self.h);
        for y in y0..y1 {
            self.data[y * self.w + x0..y * self.w + x1].fill(255);
        }
    }

    fn plot(&mut self, x: usize, y: usize, a: u8) {
        if x < self.w && y < self.h {
            let px = &mut self.data[y * self.w + x];
            *px = (*px).max(a);
        }
    }
}

/// Renders `ch` as an alpha mask of exactly `w` x `h`, or `None` if this module
/// does not own that character.
pub fn draw(ch: char, w: u32, h: u32, stroke: f32) -> Option<Vec<u8>> {
    let (w, h) = (w.max(1) as usize, h.max(1) as usize);
    let mut c = Canvas::new(w, h);
    let light = stroke.max(1.0);
    let heavy = (light * 2.0).min(h as f32 / 3.0).max(light + 1.0);

    match ch as u32 {
        0x2500..=0x257F => {
            let arms = arms(ch)?;
            if arms.contains(&3) {
                doubles(&mut c, arms, light);
            } else {
                lines(&mut c, arms, light, heavy);
            }
        }
        0x2580..=0x259F => blocks(&mut c, ch)?,
        _ => return None,
    }
    Some(c.data)
}

/// Double lines are not "a thicker arm": each junction has an *outer* rail and an
/// *inner* rail that terminate at different points. Treating them as one stroke
/// leaves the rails broken at every corner.
///
/// Rails sit at `x1`/`x2` and `y1`/`y2`; every case below decides, per rail, which
/// span of the cell it covers.
fn doubles(c: &mut Canvas, arms: Arms, t: f32) {
    let (w, h) = (c.w as f32, c.h as f32);
    let (cx, cy) = (w / 2.0, h / 2.0);
    let x1 = cx - t / 2.0 - t;
    let x2 = cx + t / 2.0;
    let y1 = cy - t / 2.0 - t;
    let y2 = cy + t / 2.0;

    let (up, right, down, left) = (arms[0] == 3, arms[1] == 3, arms[2] == 3, arms[3] == 3);

    // Vertical rails.
    let v = |c: &mut Canvas, x: f32, y0: f32, y3: f32| c.rect(x, y0, x + t, y3);
    // Horizontal rails.
    let hz = |c: &mut Canvas, y: f32, xa: f32, xb: f32| c.rect(xa, y, xb, y + t);

    match (up, right, down, left) {
        // ═ ║ : straight through.
        (false, true, false, true) => {
            hz(c, y1, 0.0, w);
            hz(c, y2, 0.0, w);
        }
        (true, false, true, false) => {
            v(c, x1, 0.0, h);
            v(c, x2, 0.0, h);
        }

        // ╔ : outer corner top-left, inner corner bottom-right.
        (false, true, true, false) => {
            hz(c, y1, x1, w);
            hz(c, y2, x2, w);
            v(c, x1, y1, h);
            v(c, x2, y2, h);
        }
        // ╗
        (false, false, true, true) => {
            hz(c, y1, 0.0, x2 + t);
            hz(c, y2, 0.0, x1 + t);
            v(c, x2, y1, h);
            v(c, x1, y2, h);
        }
        // ╚
        (true, true, false, false) => {
            hz(c, y2, x1, w);
            hz(c, y1, x2, w);
            v(c, x1, 0.0, y2 + t);
            v(c, x2, 0.0, y1 + t);
        }
        // ╝
        (true, false, false, true) => {
            hz(c, y2, 0.0, x2 + t);
            hz(c, y1, 0.0, x1 + t);
            v(c, x2, 0.0, y2 + t);
            v(c, x1, 0.0, y1 + t);
        }

        // ╠ : the left rail runs through; the right one is cut by the branch.
        (true, true, true, false) => {
            v(c, x1, 0.0, h);
            v(c, x2, 0.0, y1 + t);
            v(c, x2, y2, h);
            hz(c, y1, x2, w);
            hz(c, y2, x2, w);
        }
        // ╣
        (true, false, true, true) => {
            v(c, x2, 0.0, h);
            v(c, x1, 0.0, y1 + t);
            v(c, x1, y2, h);
            hz(c, y1, 0.0, x1 + t);
            hz(c, y2, 0.0, x1 + t);
        }
        // ╦
        (false, true, true, true) => {
            hz(c, y1, 0.0, w);
            hz(c, y2, 0.0, x1 + t);
            hz(c, y2, x2, w);
            v(c, x1, y2, h);
            v(c, x2, y2, h);
        }
        // ╩
        (true, true, false, true) => {
            hz(c, y2, 0.0, w);
            hz(c, y1, 0.0, x1 + t);
            hz(c, y1, x2, w);
            v(c, x1, 0.0, y1 + t);
            v(c, x2, 0.0, y1 + t);
        }

        // ╬ : four separate corners, no rail crosses the middle.
        (true, true, true, true) => {
            v(c, x1, 0.0, y1 + t);
            v(c, x2, 0.0, y1 + t);
            v(c, x1, y2, h);
            v(c, x2, y2, h);
            hz(c, y1, 0.0, x1 + t);
            hz(c, y1, x2, w);
            hz(c, y2, 0.0, x1 + t);
            hz(c, y2, x2, w);
        }
        _ => {}
    }
}

pub fn owns(ch: char) -> bool {
    matches!(ch as u32, 0x2500..=0x259F) && (arms(ch).is_some() || is_block(ch))
}

fn thickness(weight: u8, light: f32, heavy: f32) -> f32 {
    match weight {
        1 => light,
        2 => heavy,
        _ => light,
    }
}

fn lines(c: &mut Canvas, arms: Arms, light: f32, heavy: f32) {
    let (w, h) = (c.w as f32, c.h as f32);
    let (cx, cy) = (w / 2.0, h / 2.0);

    for (i, &weight) in arms.iter().enumerate() {
        if weight == 0 {
            continue;
        }
        let t = thickness(weight, light, heavy);
        // Arms run from the cell edge to the centre, overlapping in the middle so
        // the junction is solid.
        match i {
            0 => c.rect(cx - t / 2.0, 0.0, cx + t / 2.0, cy + t / 2.0),
            1 => c.rect(cx - t / 2.0, cy - t / 2.0, w, cy + t / 2.0),
            2 => c.rect(cx - t / 2.0, cy - t / 2.0, cx + t / 2.0, h),
            _ => c.rect(0.0, cy - t / 2.0, cx + t / 2.0, cy + t / 2.0),
        }
    }
}

fn is_block(ch: char) -> bool {
    matches!(ch as u32, 0x2580..=0x259F)
}

fn blocks(c: &mut Canvas, ch: char) -> Option<()> {
    let (w, h) = (c.w as f32, c.h as f32);
    let eighth_v = |n: f32| h * n / 8.0;
    let eighth_h = |n: f32| w * n / 8.0;

    match ch as u32 {
        0x2580 => c.rect(0.0, 0.0, w, h / 2.0),                    // ▀
        0x2581 => c.rect(0.0, h - eighth_v(1.0), w, h),            // ▁
        0x2582 => c.rect(0.0, h - eighth_v(2.0), w, h),            // ▂
        0x2583 => c.rect(0.0, h - eighth_v(3.0), w, h),            // ▃
        0x2584 => c.rect(0.0, h - eighth_v(4.0), w, h),            // ▄
        0x2585 => c.rect(0.0, h - eighth_v(5.0), w, h),            // ▅
        0x2586 => c.rect(0.0, h - eighth_v(6.0), w, h),            // ▆
        0x2587 => c.rect(0.0, h - eighth_v(7.0), w, h),            // ▇
        0x2588 => c.rect(0.0, 0.0, w, h),                          // █
        0x2589 => c.rect(0.0, 0.0, eighth_h(7.0), h),              // ▉
        0x258A => c.rect(0.0, 0.0, eighth_h(6.0), h),              // ▊
        0x258B => c.rect(0.0, 0.0, eighth_h(5.0), h),              // ▋
        0x258C => c.rect(0.0, 0.0, w / 2.0, h),                    // ▌
        0x258D => c.rect(0.0, 0.0, eighth_h(3.0), h),              // ▍
        0x258E => c.rect(0.0, 0.0, eighth_h(2.0), h),              // ▎
        0x258F => c.rect(0.0, 0.0, eighth_h(1.0), h),              // ▏
        0x2590 => c.rect(w / 2.0, 0.0, w, h),                      // ▐
        0x2591 => shade(c, 64),                                    // ░
        0x2592 => shade(c, 128),                                   // ▒
        0x2593 => shade(c, 192),                                   // ▓
        0x2594 => c.rect(0.0, 0.0, w, eighth_v(1.0)),              // ▔
        0x2595 => c.rect(w - eighth_h(1.0), 0.0, w, h),            // ▕
        0x2596 => c.rect(0.0, h / 2.0, w / 2.0, h),                // ▖
        0x2597 => c.rect(w / 2.0, h / 2.0, w, h),                  // ▗
        0x2598 => c.rect(0.0, 0.0, w / 2.0, h / 2.0),              // ▘
        0x2599 => {
            c.rect(0.0, 0.0, w / 2.0, h);
            c.rect(0.0, h / 2.0, w, h);
        } // ▙
        0x259A => {
            c.rect(0.0, 0.0, w / 2.0, h / 2.0);
            c.rect(w / 2.0, h / 2.0, w, h);
        } // ▚
        0x259B => {
            c.rect(0.0, 0.0, w, h / 2.0);
            c.rect(0.0, 0.0, w / 2.0, h);
        } // ▛
        0x259C => {
            c.rect(0.0, 0.0, w, h / 2.0);
            c.rect(w / 2.0, 0.0, w, h);
        } // ▜
        0x259D => c.rect(w / 2.0, 0.0, w, h / 2.0),                // ▝
        0x259E => {
            c.rect(w / 2.0, 0.0, w, h / 2.0);
            c.rect(0.0, h / 2.0, w / 2.0, h);
        } // ▞
        0x259F => {
            c.rect(w / 2.0, 0.0, w, h);
            c.rect(0.0, h / 2.0, w, h);
        } // ▟
        _ => return None,
    }
    Some(())
}

/// Shades are a uniform wash, not a dither pattern: a dither at cell scale moirés
/// against the pixel grid and looks like noise.
fn shade(c: &mut Canvas, level: u8) {
    for y in 0..c.h {
        for x in 0..c.w {
            c.plot(x, y, level);
        }
    }
}

fn arms(ch: char) -> Option<Arms> {
    // Table over the characters TUIs actually emit. Anything absent falls back to
    // the font, which is fine for the exotic dashed and mixed-weight variants.
    Some(match ch as u32 {
        0x2500 => [0, 1, 0, 1], // ─
        0x2501 => [0, 2, 0, 2], // ━
        0x2502 => [1, 0, 1, 0], // │
        0x2503 => [2, 0, 2, 0], // ┃
        0x250C => [0, 1, 1, 0], // ┌
        0x250D => [0, 2, 1, 0], // ┍
        0x250E => [0, 1, 2, 0], // ┎
        0x250F => [0, 2, 2, 0], // ┏
        0x2510 => [0, 0, 1, 1], // ┐
        0x2511 => [0, 0, 1, 2], // ┑
        0x2512 => [0, 0, 2, 1], // ┒
        0x2513 => [0, 0, 2, 2], // ┓
        0x2514 => [1, 1, 0, 0], // └
        0x2515 => [1, 2, 0, 0], // ┕
        0x2516 => [2, 1, 0, 0], // ┖
        0x2517 => [2, 2, 0, 0], // ┗
        0x2518 => [1, 0, 0, 1], // ┘
        0x2519 => [1, 0, 0, 2], // ┙
        0x251A => [2, 0, 0, 1], // ┚
        0x251B => [2, 0, 0, 2], // ┛
        0x251C => [1, 1, 1, 0], // ├
        0x251D => [1, 2, 1, 0], // ┝
        0x2520 => [2, 1, 2, 0], // ┠
        0x2523 => [2, 2, 2, 0], // ┣
        0x2524 => [1, 0, 1, 1], // ┤
        0x2525 => [1, 0, 1, 2], // ┥
        0x2528 => [2, 0, 2, 1], // ┨
        0x252B => [2, 0, 2, 2], // ┫
        0x252C => [0, 1, 1, 1], // ┬
        0x252F => [0, 2, 1, 2], // ┯
        0x2530 => [0, 1, 2, 1], // ┰
        0x2533 => [0, 2, 2, 2], // ┳
        0x2534 => [1, 1, 0, 1], // ┴
        0x2537 => [1, 2, 0, 2], // ┷
        0x2538 => [2, 1, 0, 1], // ┸
        0x253B => [2, 2, 0, 2], // ┻
        0x253C => [1, 1, 1, 1], // ┼
        0x253F => [1, 2, 1, 2], // ┿
        0x2542 => [2, 1, 2, 1], // ╂
        0x254B => [2, 2, 2, 2], // ╋
        0x2550 => [0, 3, 0, 3], // ═
        0x2551 => [3, 0, 3, 0], // ║
        0x2554 => [0, 3, 3, 0], // ╔
        0x2557 => [0, 0, 3, 3], // ╗
        0x255A => [3, 3, 0, 0], // ╚
        0x255D => [3, 0, 0, 3], // ╝
        0x2560 => [3, 3, 3, 0], // ╠
        0x2563 => [3, 0, 3, 3], // ╣
        0x2566 => [0, 3, 3, 3], // ╦
        0x2569 => [3, 3, 0, 3], // ╩
        0x256C => [3, 3, 3, 3], // ╬
        // Rounded corners draw as square ones: at 10x19 px the arc is two pixels
        // and reads as a notch, not a curve.
        0x256D => [0, 1, 1, 0], // ╭
        0x256E => [0, 0, 1, 1], // ╮
        0x256F => [1, 0, 0, 1], // ╯
        0x2570 => [1, 1, 0, 0], // ╰
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask(ch: char) -> Vec<u8> {
        draw(ch, 10, 20, 1.0).expect("owned character")
    }

    fn col_filled(m: &[u8], w: usize, h: usize, x: usize) -> usize {
        (0..h).filter(|y| m[y * w + x] > 0).count()
    }

    fn row_filled(m: &[u8], w: usize, y: usize) -> usize {
        (0..w).filter(|x| m[y * w + x] > 0).count()
    }

    #[test]
    fn owns_only_its_own_range() {
        assert!(owns('─') && owns('┼') && owns('█') && owns('▄'));
        assert!(!owns('a') && !owns('世') && !owns('\u{2700}'));
    }

    #[test]
    fn horizontal_line_spans_the_full_width() {
        let m = mask('─');
        // The whole point: it must touch both edges so neighbours connect.
        assert!(m[10 / 2 + 10 * 9] > 0 || row_filled(&m, 10, 9) == 10 || row_filled(&m, 10, 10) == 10);
        let mid = (0..20).find(|&y| row_filled(&m, 10, y) == 10);
        assert!(mid.is_some(), "no row spans the full width");
    }

    #[test]
    fn vertical_line_spans_the_full_height() {
        let m = mask('│');
        let mid = (0..10).find(|&x| col_filled(&m, 10, 20, x) == 20);
        assert!(mid.is_some(), "no column spans the full height");
    }

    #[test]
    fn corner_reaches_exactly_two_edges() {
        // ┌ must touch the right and bottom edges, and neither top nor left.
        let m = mask('┌');
        let touches_top = row_filled(&m, 10, 0) > 0;
        let touches_bottom = row_filled(&m, 10, 19) > 0;
        let touches_left = col_filled(&m, 10, 20, 0) > 0;
        let touches_right = col_filled(&m, 10, 20, 9) > 0;
        assert!(!touches_top && touches_bottom && !touches_left && touches_right);
    }

    #[test]
    fn cross_reaches_all_four_edges() {
        let m = mask('┼');
        assert!(row_filled(&m, 10, 0) > 0 && row_filled(&m, 10, 19) > 0);
        assert!(col_filled(&m, 10, 20, 0) > 0 && col_filled(&m, 10, 20, 9) > 0);
    }

    #[test]
    fn heavy_is_thicker_than_light() {
        let light: usize = mask('│').iter().filter(|&&v| v > 0).count();
        let heavy: usize = mask('┃').iter().filter(|&&v| v > 0).count();
        assert!(heavy > light, "heavy {heavy} should out-ink light {light}");
    }

    #[test]
    fn full_block_fills_every_pixel() {
        assert!(mask('█').iter().all(|&v| v == 255));
    }

    #[test]
    fn lower_half_block_fills_the_bottom_only() {
        let m = mask('▄');
        assert!(row_filled(&m, 10, 5) == 0, "top must be empty");
        assert!(row_filled(&m, 10, 15) == 10, "bottom must be solid");
    }

    #[test]
    fn double_lines_have_two_rails() {
        // ═ must show exactly two separated horizontal rails, not one thick band.
        let m = mask('═');
        let runs = (0..20)
            .map(|y| row_filled(&m, 10, y) == 10)
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|w| !w[0] && w[1])
            .count();
        assert_eq!(runs, 2, "═ should be two distinct rails");
    }

    #[test]
    fn double_corner_reaches_exactly_two_edges() {
        // ╔ touches right and bottom only, and does so with both rails.
        let m = mask('╔');
        assert!(row_filled(&m, 10, 0) == 0, "must not touch the top edge");
        assert!(col_filled(&m, 10, 20, 0) == 0, "must not touch the left edge");
        assert!(col_filled(&m, 10, 20, 9) == 2, "two rails must reach the right edge");
        assert!(row_filled(&m, 10, 19) == 2, "two rails must reach the bottom edge");
    }

    #[test]
    fn double_cross_leaves_the_centre_open() {
        // ╬ is four corners: the very centre stays empty.
        let m = mask('╬');
        assert_eq!(m[10 * 10 + 5], 0, "the middle of ╬ must be a hole");
        assert!(col_filled(&m, 10, 20, 0) == 2 && col_filled(&m, 10, 20, 9) == 2);
        assert!(row_filled(&m, 10, 0) == 2 && row_filled(&m, 10, 19) == 2);
    }

    #[test]
    fn double_tee_keeps_the_through_rail_intact() {
        // ╠ : the left rail runs the full height, uninterrupted.
        let m = mask('╠');
        let full = (0..10).any(|x| col_filled(&m, 10, 20, x) == 20);
        assert!(full, "╠ must keep one rail running the whole height");
    }

    #[test]
    fn shades_are_graded() {
        let light = draw('░', 4, 4, 1.0).unwrap()[0];
        let medium = draw('▒', 4, 4, 1.0).unwrap()[0];
        let dark = draw('▓', 4, 4, 1.0).unwrap()[0];
        assert!(light < medium && medium < dark);
    }
}
