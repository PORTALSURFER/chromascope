# Chromascope visual system

Chromascope uses the same visual language as Pump so the two devices feel like
parts of one product family. The values below mirror Pump's
`src/gui/visual_system.rs` and `docs/visual-system.md` rather than introducing
a second blue/cyan theme.

## Palette

All values are RGBA unless stated otherwise.

| Semantic role | Value |
| --- | --- |
| canvas and surfaces | `#1B1E1EFF` |
| selected/raised overlay | `#2A2D2DFF` |
| border | `#3A3D3DFF` |
| emphasized border/divider | `#404342FF` |
| strong grid | `#363939FF` |
| soft grid/recessed track | `#282B2BFF` |
| primary text | `#D8D7D3FF` |
| muted text | `#999B9AFF` |
| primary coral / main trace | `#E95843FF` |
| secondary coral / live emphasis | `#F16C56FF` |
| warming | `#D9975FFF` |
| danger/hot | `#EF4C3DFF` |
| disabled fill | `#242829FF` |

The main spectrum is always the primary coral. Companion traces deliberately
keep their stable per-source colors and use the shared charcoal surface as the
common visual frame; collapsing them to one Pump accent would make multiselect
overlays ambiguous.

## Composition

The native editor uses Pump's compact geometry roles: 3.4/6.8/10.2/13.6
spacing, 10.2 px surface padding, 6.8 px panel radius, one-pixel borders, and
27.2 px source-row/control height. Text uses Pump's mono hierarchy of
18.7/23.8 brand, 11.9/15.3 body, 10.2/13.6 value, 8.5/13.6 control label,
and 8/11.9 metadata size/line-height roles. The source list stays virtualized
and scrollable; styling does not increase analysis work for registered
companions.

The declarative fallback receives the same semantic surface, border, text, and
primary-accent roles through its root `ThemeTokens`. Its integer spacing and
control tokens are the nearest Patchbay representation of the native values.
