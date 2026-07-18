//! Bundled colour themes for the theme picker.
//!
//! Each entry is a fully-specified [`Theme`]: background, foreground, cursor,
//! selection, accent, dim, and the complete 16-colour ANSI palette. The picker
//! (`Overlay::Theme`) lets you browse these with live preview, and picking one
//! writes it into the config exactly as if you had typed the colours by hand.
//!
//! Palettes are transcribed from each project's published colour scheme. They are
//! plain data — no runtime cost until the picker asks for them.

use crate::config::{Rgb, Theme};

/// Packs an `0xrrggbb` literal into an [`Rgb`], so a palette reads as a column of
/// hex values rather than a wall of `Rgb(_, _, _)` triples.
const fn c(hex: u32) -> Rgb {
    Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Builds a [`Theme`] from the parts every builtin specifies. Keeping this one
/// constructor means a new theme is one row of hex, and every builtin is
/// guaranteed to carry all 16 ANSI colours.
fn theme(
    background: u32,
    foreground: u32,
    cursor: u32,
    selection: u32,
    accent: u32,
    dim: u32,
    ansi: [u32; 16],
) -> Theme {
    Theme {
        background: c(background),
        foreground: c(foreground),
        cursor: c(cursor),
        selection: c(selection),
        accent: c(accent),
        dim: c(dim),
        ansi: ansi.iter().map(|&h| c(h)).collect(),
    }
}

/// The bundled themes, as `(display name, theme)`, in menu order. Names are unique
/// so the picker can filter on them unambiguously.
pub fn builtins() -> Vec<(&'static str, Theme)> {
    vec![
        (
            "Dracula",
            theme(
                0x282a36, 0xf8f8f2, 0xf8f8f2, 0x44475a, 0xbd93f9, 0x6272a4,
                [
                    0x21222c, 0xff5555, 0x50fa7b, 0xf1fa8c, 0xbd93f9, 0xff79c6, 0x8be9fd, 0xf8f8f2,
                    0x6272a4, 0xff6e6e, 0x69ff94, 0xffffa5, 0xd6acff, 0xff92df, 0xa4ffff, 0xffffff,
                ],
            ),
        ),
        (
            "Nord",
            theme(
                0x2e3440, 0xd8dee9, 0xd8dee9, 0x434c5e, 0x88c0d0, 0x4c566a,
                [
                    0x3b4252, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x88c0d0, 0xe5e9f0,
                    0x4c566a, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x8fbcbb, 0xeceff4,
                ],
            ),
        ),
        (
            "Gruvbox Dark",
            theme(
                0x282828, 0xebdbb2, 0xebdbb2, 0x504945, 0xfabd2f, 0x928374,
                [
                    0x282828, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0xa89984,
                    0x928374, 0xfb4934, 0xb8bb26, 0xfabd2f, 0x83a598, 0xd3869b, 0x8ec07c, 0xebdbb2,
                ],
            ),
        ),
        (
            "Gruvbox Light",
            theme(
                0xfbf1c7, 0x3c3836, 0x3c3836, 0xd5c4a1, 0xd79921, 0x928374,
                [
                    0xfbf1c7, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0x7c6f64,
                    0x928374, 0x9d0006, 0x79740e, 0xb57614, 0x076678, 0x8f3f71, 0x427b58, 0x3c3836,
                ],
            ),
        ),
        (
            "Solarized Dark",
            theme(
                0x002b36, 0x839496, 0x93a1a1, 0x073642, 0x268bd2, 0x586e75,
                [
                    0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5,
                    0x002b36, 0xcb4b16, 0x586e75, 0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3,
                ],
            ),
        ),
        (
            "Solarized Light",
            theme(
                0xfdf6e3, 0x657b83, 0x586e75, 0xeee8d5, 0x268bd2, 0x93a1a1,
                [
                    0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5,
                    0x002b36, 0xcb4b16, 0x586e75, 0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3,
                ],
            ),
        ),
        (
            "Catppuccin Mocha",
            theme(
                0x1e1e2e, 0xcdd6f4, 0xf5e0dc, 0x585b70, 0xcba6f7, 0x585b70,
                [
                    0x45475a, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xbac2de,
                    0x585b70, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xa6adc8,
                ],
            ),
        ),
        (
            "Catppuccin Latte",
            theme(
                0xeff1f5, 0x4c4f69, 0xdc8a78, 0xacb0be, 0x8839ef, 0x6c6f85,
                [
                    0x5c5f77, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x1e66f5, 0xea76cb, 0x179299, 0xacb0be,
                    0x6c6f85, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x1e66f5, 0xea76cb, 0x179299, 0xbcc0cc,
                ],
            ),
        ),
        (
            "Tokyo Night",
            theme(
                0x1a1b26, 0xc0caf5, 0xc0caf5, 0x33467c, 0x7aa2f7, 0x414868,
                [
                    0x15161e, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xa9b1d6,
                    0x414868, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xc0caf5,
                ],
            ),
        ),
        (
            "Tokyo Night Storm",
            theme(
                0x24283b, 0xc0caf5, 0xc0caf5, 0x364a82, 0x7aa2f7, 0x414868,
                [
                    0x1d202f, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xa9b1d6,
                    0x414868, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xc0caf5,
                ],
            ),
        ),
        (
            "One Dark",
            theme(
                0x282c34, 0xabb2bf, 0x528bff, 0x3e4451, 0x61afef, 0x5c6370,
                [
                    0x282c34, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xabb2bf,
                    0x5c6370, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xffffff,
                ],
            ),
        ),
        (
            "One Light",
            theme(
                0xfafafa, 0x383a42, 0x526fff, 0xe5e5e6, 0x4078f2, 0xa0a1a7,
                [
                    0x383a42, 0xe45649, 0x50a14f, 0xc18401, 0x4078f2, 0xa626a4, 0x0184bc, 0xa0a1a7,
                    0x696c77, 0xe45649, 0x50a14f, 0xc18401, 0x4078f2, 0xa626a4, 0x0184bc, 0x383a42,
                ],
            ),
        ),
        (
            "Monokai",
            theme(
                0x272822, 0xf8f8f2, 0xf8f8f0, 0x49483e, 0xa6e22e, 0x75715e,
                [
                    0x272822, 0xf92672, 0xa6e22e, 0xf4bf75, 0x66d9ef, 0xae81ff, 0xa1efe4, 0xf8f8f2,
                    0x75715e, 0xf92672, 0xa6e22e, 0xf4bf75, 0x66d9ef, 0xae81ff, 0xa1efe4, 0xf9f8f5,
                ],
            ),
        ),
        (
            "Monokai Pro",
            theme(
                0x2d2a2e, 0xfcfcfa, 0xfcfcfa, 0x5b595c, 0xffd866, 0x727072,
                [
                    0x2d2a2e, 0xff6188, 0xa9dc76, 0xffd866, 0xfc9867, 0xab9df2, 0x78dce8, 0xfcfcfa,
                    0x727072, 0xff6188, 0xa9dc76, 0xffd866, 0xfc9867, 0xab9df2, 0x78dce8, 0xfcfcfa,
                ],
            ),
        ),
        (
            "Everforest Dark",
            theme(
                0x2d353b, 0xd3c6aa, 0xd3c6aa, 0x475258, 0xa7c080, 0x5c6a72,
                [
                    0x475258, 0xe67e80, 0xa7c080, 0xdbbc7f, 0x7fbbb3, 0xd699b6, 0x83c092, 0xd3c6aa,
                    0x5c6a72, 0xe67e80, 0xa7c080, 0xdbbc7f, 0x7fbbb3, 0xd699b6, 0x83c092, 0xd3c6aa,
                ],
            ),
        ),
        (
            "Everforest Light",
            theme(
                0xfdf6e3, 0x5c6a72, 0x5c6a72, 0xedeada, 0x8da101, 0x939f91,
                [
                    0x5c6a72, 0xf85552, 0x8da101, 0xdfa000, 0x3a94c5, 0xdf69ba, 0x35a77c, 0xfdf6e3,
                    0x939f91, 0xf85552, 0x8da101, 0xdfa000, 0x3a94c5, 0xdf69ba, 0x35a77c, 0xfdf6e3,
                ],
            ),
        ),
        (
            "Rose Pine",
            theme(
                0x191724, 0xe0def4, 0xe0def4, 0x403d52, 0xc4a7e7, 0x6e6a86,
                [
                    0x26233a, 0xeb6f92, 0x31748f, 0xf6c177, 0x9ccfd8, 0xc4a7e7, 0xebbcba, 0xe0def4,
                    0x6e6a86, 0xeb6f92, 0x31748f, 0xf6c177, 0x9ccfd8, 0xc4a7e7, 0xebbcba, 0xe0def4,
                ],
            ),
        ),
        (
            "Rose Pine Moon",
            theme(
                0x232136, 0xe0def4, 0xe0def4, 0x44415a, 0xc4a7e7, 0x6e6a86,
                [
                    0x393552, 0xeb6f92, 0x3e8fb0, 0xf6c177, 0x9ccfd8, 0xc4a7e7, 0xea9a97, 0xe0def4,
                    0x6e6a86, 0xeb6f92, 0x3e8fb0, 0xf6c177, 0x9ccfd8, 0xc4a7e7, 0xea9a97, 0xe0def4,
                ],
            ),
        ),
        (
            "Rose Pine Dawn",
            theme(
                0xfaf4ed, 0x575279, 0x575279, 0xdfdad9, 0x907aa9, 0x9893a5,
                [
                    0xf2e9e1, 0xb4637a, 0x286983, 0xea9d34, 0x56949f, 0x907aa9, 0xd7827e, 0x575279,
                    0x9893a5, 0xb4637a, 0x286983, 0xea9d34, 0x56949f, 0x907aa9, 0xd7827e, 0x575279,
                ],
            ),
        ),
        (
            "Ayu Dark",
            theme(
                0x0a0e14, 0xb3b1ad, 0xe6b450, 0x273747, 0xe6b450, 0x686868,
                [
                    0x01060e, 0xea6c73, 0x91b362, 0xf9af4f, 0x53bdfa, 0xfae994, 0x90e1c6, 0xc7c7c7,
                    0x686868, 0xf07178, 0xc2d94c, 0xffb454, 0x59c2ff, 0xffee99, 0x95e6cb, 0xffffff,
                ],
            ),
        ),
        (
            "Ayu Mirage",
            theme(
                0x1f2430, 0xcbccc6, 0xffcc66, 0x34455a, 0xffcc66, 0x686868,
                [
                    0x191e2a, 0xed8274, 0xa6cc70, 0xfad07b, 0x6dcbfa, 0xcfbafa, 0x90e1c6, 0xc7c7c7,
                    0x686868, 0xf28779, 0xd5ff80, 0xffd580, 0x73d0ff, 0xdfbfff, 0x95e6cb, 0xffffff,
                ],
            ),
        ),
        (
            "Ayu Light",
            theme(
                0xfafafa, 0x5c6166, 0xff9940, 0xd1e4f4, 0xff9940, 0x828c99,
                [
                    0x000000, 0xf07171, 0x86b300, 0xf2ae49, 0x399ee6, 0xa37acc, 0x4cbf99, 0x6b7680,
                    0x686868, 0xf07171, 0x86b300, 0xf2ae49, 0x399ee6, 0xa37acc, 0x4cbf99, 0xd1d1d1,
                ],
            ),
        ),
        (
            "GitHub Dark",
            theme(
                0x0d1117, 0xc9d1d9, 0x58a6ff, 0x264f78, 0x58a6ff, 0x6e7681,
                [
                    0x484f58, 0xff7b72, 0x3fb950, 0xd29922, 0x58a6ff, 0xbc8cff, 0x39c5cf, 0xb1bac4,
                    0x6e7681, 0xffa198, 0x56d364, 0xe3b341, 0x79c0ff, 0xd2a8ff, 0x56d4dd, 0xffffff,
                ],
            ),
        ),
        (
            "GitHub Light",
            theme(
                0xffffff, 0x24292f, 0x0969da, 0xb6d5f5, 0x0969da, 0x57606a,
                [
                    0x24292f, 0xcf222e, 0x116329, 0x4d2d00, 0x0969da, 0x8250df, 0x1b7c83, 0x6e7781,
                    0x57606a, 0xa40e26, 0x1a7f37, 0x633c01, 0x218bff, 0xa475f9, 0x3192aa, 0x8c959f,
                ],
            ),
        ),
        (
            "Kanagawa",
            theme(
                0x1f1f28, 0xdcd7ba, 0xc8c093, 0x2d4f67, 0x7e9cd8, 0x727169,
                [
                    0x090618, 0xc34043, 0x76946a, 0xc0a36e, 0x7e9cd8, 0x957fb8, 0x6a9589, 0xc8c093,
                    0x727169, 0xe82424, 0x98bb6c, 0xe6c384, 0x7fb4ca, 0x938aa9, 0x7aa89f, 0xdcd7ba,
                ],
            ),
        ),
        (
            "Nightfox",
            theme(
                0x192330, 0xcdcecf, 0xcdcecf, 0x2b3b51, 0x719cd6, 0x575860,
                [
                    0x393b44, 0xc94f6d, 0x81b29a, 0xdbc074, 0x719cd6, 0x9d79d6, 0x63cdcf, 0xdfdfe0,
                    0x575860, 0xd16983, 0x8ebaa4, 0xe0c989, 0x86abdc, 0xbaa1e2, 0x7ad5d6, 0xe4e4e5,
                ],
            ),
        ),
        (
            "Zenburn",
            theme(
                0x3f3f3f, 0xdcdccc, 0xdcdccc, 0x545454, 0xf0dfaf, 0x709080,
                [
                    0x3f3f3f, 0xcc9393, 0x7f9f7f, 0xe3ceab, 0x8cd0d3, 0xdc8cc3, 0x93e0e3, 0xdcdccc,
                    0x709080, 0xdca3a3, 0xbfebbf, 0xf0dfaf, 0x94bff3, 0xec93d3, 0x93e0e3, 0xffffff,
                ],
            ),
        ),
        (
            "Material Dark",
            theme(
                0x263238, 0xeeffff, 0xffcc00, 0x314549, 0x82aaff, 0x546e7a,
                [
                    0x263238, 0xf07178, 0xc3e88d, 0xffcb6b, 0x82aaff, 0xc792ea, 0x89ddff, 0xeeffff,
                    0x546e7a, 0xf07178, 0xc3e88d, 0xffcb6b, 0x82aaff, 0xc792ea, 0x89ddff, 0xffffff,
                ],
            ),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn builtins_are_plentiful_distinct_and_complete() {
        let all = builtins();
        assert!(all.len() >= 20, "expected at least 20 builtin themes, got {}", all.len());

        // Names must be unique so the picker can filter on them without ambiguity.
        let names: HashSet<&str> = all.iter().map(|(n, _)| *n).collect();
        assert_eq!(names.len(), all.len(), "theme names must be distinct");

        for (name, t) in &all {
            assert_eq!(t.ansi.len(), 16, "{name} must specify all 16 ANSI colours");
            // A theme whose text is its background is unreadable — a transcription slip.
            assert_ne!(t.foreground, t.background, "{name} foreground equals background");
        }
    }

    #[test]
    fn builtins_survive_a_json_round_trip() {
        // The picker persists a chosen theme into the JSON config, so every builtin
        // must serialise and parse back to the same colours.
        for (name, t) in builtins() {
            let json = serde_json::to_string(&t).unwrap();
            let back: Theme = serde_json::from_str(&json).unwrap();
            assert_eq!(back.ansi.len(), 16, "{name} lost colours through JSON");
            assert_eq!(back.background, t.background, "{name} background drifted");
            assert_eq!(back.accent, t.accent, "{name} accent drifted");
        }
    }
}
