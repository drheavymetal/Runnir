# Fonts bundled with runnir

runnir embeds these so a fresh machine renders correctly with nothing installed.
They are a LAST RESORT: if the same family is present on the system, the system's
copy is used and none of this is touched.

All of it is redistributable. This file exists because "redistributable" is not the
same as "one licence", and the Nerd Font is not one licence.

## NotoSansRunic-Regular.ttf

Elder Futhark (U+16A0..U+16FF), for the map's rune rain.

- Copyright: The Noto Project Authors
- Licence: **SIL Open Font License 1.1** (`OFL-1.1-no-RFN`)
- Source: https://github.com/notofonts/noto-fonts

Bundled because it has to be: no monospace face on a normal system carries the Runic
block, so without this the rain draws as empty boxes rather than runes. 9.6 KB.

## JetBrainsMonoNerdFontMono-{Regular,Bold,Italic,BoldItalic}.ttf

runnir's default family. These are JetBrains Mono **patched by Nerd Fonts**, which is
two things in one file and therefore two sets of rights.

### The outlines

- Copyright: JetBrains s.r.o.
- Licence: **SIL Open Font License 1.1** (`OFL-1.1-no-RFN`) — see `OFL-JetBrainsMono.txt`
- Source: https://github.com/JetBrains/JetBrainsMono

### The patched-in icon glyphs

Nerd Fonts merges several icon sets into the private use area. The patcher itself is
MIT (https://github.com/ryanoasis/nerd-fonts); the glyph sets keep their own terms,
and these are the ones that ship inside these files:

| Set | Licence |
| --- | --- |
| Font Awesome | CC BY 4.0 |
| Devicons | MIT |
| Octicons | MIT |
| Material Design Icons | Apache 2.0 / OFL 1.1 |
| Powerline Symbols | MIT |
| Weather Icons | SIL OFL 1.1 |
| Codicons | CC BY 4.0 |

CC BY 4.0 requires attribution, which is what this table is for. None of these
licences restricts bundling in software, commercial or otherwise; what they require is
that the notice travels with the files. It does, here.

## Why bundle at all

Without the runic face the map's rain is boxes — broken, not merely different. The
default family is a weaker case: runnir already falls back to whatever monospace a
system has, so text renders either way. It is bundled so that a machine that just ran
`install.sh` looks the way the terminal was designed to look, rather than the way its
distro's default monospace looks.
