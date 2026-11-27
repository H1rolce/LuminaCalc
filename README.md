# <img src="lumina-ui/assets/icon.png" alt="LuminaCalc" width="52" align="absmiddle" /> LuminaCalc

**English** | [Русский](README.ru.md)

**LuminaCalc** is a desktop calculator written in Rust (`egui` / `eframe`) for Windows and Linux. It combines four calculation modes in a single window, supports dark and light themes (with automatic system accent color on Windows), and provides an English and Russian interface.

![LuminaCalc Preview](preview.gif)

[![Latest Release](https://img.shields.io/github/v/release/H1rolce/LuminaCalc?label=Download%20Latest%20Release&style=for-the-badge)](https://github.com/H1rolce/LuminaCalc/releases/latest)

---

## Features

- **Notebook (Soulver / Numi style).** Multi-line calculations that update as you type: variables, line references (`#1`, `#2`), block aggregates (`sum`, `avg`), physical unit conversions (`120 km/h in m/s`, `22 °C in °F`), and base conversions (`255 in hex`).
- **Scientific Calculator (Casio ClassWiz / fx-991ES PLUS style).** Interactive 2D Natural-V.P.A.M. display with nested fractions, balanced parentheses inside fractions (`((2+5)*2)/(3)`), and clickable caret placement. Includes `SHIFT`, `HYP`, `DEG/RAD/GRAD`, `S⇔D`, `a b/c`, `ENG`, `CONST`, memory (`MC/MR/M-/M+/STO`), `Ans` chaining after `=`, automatic parenthesis closing, token-aware `DEL`, and saved history.
- **2D Function Plotter.** Multiple curves on `egui_plot`, automatic sliders for free parameters (`y = a * sin(b * x)`) with animation, discontinuity splitting around asymptotes (`1/x`, `tan(x)`), and cursor snapping to roots, extrema, Y-intercepts, and curve intersections.
- **Programmer & Unit Converter.** 64/32/16/8-bit register (`HEX`/`DEC`/`OCT`/`BIN`, signed and unsigned, bitwise ops, clickable and drag-paintable 64-bit grid) alongside an engineering unit converter.
- **Desktop Integration.** Works on Windows and Linux (Wayland / X11 with `app_id = "luminacalc"` and a `.desktop` entry). Reads the Windows accent color from `DWM\AccentColor`, persists state to TOML, supports a global summon shortcut (`Ctrl+Alt+Space`), always-on-top mode, and ships with embedded multi-resolution icons.

---

## Download

Prebuilt x86_64 binaries for Windows and Linux are available on the [**Latest Release**](https://github.com/H1rolce/LuminaCalc/releases/latest) page (see also [all releases](https://github.com/H1rolce/LuminaCalc/releases)):

- **Windows:** `LuminaCalc-windows-x86_64.zip` — portable `LuminaCalc.exe`, extract and run.
- **Linux (AppImage):** `LuminaCalc-linux-x86_64.AppImage` — standalone portable image:
  ```bash
  chmod +x LuminaCalc-linux-x86_64.AppImage
  ./LuminaCalc-linux-x86_64.AppImage
  ```
- **Linux (tar.gz):** `LuminaCalc-linux-x86_64.tar.gz` — `luminacalc` binary, `luminacalc.desktop` launcher, and `luminacalc.png` icon.

---

## Keyboard Shortcuts

| Shortcut | Action |
|---|---|
| `Ctrl+Alt+Space` | Global summon / focus window (Windows / X11; on Wayland configure via compositor) |
| `Ctrl+1` … `Ctrl+4` | Switch tabs (Notebook / Scientific / Plotter / Programmer) |
| `Ctrl++` / `Ctrl+-` | Increase / decrease Notebook font size |
| `S` / `Shift+S` / `Ctrl+S` | Scientific: `sin(` / `asin(` / `sinh(` (layout-independent) |
| `O` / `Shift+O` / `Ctrl+O` | Scientific: `cos(` / `acos(` / `cosh(` (layout-independent) |
| `T` / `Shift+T` / `Ctrl+T` | Scientific: `tan(` / `atan(` / `tanh(` (layout-independent) |
| `L` / `N` / `R` / `Q` / `P` / `E` / `Shift+E` / `D` | Scientific: `log(`, `ln(`, `√(`, `²`, `π`, `e`, `×10ˣ`, `S⇔D` |
| `F5` / `F6` / `F7` / `F8` | Programmer: switch `HEX` / `DEC` / `OCT` / `BIN` |
| `◀` `▶` `▲` `▼` `Tab` | Navigate expression, fractions, and history in Scientific mode |
| `Esc` | Clear expression / close `CONST` menu |

---

## Building from Source

Requires Rust (`edition = 2024`).

On **Linux (Ubuntu / Debian)**, install the system development libraries first:

```bash
sudo apt-get update
sudo apt-get install -y pkg-config libxkbcommon-dev libwayland-dev libx11-dev libxi-dev \
  libxcursor-dev libxrandr-dev libgl1-mesa-dev libfontconfig1-dev libgtk-3-dev \
  libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
```

Then run tests, Clippy, or build the release binary:

```bash
# Run the full workspace test suite
cargo test --workspace

# Run Clippy with warnings as errors
cargo clippy --workspace --all-targets -- -D warnings

# Optimized release build (-O3, fat LTO, single codegen-unit, strip)
cargo build --release -p lumina-ui
```

---

## Architecture

The workspace is split into two crates: the headless math engine (`lumina-core`) and the GUI (`lumina-ui`).

```text
LuminaCalc/
├── Cargo.toml                  # Workspace + release profile (-O3, fat LTO, strip)
├── LICENSE                     # MIT License
├── lumina-core/                # Headless calculation engine (zero GUI dependencies)
│   ├── src/
│   │   ├── lib.rs              # Public core facade
│   │   ├── error.rs            # CalcError, ErrorKind, ErrorCategory (Syntax vs Math), Span
│   │   ├── lexer.rs            # Zero-copy winnow lexer (EN/RU, 0x/0b/0o, °, ², ³, #ref)
│   │   ├── parser.rs           # Precedence-climbing parser, implicit mul, auto_close_parens
│   │   ├── eval.rs             # Exact Decimal evaluator + modular trigonometry
│   │   ├── units.rs            # Dimensional analysis (8D vector: 7 SI + Info) & unit table
│   │   ├── value.rs            # Value, UnitHint, formatters (Decimal/Sci/Radix/Eng/Fraction)
│   │   ├── highlight.rs        # Semantic token classification for syntax highlighting
│   │   ├── notebook.rs         # Reactive notebook engine with per-line AST caching
│   │   ├── plot.rs             # Fast f64 evaluator, curve sampler, root/intersection solvers
│   │   └── programmer.rs       # BitRegister (8/16/32/64-bit, signed/unsigned, shifts/rotates)
│   └── tests/
│       └── integration.rs      # End-to-end core integration tests
└── lumina-ui/                  # Desktop UI built on eframe / egui
    ├── build.rs                # Embeds multi-resolution icon.ico on Windows
    ├── assets/                 # icon.png, icon.ico, icon.rc, luminacalc.desktop
    └── src/
        ├── main.rs             # eframe entry point, frameless viewport, window/taskbar icon
        ├── app.rs              # Root state, shortcuts, clipboard, debounced TOML persistence
        ├── state.rs            # AppState serialization/deserialization (state.toml)
        ├── style.rs            # Design system, Dark/Light palettes, fonts, LayoutJob highlighter
        ├── win_theme.rs        # Windows registry reader for system theme & AccentColor
        ├── titlebar.rs         # Custom titlebar, proportional tabs, 8-way frameless resize
        ├── toast.rs            # Lightweight foreground toast notifications
        ├── i18n.rs             # EN / RU localization & ErrorKind formatting
        ├── notebook_view.rs    # Two-column notebook view with LayoutJob & result caching
        ├── sci_view.rs         # Scientific calculator (6-column grid, CONST, Ans, smart DEL)
        ├── math_display.rs     # Interactive 2D Natural-V.P.A.M. parser, renderer & hit-tester
        ├── plot_view.rs        # Plotter UI, parameter sliders, PlotPoints::Borrowed cache
        └── prog_view.rs        # Interactive bit grid & cached engineering unit converter
```

### Data Flow

```mermaid
flowchart LR
    UI["lumina-ui (egui / eframe)"] -->|expression / events| Core["lumina-core (pure Rust)"]
    subgraph Core
        Lexer["lexer.rs (winnow)"] --> Parser["parser.rs (AST)"]
        Lexer --> HL["highlight.rs (HighlightSpan)"]
        Parser --> Eval["eval.rs (Decimal + Units)"]
        Parser --> Plot["plot.rs (f64 + Solvers)"]
        Eval --> Units["units.rs (8D Dim)"]
        Eval --> Val["value.rs (Format / Fraction)"]
    end
    HL -->|syntax spans| UI
    Val -->|Value / CalcError| UI
    Plot -->|segments + SnapPoint| UI
```

---

## Technical Details

### 1. Exact Arithmetic & Trigonometry (`lumina-core`)
- Core arithmetic runs on 96-bit `rust_decimal::Decimal`, avoiding IEEE 754 floating-point rounding artifacts (`0.1 + 0.2 = 0.3`).
- Transcendental functions evaluate in `f64` and round to 15 significant digits. In `Deg` and `Grad` modes, angles are reduced modulo `360` / `400` in `Decimal` first: `sin(180°)` and `cos(90°)` return exact `0`, while poles (`tan(90°)`, `tan(π/2)`, `cot(180°)`) return `ErrorKind::Domain` (`Math ERROR`).
- Negative zero (`-0`) is normalized away both in `Decimal` and during formatting.
- Every error carries a byte `Span` and an `ErrorCategory::{Syntax, Math}`. User input never hits `.unwrap()` or `.expect()`, and `NaN` / `Infinity` never appear in the UI.

### 2. Parser & Precedence (Casio ClassWiz)
- Precedence (low to high): `in`-conversion → `|` → `xor` → `&` → `<<`/`>>` → `+`/`-` → `*`/`/`/`mod`/`of`/`%` → implicit multiplication (`2(3+4)`, `(2+3)(4+5)`, `(2+3)2`, `2pi`, `3sin(30)`, `2Ans`, `1/2pi = 1/(2π)`) → unary `-`/`+`/`~` → `^` (right-associative: `2^3^2 = 512`, `-2^2 = -4`) → postfix `!`, `%`, `²`, `³` → primaries.
- `auto_close_parens` balances trailing open parentheses on `=` in Scientific mode (`sin(30` → `sin(30)`).

### 3. Dimensional Analysis (`units.rs`)
- Physical dimensions are stored as an 8-byte exponent vector `Dim([i8; 8])` (length, mass, time, current, temperature, amount, luminous intensity, information in bits).
- Offset temperature scales (`°C`, `°F`) convert through Kelvin without precision drift.

### 4. Interactive 2D Natural-V.P.A.M. Renderer (`math_display.rs` + `sci_view.rs`)
- A dedicated 2D parser builds a spanned `MathNode` tree (`Frac`, `Sup`, `Sqrt`, `Paren`, `Slot`) even for incomplete expressions.
- Rendering records clickable `HitZone` rectangles and `CaretPos` coordinates for direct cursor placement inside numerators, denominators, or between digits.
- `ScientificView` caches the parsed `MathNode` tree rather than re-parsing on cursor blink ticks.

### 5. UI Caching & Idle Behavior
- **Notebook:** caches `LayoutJob` by `(doc_version, theme, accent, font_size, wrap_width)` (avoiding stale glyph-atlas UVs) together with formatted result strings.
- **Plotter:** caches sampled curve segments and snap points by `(version, bounds)` and passes them via `PlotPoints::Borrowed` without per-frame copies.
- **Programmer:** caches unit converter results by `(cat, from, to, val)`.
- **Idle behavior:** debounced TOML saves (`0.8s`) and toast notifications schedule repaints with `request_repaint_after`, keeping idle CPU and GPU usage near zero.

---

## License

MIT License. See [LICENSE](LICENSE) for details.
