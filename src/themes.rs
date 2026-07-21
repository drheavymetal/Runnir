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
        // Transcribed from the kitty palette DankMaterialShell generates for its
        // synthwaveElectric theme: near-black blue ground, electric-blue selection,
        // an orange ramp for the accent side. `color0` deliberately equals the
        // background, as it does in the source.
        (
            "Synthwave Electric",
            theme(
                0x0a0a15, 0xe6f0ff, 0xe6f0ff, 0x0080ff, 0xff6600, 0xa5968c,
                [
                    0x0a0a15, 0xff0c00, 0x2dff00, 0xffd100, 0xf26000, 0x762f00, 0xff6600, 0xffece0,
                    0xa5968c, 0xff483f, 0x6cff4c, 0xffdf4c, 0xff7c26, 0xff934c, 0xffba8c, 0xfff7f2,
                ],
            ),
        ),
        // The rest are transcribed from mbadolato/iTerm2-Color-Schemes, which keeps a
        // kitty `.conf` per scheme straight from each project's published palette.
        // Two fields cannot come from there: those ports set `selection_background`
        // to the foreground (kitty renders a selection as reverse video, runnir as a
        // background), and they carry no accent at all. Both are taken from each
        // theme's own published values instead — `colour8` is a poor stand-in, since
        // in several of these it is either as bright as the text or as dark as the
        // background.
        (
            "Catppuccin Frappe",
            theme(
                0x303446, 0xc6d0f5, 0xf2d5cf, 0x626880, 0xca9ee6, 0x838ba7,
                [
                    0x51576d, 0xe78284, 0xa6d189, 0xe5c890, 0x8caaee, 0xf4b8e4, 0x81c8be, 0xb5bfe2,
                    0x626880, 0xeda0a2, 0xb9dba2, 0xecd7ae, 0xadc2f3, 0xf38ed8, 0x98d2ca, 0xa5adce,
                ],
            ),
        ),
        (
            "Catppuccin Macchiato",
            theme(
                0x24273a, 0xcad3f5, 0xf4dbd6, 0x5b6078, 0xc6a0f6, 0x8087a2,
                [
                    0x494d64, 0xed8796, 0xa6da95, 0xeed49f, 0x8aadf4, 0xf5bde6, 0x8bd5ca, 0xb8c0e0,
                    0x5b6078, 0xf2a7b2, 0xbde3b0, 0xf4e3c1, 0xadc5f7, 0xf493da, 0xa5ded6, 0xa5adcb,
                ],
            ),
        ),
        (
            "Tokyo Night Moon",
            theme(
                0x222436, 0xc8d3f5, 0xc8d3f5, 0x2d3f76, 0x82aaff, 0x444a73,
                [
                    0x1b1d2b, 0xff757f, 0xc3e88d, 0xffc777, 0x82aaff, 0xc099ff, 0x86e1fc, 0x828bb8,
                    0x444a73, 0xff757f, 0xc3e88d, 0xffc777, 0x82aaff, 0xc099ff, 0x86e1fc, 0xc8d3f5,
                ],
            ),
        ),
        (
            "Tokyo Night Day",
            theme(
                0xe1e2e7, 0x3760bf, 0x3760bf, 0xb7c1e3, 0x2e7de9, 0xa1a6c5,
                [
                    0xe9e9ed, 0xf52a65, 0x587539, 0x8c6c3e, 0x2e7de9, 0x9854f1, 0x007197, 0x6172b0,
                    0xa1a6c5, 0xf52a65, 0x587539, 0x8c6c3e, 0x2e7de9, 0x9854f1, 0x007197, 0x3760bf,
                ],
            ),
        ),
        (
            "Kanagawa Dragon",
            theme(
                0x181616, 0xc5c9c5, 0xc8c093, 0x2d4f67, 0x8ba4b0, 0xa6a69c,
                [
                    0x0d0c0c, 0xc4746e, 0x8a9a7b, 0xc4b28a, 0x8ba4b0, 0xa292a3, 0x8ea4a2, 0xc8c093,
                    0xa6a69c, 0xe46876, 0x87a987, 0xe6c384, 0x7fb4ca, 0x938aa9, 0x7aa89f, 0xc5c9c5,
                ],
            ),
        ),
        (
            "Kanagawa Lotus",
            theme(
                0xf2ecbc, 0x545464, 0x43436c, 0xc9cbd1, 0x4d699b, 0x8a8980,
                [
                    0x1f1f28, 0xc84053, 0x6f894e, 0x77713f, 0x4d699b, 0xb35b79, 0x597b75, 0x545464,
                    0x8a8980, 0xd7474b, 0x6e915f, 0x836f4a, 0x6693bf, 0x624c83, 0x5e857a, 0x43436c,
                ],
            ),
        ),
        (
            "Gruvbox Material Dark",
            theme(
                0x282828, 0xd4be98, 0xd4be98, 0x45403d, 0xd8a657, 0x7c6f64,
                [
                    0x282828, 0xea6962, 0xa9b665, 0xd8a657, 0x7daea3, 0xd3869b, 0x89b482, 0xd4be98,
                    0x7c6f64, 0xea6962, 0xa9b665, 0xd8a657, 0x7daea3, 0xd3869b, 0x89b482, 0xddc7a1,
                ],
            ),
        ),
        (
            "Gruvbox Material Light",
            theme(
                0xfbf1c7, 0x654735, 0x654735, 0xeee0b7, 0xb47109, 0xa89984,
                [
                    0xfbf1c7, 0xc14a4a, 0x6c782e, 0xb47109, 0x45707a, 0x945e80, 0x4c7a5d, 0x654735,
                    0xa89984, 0xc14a4a, 0x6c782e, 0xb47109, 0x45707a, 0x945e80, 0x4c7a5d, 0x4f3829,
                ],
            ),
        ),
        (
            "Carbonfox",
            theme(
                0x161616, 0xf2f4f8, 0xf2f4f8, 0x393939, 0x78a9ff, 0x484848,
                [
                    0x282828, 0xee5396, 0x25be6a, 0x08bdba, 0x78a9ff, 0xbe95ff, 0x33b1ff, 0xdfdfe0,
                    0x484848, 0xf16da6, 0x46c880, 0x2dc7c4, 0x8cb6ff, 0xc8a5ff, 0x52bdff, 0xe4e4e5,
                ],
            ),
        ),
        (
            "Duskfox",
            theme(
                0x232136, 0xe0def4, 0xe0def4, 0x433c59, 0xc4a7e7, 0x544d8a,
                [
                    0x393552, 0xeb6f92, 0xa3be8c, 0xf6c177, 0x569fba, 0xc4a7e7, 0x9ccfd8, 0xe0def4,
                    0x544d8a, 0xf083a2, 0xb1d196, 0xf9cb8c, 0x65b1cd, 0xccb1ed, 0xa6dae3, 0xe2e0f7,
                ],
            ),
        ),
        (
            "Terafox",
            theme(
                0x152528, 0xe6eaea, 0xe6eaea, 0x293e40, 0x5a93aa, 0x4e5157,
                [
                    0x2f3239, 0xe85c51, 0x7aa4a1, 0xfda47f, 0x5a93aa, 0xad5c7c, 0xa1cdd8, 0xebebeb,
                    0x4e5157, 0xeb746b, 0x8eb2af, 0xfdb292, 0x73a3b7, 0xb97490, 0xafd4de, 0xeeeeee,
                ],
            ),
        ),
        (
            "Dawnfox",
            theme(
                0xfaf4ed, 0x575279, 0x575279, 0xdfdad9, 0x286983, 0x9893a5,
                [
                    0x575279, 0xb4637a, 0x618774, 0xea9d34, 0x286983, 0x907aa9, 0x56949f, 0xb2b6bd,
                    0x5f5695, 0xc26d85, 0x629f81, 0xeea846, 0x2d81a3, 0x9a80b9, 0x5ca7b4, 0xe6ebf3,
                ],
            ),
        ),
        (
            "Nordfox",
            theme(
                0x2e3440, 0xcdcecf, 0xcdcecf, 0x3b4252, 0x88c0d0, 0x4c566a,
                [
                    0x3b4252, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x88c0d0, 0xe5e9f0,
                    0x53648d, 0xd06f79, 0xb1d196, 0xf0d399, 0x8cafd2, 0xc895bf, 0x93ccdc, 0xe7ecf4,
                ],
            ),
        ),
        (
            "GitHub Dark Dimmed",
            theme(
                0x22272e, 0xadbac7, 0x539bf5, 0x264f78, 0x539bf5, 0x636e7b,
                [
                    0x545d68, 0xf47067, 0x57ab5a, 0xc69026, 0x539bf5, 0xb083f0, 0x39c5cf, 0x909dab,
                    0x636e7b, 0xff938a, 0x6bc46d, 0xdaaa3f, 0x6cb6ff, 0xdcbdfb, 0x56d4dd, 0xcdd9e5,
                ],
            ),
        ),
        (
            "Oxocarbon",
            theme(
                0x161616, 0xf2f4f8, 0xffffff, 0x393939, 0x00b4ff, 0x585858,
                [
                    0x161616, 0x00dfdb, 0x00b4ff, 0xff4297, 0x00c15a, 0xc693ff, 0xff74b8, 0xf2f4f8,
                    0x585858, 0x00dfdb, 0x00b4ff, 0xff4297, 0x00c15a, 0xc693ff, 0xff74b8, 0xf2f4f8,
                ],
            ),
        ),
        (
            "Poimandres",
            theme(
                0x1a1e28, 0xa6accd, 0xffffff, 0x303340, 0x5de4c7, 0x767c9d,
                [
                    0x1a1e28, 0xd0679d, 0x5de4c7, 0xfffac2, 0x89ddff, 0xfcc5e9, 0xadd7ff, 0xffffff,
                    0xa6accd, 0xd0679d, 0x5de4c7, 0xfffac2, 0xadd7ff, 0xfae4fc, 0x89ddff, 0xffffff,
                ],
            ),
        ),
        (
            "Vesper",
            theme(
                0x101010, 0xffffff, 0xacb1ab, 0x2a2a2a, 0xffc799, 0x7e7e7e,
                [
                    0x101010, 0xf5a191, 0x90b99f, 0xe6b99d, 0xaca1cf, 0xe29eca, 0xea83a5, 0xa0a0a0,
                    0x7e7e7e, 0xff8080, 0x99ffe4, 0xffc799, 0xb9aeda, 0xecaad6, 0xf591b2, 0xffffff,
                ],
            ),
        ),
        (
            "Flexoki Dark",
            theme(
                0x100f0f, 0xcecdc3, 0xcecdc3, 0x282726, 0x4385be, 0x878580,
                [
                    0x100f0f, 0xd14d41, 0x879a39, 0xd0a215, 0x4385be, 0xce5d97, 0x3aa99f, 0x878580,
                    0x575653, 0xaf3029, 0x66800b, 0xad8301, 0x205ea6, 0xa02f6f, 0x24837b, 0xcecdc3,
                ],
            ),
        ),
        (
            "Flexoki Light",
            theme(
                0xfffcf0, 0x100f0f, 0x100f0f, 0xe6e4d9, 0x205ea6, 0x6f6e69,
                [
                    0x100f0f, 0xaf3029, 0x66800b, 0xad8301, 0x205ea6, 0xa02f6f, 0x24837b, 0x6f6e69,
                    0xb7b5ac, 0xd14d41, 0x879a39, 0xd0a215, 0x4385be, 0xce5d97, 0x3aa99f, 0xcecdc3,
                ],
            ),
        ),
        (
            "Melange Dark",
            theme(
                0x292522, 0xece1d7, 0xece1d7, 0x403a36, 0xe49b5d, 0x867462,
                [
                    0x34302c, 0xbd8183, 0x78997a, 0xe49b5d, 0x7f91b2, 0xb380b0, 0x7b9695, 0xc1a78e,
                    0x867462, 0xd47766, 0x85b695, 0xebc06d, 0xa3a9ce, 0xcf9bc2, 0x89b3b6, 0xece1d7,
                ],
            ),
        ),
        (
            "Melange Light",
            theme(
                0xf1f1f1, 0x54433a, 0x54433a, 0xe0d9d4, 0xbc5c00, 0xa98a78,
                [
                    0xe9e1db, 0xc77b8b, 0x6e9b72, 0xbc5c00, 0x7892bd, 0xbe79bb, 0x739797, 0x7d6658,
                    0xa98a78, 0xbf0021, 0x3a684a, 0xa06d00, 0x465aa4, 0x904180, 0x3d6568, 0x54433a,
                ],
            ),
        ),
        (
            "Modus Vivendi",
            theme(
                0x000000, 0xffffff, 0xffffff, 0x3c3c3c, 0x2fafff, 0x595959,
                [
                    0x000000, 0xff5f59, 0x44bc44, 0xd0bc00, 0x2fafff, 0xfeacd0, 0x00d3d0, 0xa6a6a6,
                    0x595959, 0xff7f9f, 0x00c06f, 0xfec43f, 0x79a8ff, 0xb6a0ff, 0x6ae4b9, 0xffffff,
                ],
            ),
        ),
        (
            "Modus Operandi",
            theme(
                0xffffff, 0x000000, 0x000000, 0xbcbcbc, 0x0031a9, 0x595959,
                [
                    0x000000, 0xa60000, 0x006800, 0x6f5500, 0x0031a9, 0x721045, 0x005e8b, 0xa6a6a6,
                    0x595959, 0x972500, 0x00663f, 0x884900, 0x3548cf, 0x531ab6, 0x005f5f, 0x595959,
                ],
            ),
        ),
        (
            "Iceberg Dark",
            theme(
                0x161821, 0xc6c8d1, 0xc6c8d1, 0x272c42, 0x84a0c6, 0x6b7089,
                [
                    0x1e2132, 0xe27878, 0xb4be82, 0xe2a478, 0x84a0c6, 0xa093c7, 0x89b8c2, 0xc6c8d1,
                    0x6b7089, 0xe98989, 0xc0ca8e, 0xe9b189, 0x91acd1, 0xada0d3, 0x95c4ce, 0xd2d4de,
                ],
            ),
        ),
        (
            "Iceberg Light",
            theme(
                0xe8e9ec, 0x33374c, 0x33374c, 0xcad0de, 0x2d539e, 0x8389a3,
                [
                    0xdcdfe7, 0xcc517a, 0x668e3d, 0xc57339, 0x2d539e, 0x7759b4, 0x3f83a6, 0x33374c,
                    0x8389a3, 0xcc3768, 0x598030, 0xb6662d, 0x22478e, 0x6845ad, 0x327698, 0x262a3f,
                ],
            ),
        ),
        (
            "Night Owl",
            theme(
                0x011627, 0xd6deeb, 0x7e57c2, 0x1d3b53, 0x82aaff, 0x637777,
                [
                    0x011627, 0xef5350, 0x22da6e, 0xaddb67, 0x82aaff, 0xc792ea, 0x21c7a8, 0xffffff,
                    0x575656, 0xef5350, 0x22da6e, 0xffeb95, 0x82aaff, 0xc792ea, 0x7fdbca, 0xffffff,
                ],
            ),
        ),
        (
            "Light Owl",
            theme(
                0xfbfbfb, 0x403f53, 0x403f53, 0xd3e8f8, 0x288ed7, 0x989fb1,
                [
                    0x403f53, 0xde3d3b, 0x08916a, 0xe0af02, 0x288ed7, 0xd6438a, 0x2aa298, 0xbdbdbd,
                    0x989fb1, 0xde3d3b, 0x08916a, 0xdaaa01, 0x288ed7, 0xd6438a, 0x2aa298, 0xf0f0f0,
                ],
            ),
        ),
        (
            "Snazzy",
            theme(
                0x1e1f29, 0xebece6, 0xe4e4e4, 0x34353e, 0xfc4cb4, 0x555555,
                [
                    0x000000, 0xfc4346, 0x50fb7c, 0xf0fb8c, 0x49baff, 0xfc4cb4, 0x8be9fe, 0xededec,
                    0x555555, 0xfc4346, 0x50fb7c, 0xf0fb8c, 0x49baff, 0xfc4cb4, 0x8be9fe, 0xededec,
                ],
            ),
        ),
        (
            "Doom One",
            theme(
                0x282c34, 0xbbc2cf, 0x51afef, 0x42444a, 0x51afef, 0x5b6268,
                [
                    0x000000, 0xff6c6b, 0x98be65, 0xecbe7b, 0xa9a1e1, 0xc678dd, 0x51afef, 0xbbc2cf,
                    0x595959, 0xff6655, 0x99bb66, 0xecbe7b, 0xa9a1e1, 0xc678dd, 0x51afef, 0xbfbfbf,
                ],
            ),
        ),
        (
            "Sonokai",
            theme(
                0x2c2e34, 0xe2e2e3, 0xe2e2e3, 0x3b3e48, 0x76cce0, 0x7f8490,
                [
                    0x181819, 0xfc5d7c, 0x9ed072, 0xe7c664, 0x76cce0, 0xb39df3, 0xf39660, 0xe2e2e3,
                    0x7f8490, 0xfc5d7c, 0x9ed072, 0xe7c664, 0x76cce0, 0xb39df3, 0xf39660, 0xe2e2e3,
                ],
            ),
        ),
        (
            "Srcery",
            theme(
                0x1c1b19, 0xfce8c3, 0xfbb829, 0x303030, 0xfbb829, 0x918175,
                [
                    0x1c1b19, 0xef2f27, 0x519f50, 0xfbb829, 0x2c78bf, 0xe02c6d, 0x0aaeb3, 0xbaa67f,
                    0x918175, 0xf75341, 0x98bc37, 0xfed06e, 0x68a8e4, 0xff5c8f, 0x2be4d0, 0xfce8c3,
                ],
            ),
        ),
        (
            "Cobalt2",
            theme(
                0x132738, 0xffffff, 0xf0cc09, 0x0050a4, 0xffc600, 0x555555,
                [
                    0x000000, 0xff0000, 0x38de21, 0xffe50a, 0x1460d2, 0xff005d, 0x00bbbb, 0xbbbbbb,
                    0x555555, 0xf40e17, 0x3bd01d, 0xedc809, 0x5555ff, 0xff55ff, 0x6ae3fa, 0xffffff,
                ],
            ),
        ),
        (
            "Aura Dark",
            theme(
                0x15141b, 0xcdccce, 0xa277ff, 0x29263c, 0xa277ff, 0x6d6d6d,
                [
                    0x15141b, 0xff6767, 0x61ffca, 0xffca85, 0xa277ff, 0x61ffca, 0xa277ff, 0xcdccce,
                    0x464646, 0xffca85, 0xa277ff, 0xffca85, 0xa277ff, 0x61ffca, 0x61ffca, 0xedecee,
                ],
            ),
        ),
        (
            "Shades of Purple",
            theme(
                0x1e1d40, 0xffffff, 0xfad000, 0x3c3a7a, 0xfad000, 0x686868,
                [
                    0x000000, 0xd90429, 0x3ad900, 0xffe700, 0x6943ff, 0xff2c70, 0x00c5c7, 0xc7c7c7,
                    0x686868, 0xf92a1c, 0x43d426, 0xf1d000, 0x6871ff, 0xff77ff, 0x79e8fb, 0xffffff,
                ],
            ),
        ),
        (
            "Xcode Dark",
            theme(
                0x292a30, 0xdfdfe0, 0xdfdfe0, 0x545861, 0x4eb0cc, 0x7f8c98,
                [
                    0x414453, 0xff8170, 0x78c2b3, 0xd9c97c, 0x4eb0cc, 0xff7ab2, 0xb281eb, 0xdfdfe0,
                    0x7f8c98, 0xff8170, 0xacf2e4, 0xffa14f, 0x6bdfff, 0xff7ab2, 0xdabaff, 0xdfdfe0,
                ],
            ),
        ),
        (
            "Xcode Light",
            theme(
                0xffffff, 0x262626, 0x262626, 0xb4d8fd, 0x0f68a0, 0x8a99a6,
                [
                    0xb4d8fd, 0xd12f1b, 0x3e8087, 0x78492a, 0x0f68a0, 0xad3da4, 0x804fb8, 0x262626,
                    0x8a99a6, 0xd12f1b, 0x23575c, 0x78492a, 0x0b4f79, 0xad3da4, 0x4b21b0, 0x262626,
                ],
            ),
        ),
        (
            "Matte Black",
            theme(
                0x121212, 0xbebebe, 0xeaeaea, 0x2a2a2a, 0xffc107, 0x8a8a8d,
                [
                    0x333333, 0xd35f5f, 0xffc107, 0xb91c1c, 0xe68e0d, 0xd35f5f, 0xbebebe, 0xbebebe,
                    0x8a8a8d, 0x891c1c, 0xffc107, 0xb90a0a, 0xf59e0b, 0xb91c1c, 0xeaeaea, 0xffffff,
                ],
            ),
        ),
        (
            "Moonfly",
            theme(
                0x080808, 0xbdbdbd, 0x9e9e9e, 0x323437, 0x80a0ff, 0x949494,
                [
                    0x323437, 0xff5454, 0x8cc85f, 0xe3c78a, 0x80a0ff, 0xcf87e8, 0x79dac8, 0xc6c6c6,
                    0x949494, 0xff5189, 0x36c692, 0xc6c684, 0x74b2ff, 0xae81ff, 0x85dc85, 0xe4e4e4,
                ],
            ),
        ),
        (
            "Miasma",
            theme(
                0x222222, 0xc2c2b0, 0xc7c7c7, 0x3a3a3a, 0xbb7744, 0x666666,
                [
                    0x000000, 0x685742, 0x5f875f, 0xb36d43, 0x78824b, 0xbb7744, 0xc9a554, 0xd7c483,
                    0x666666, 0x685742, 0x5f875f, 0xb36d43, 0x78824b, 0xbb7744, 0xc9a554, 0xd7c483,
                ],
            ),
        ),
        (
            "Vague",
            theme(
                0x141415, 0xcdcdcd, 0xcdcdcd, 0x252530, 0x6e94b2, 0x606079,
                [
                    0x252530, 0xd8647e, 0x7fa563, 0xf3be7c, 0x6e94b2, 0xbb9dbd, 0xaeaed1, 0xcdcdcd,
                    0x606079, 0xe08398, 0x99b782, 0xf5cb96, 0x8ba9c1, 0xc9b1ca, 0xbebeda, 0xd7d7d7,
                ],
            ),
        ),
        (
            "Mellow",
            theme(
                0x161617, 0xc9c7cd, 0xcac9dd, 0x2a2a2c, 0xaca1cf, 0x57575f,
                [
                    0x27272a, 0xf5a191, 0x90b99f, 0xe6b99d, 0xaca1cf, 0xe29eca, 0xea83a5, 0xc1c0d4,
                    0x424246, 0xffae9f, 0x9dc6ac, 0xf0c5a9, 0xb9aeda, 0xecaad6, 0xf591b2, 0xcac9dd,
                ],
            ),
        ),
        (
            "One Half Dark",
            theme(
                0x282c34, 0xdcdfe4, 0xa3b3cc, 0x3e4451, 0x61afef, 0x5d677a,
                [
                    0x282c34, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xdcdfe4,
                    0x5d677a, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xdcdfe4,
                ],
            ),
        ),
        (
            "One Half Light",
            theme(
                0xfafafa, 0x383a42, 0xa5b4e5, 0xbfceff, 0x0184bc, 0xa0a1a7,
                [
                    0x383a42, 0xe45649, 0x50a14f, 0xc18401, 0x0184bc, 0xa626a4, 0x0997b3, 0xbababa,
                    0x4f525e, 0xe06c75, 0x98c379, 0xd8b36e, 0x61afef, 0xc678dd, 0x56b6c2, 0xffffff,
                ],
            ),
        ),
        (
            "Selenized Dark",
            theme(
                0x103c48, 0xadbcbc, 0xadbcbc, 0x184956, 0x4695f7, 0x72898f,
                [
                    0x184956, 0xfa5750, 0x75b938, 0xdbb32d, 0x4695f7, 0xf275be, 0x41c7b9, 0xadbcbc,
                    0x72898f, 0xff665c, 0x84c747, 0xebc13d, 0x58a3ff, 0xff84cd, 0x53d6c7, 0xcad8d9,
                ],
            ),
        ),
        (
            "Selenized Light",
            theme(
                0xfbf3db, 0x53676d, 0x53676d, 0xece3cc, 0x0072d4, 0x909995,
                [
                    0xece3cc, 0xd2212d, 0x489100, 0xad8900, 0x0072d4, 0xca4898, 0x009c8f, 0x53676d,
                    0x909995, 0xcc1729, 0x428b00, 0xa78300, 0x006dce, 0xc44392, 0x00978a, 0x3a4d53,
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
            // The same slip, in the three colours runnir draws its own chrome with: a
            // selection you cannot see, a tab bar the colour of the background, or a
            // status bar whose dim text vanishes.
            assert_ne!(t.selection, t.background, "{name} selection equals background");
            assert_ne!(t.accent, t.background, "{name} accent equals background");
            assert_ne!(t.dim, t.background, "{name} dim equals background");
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
