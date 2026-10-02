# Synapto Brand Assets

This directory contains the official vector brand assets for Synapto.

## Asset Files

| File | Type | Dimensions / ViewBox | Primary Usage |
| :--- | :--- | :--- | :--- |
| `synapto.svg` | Vector Icon | `415.42 × 424.18` | Standalone icon, avatars, square badge graphics |
| `synapto-text.svg` | Full Logo | `1840 × 424.18` | High-resolution print, marketing banners, presentations |
| `synapto-text-100.svg` | Documentation Logo | `575 × 131.40` | GitHub README header, website banners, docs |

## Typography Specification

- **Font Family**: Noto Serif
- **Weight**: Regular (400)
- **License**: SIL Open Font License 1.1
- **Color**: `#3470a3`

### Path Outline Invariant

All text glyphs in `synapto-text.svg` and `synapto-text-100.svg` are converted to vector `<path>` outlines. Text must not be stored as raw `<text>` elements. Storing text as vector outlines guarantees:

1. Deterministic visual rendering across macOS, Linux, Windows, mobile browsers, and GitHub image proxies.
2. Complete elimination of font substitution discrepancies and canvas clipping defects.
