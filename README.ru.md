# LuminaCalc

[English](README.md) | **Русский**

**LuminaCalc** — быстрый нативный кроссплатформенный десктопный калькулятор на Rust (`egui` / `eframe`) для **Windows** и **Linux**, объединяющий 4 режима работы в одном легковесном окне с переключением темы Dark / Light (и автоподстройкой системного акцента на Windows) и двуязычным интерфейсом (RU / EN).

![Демо LuminaCalc](preview.gif)

[![Latest Release](https://img.shields.io/github/v/release/H1rolce/LuminaCalc?label=%D0%A1%D0%BA%D0%B0%D1%87%D0%B0%D1%82%D1%8C%20%D0%BF%D0%BE%D1%81%D0%BB%D0%B5%D0%B4%D0%BD%D0%B8%D0%B9%20%D1%80%D0%B5%D0%BB%D0%B8%D0%B7&style=for-the-badge)](https://github.com/H1rolce/LuminaCalc/releases/latest)

---

## Возможности

- **Блокнот (Soulver / Numi style)** — многострочные реактивные расчёты с переменными, ссылками на строки (`#1`, `#2`), блочными агрегатами (`sum`/`сумма`, `avg`/`среднее`), конвертацией физических единиц (`120 км/ч в м/с`, `22 °C в °F`) и форматов (`255 в hex`).
- **Инженерный калькулятор (Casio ClassWiz / fx-991ES PLUS style)** — интерактивный 2D Natural-V.P.A.M. дисплей с многоэтажными дробями, вводом скобок внутри дробей (`((2+5)*2)/(3)`), курсорным редактированием кликом мыши, `SHIFT`, `HYP`, `DEG/RAD/GRAD`, `S⇔D`, `a b/c`, `ENG`, `CONST`, памятью (`MC/MR/M-/M+/STO`), цепочкой `Ans`, автозакрытием скобок на `=`, умным `DEL` и сохраняемой историей.
- **Графики (2D Plotter)** — построение нескольких функций на `egui_plot`, автогенерация слайдеров свободных параметров (`y = a * sin(b * x)`) с плавной анимацией, разрыв асимптот (`1/x`, `tan(x)`) и магнитное прилипание курсора к корням, экстремумам, пересечениям с осью Y и между кривыми.
- **Программист + Конвертер единиц** — 64/32/16/8-битный регистр (`HEX`/`DEC`/`OCT`/`BIN`, знаковый/беззнаковый, побитовые операции, кликабельная/перетаскиваемая 64-битная сетка) и инженерный конвертер величин.
- **Системная интеграция** — нативная работа на **Windows** и **Linux** (Wayland / X11 с `app_id = "luminacalc"` и `.desktop`-ярлыком), чтение акцентного цвета системы на Windows (`DWM\AccentColor`), автосохранение состояния в TOML, глобальная горячая клавиша вызова (`Ctrl+Alt+Space`), режим «поверх всех окон» и встроенные иконки.

---

## Скачать

Готовые сборки x86_64 для **Windows** и **Linux** доступны на странице [**Последнего релиза**](https://github.com/H1rolce/LuminaCalc/releases/latest) (см. также [все релизы](https://github.com/H1rolce/LuminaCalc/releases)):

- **Windows:** `LuminaCalc-windows-x86_64.zip` — портативный `LuminaCalc.exe`, достаточно распаковать и запустить.
- **Linux (AppImage):** `LuminaCalc-linux-x86_64.AppImage` — универсальный портативный пакет:
  ```bash
  chmod +x LuminaCalc-linux-x86_64.AppImage
  ./LuminaCalc-linux-x86_64.AppImage
  ```
- **Linux (tar.gz):** `LuminaCalc-linux-x86_64.tar.gz` — нативный бинарник `luminacalc`, ярлык `luminacalc.desktop` и иконка `luminacalc.png`.

---

## Горячие клавиши

| Сочетание | Действие |
|---|---|
| `Ctrl+Alt+Space` | Глобальный вызов / фокус окна |
| `Ctrl+1` … `Ctrl+4` | Переключение вкладок (Блокнот / Инженерный / Графики / Программист) |
| `Ctrl++` / `Ctrl+-` | Увеличить / уменьшить шрифт в Блокноте |
| `S` / `Shift+S` / `Ctrl+S` | Инженерный: `sin(` / `asin(` / `sinh(` (независимо от раскладки) |
| `O` / `Shift+O` / `Ctrl+O` | Инженерный: `cos(` / `acos(` / `cosh(` (независимо от раскладки) |
| `T` / `Shift+T` / `Ctrl+T` | Инженерный: `tan(` / `atan(` / `tanh(` (независимо от раскладки) |
| `L` / `N` / `R` / `Q` / `P` / `E` / `Shift+E` / `D` | Инженерный: `log(`, `ln(`, `√(`, `²`, `π`, `e`, `×10ˣ`, `S⇔D` |
| `F5` / `F6` / `F7` / `F8` | Программист: переключение `HEX` / `DEC` / `OCT` / `BIN` |
| `◀` `▶` `▲` `▼` `Tab` | Навигация по выражению, дробям и истории в Инженерном режиме |
| `Esc` | Очистить выражение / закрыть меню `CONST` |

---

## Сборка из исходников

Требуется Rust (`edition = 2024`).

На **Linux (Ubuntu / Debian)** предварительно установите системные `-dev` пакеты:

```bash
sudo apt-get update
sudo apt-get install -y pkg-config libxkbcommon-dev libwayland-dev libx11-dev libxi-dev \
  libxcursor-dev libxrandr-dev libgl1-mesa-dev libfontconfig1-dev libgtk-3-dev \
  libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
```

Затем запустите тесты, линтер или релизную сборку:

```bash
# Запуск тестов всего workspace
cargo test --workspace

# Проверка линтером
cargo clippy --workspace --all-targets -- -D warnings

# Оптимизированная release-сборка (-O3, fat LTO, single codegen-unit, strip)
cargo build --release -p lumina-ui
```

---

## Архитектура проекта

Проект организован как **Cargo Workspace** из двух строго разделённых крейтов:

```text
LuminaCalc/
├── Cargo.toml                  # Workspace + release-профиль (-O3, LTO fat, strip)
├── LICENSE                     # Лицензия MIT
├── lumina-core/                # Чистое математическое ядро (0 GUI-зависимостей)
│   ├── src/
│   │   ├── lib.rs              # Публичный фасад ядра
│   │   ├── error.rs            # CalcError, ErrorKind, ErrorCategory (Syntax vs Math), Span
│   │   ├── lexer.rs            # Zero-copy лексер на winnow (EN/RU, 0x/0b/0o, °, ², ³, #ref)
│   │   ├── parser.rs           # Precedence-climbing парсер, неявное умножение, auto_close_parens
│   │   ├── eval.rs             # Точный вычислитель на Decimal + модульная тригонометрия
│   │   ├── units.rs            # Размерностный анализ (8D-вектор: 7 SI + Info) и таблица единиц
│   │   ├── value.rs            # Value, UnitHint, форматирование (Decimal/Sci/Radix/Eng/Fraction)
│   │   ├── highlight.rs        # Семантическая классификация токенов для подсветки синтаксиса
│   │   ├── notebook.rs         # Реактивный движок блокнота с кэшированием AST по строкам
│   │   ├── plot.rs             # Быстрый f64-вычислитель, семплирование кривых, поиск корней/пересечений
│   │   └── programmer.rs       # BitRegister (8/16/32/64-bit, signed/unsigned, сдвиги, повороты)
│   └── tests/
│       └── integration.rs      # Интеграционные тесты ядра
└── lumina-ui/                  # Десктопный интерфейс на eframe / egui
    ├── build.rs                # Встраивание многоразмерной icon.ico на Windows
    ├── assets/                 # icon.png, icon.ico, icon.rc, luminacalc.desktop
    └── src/
        ├── main.rs             # Точка входа eframe, окно без рамок, иконка окна/панели задач
        ├── app.rs              # Корневое состояние, хоткеи, буфер обмена, дебаунс сохранения TOML
        ├── state.rs            # Сериализация/десериализация AppState в state.toml
        ├── style.rs            # Дизайн-система, генерация палитр Dark/Light, шрифты, подсветка LayoutJob
        ├── win_theme.rs        # Чтение системной темы и AccentColor из реестра Windows (advapi32)
        ├── titlebar.rs         # Кастомная шапка окна, пропорциональные вкладки, 8-сторонний ресайз
        ├── toast.rs            # Лёгкие всплывающие уведомления поверх UI
        ├── i18n.rs             # Локализация RU / EN и перевод ErrorKind
        ├── notebook_view.rs    # Двухколоночный редактор блокнота с кэшем LayoutJob и результатов
        ├── sci_view.rs         # Инженерный калькулятор (6-колоночная сетка, CONST, Ans, smart DEL)
        ├── math_display.rs     # Интерактивный 2D Natural-V.P.A.M. парсер, рендер и hit-testing
        ├── plot_view.rs        # Интерфейс плоттера, слайдеры параметров, кэш PlotPoints::Borrowed
        └── prog_view.rs        # Битовая сетка программиста и кэшируемый конвертер единиц
```

### Поток данных и разделение ответственности

```mermaid
flowchart LR
    UI["lumina-ui (egui / eframe)"] -->|строка / события| Core["lumina-core (pure Rust)"]
    subgraph Core
        Lexer["lexer.rs (winnow)"] --> Parser["parser.rs (AST)"]
        Lexer --> HL["highlight.rs (HighlightSpan)"]
        Parser --> Eval["eval.rs (Decimal + Units)"]
        Parser --> Plot["plot.rs (f64 + Solvers)"]
        Eval --> Units["units.rs (8D Dim)"]
        Eval --> Val["value.rs (Format / Fraction)"]
    end
    HL -->|цветовые спаны| UI
    Val -->|Value / CalcError| UI
    Plot -->|сегменты + SnapPoint| UI
```

---

## Ключевые архитектурные решения

### 1. Точная арифметика и тригонометрия (`lumina-core`)
- Базовые вычисления выполняются в 96-битном `rust_decimal::Decimal`, полностью исключая артефакты IEEE 754 (`0.1 + 0.2 = 0.3`).
- Трансцендентные функции используют `f64` с округлением до 15 значащих цифр, при этом тригонометрия в `Deg` и `Grad` предварительно приводится по модулю `360` / `400` в `Decimal` — `sin(180°)` и `cos(90°)` равны ровно `0`, а полюсы `tan(90°)`, `tan(π/2)`, `cot(180°)` возвращают `ErrorKind::Domain` (`Math ERROR`).
- Отрицательный ноль (`-0`) нормализуется на уровне `Decimal` и форматирования.
- Все ошибки несут байтовый `Span` и делятся через `ErrorCategory::{Syntax, Math}` — никаких `.unwrap()` / `.expect()` на пользовательском вводе и никаких `NaN`/`Infinity` в UI.

### 2. Парсер и приоритеты (Casio ClassWiz)
- Приоритет (от низшего к высшему): `in`-конвертация → `|` → `xor` → `&` → `<<`/`>>` → `+`/`-` → `*`/`/`/`mod`/`of`/`%` → неявное умножение (`2(3+4)`, `(2+3)(4+5)`, `(2+3)2`, `2pi`, `3sin(30)`, `2Ans`, `1/2pi = 1/(2π)`) → унарные `-`/`+`/`~` → `^` (правоассоциативная: `2^3^2 = 512`, `-2^2 = -4`) → постфиксные `!`, `%`, `²`, `³` → первичные выражения.
- `auto_close_parens` автоматически закрывает незакрытые скобки на `=` в инженерном режиме (`sin(30` → `sin(30)`).

### 3. Размерностный анализ (`units.rs`)
- Размерность представлена компактным 8-байтовым вектором степеней `Dim([i8; 8])` (длина, масса, время, ток, температура, количество вещества, сила света, информация в битах).
- Смещённые шкалы (`°C`, `°F`) корректно преобразуются в кельвины и обратно без потери точности.

### 4. Интерактивный 2D Natural-V.P.A.M. рендер (`math_display.rs` + `sci_view.rs`)
- Отдельный отказоустойчивый парсер строит дерево `MathNode` (`Frac`, `Sup`, `Sqrt`, `Paren`, `Slot`) с точными байтовыми диапазонами.
- При отрисовке собираются `HitZone`-области и позиция мигающего курсора `CaretPos`, позволяя кликать мышью прямо в числитель, знаменатель или между цифрами.
- Распарсенное дерево `MathNode` кэшируется в `ScientificView` (нет повторного парсинга на тиках мигания курсора).

### 5. Производительность UI и отсутствие лишних аллокаций
- **Блокнот:** кэширует `LayoutJob` по ключу `(doc_version, theme, accent, font_size, wrap_width)` (а не `Arc<Galley>`, чтобы избежать устаревания UV при перепаковке атласа шрифтов) и кэширует отформатированные строки результатов.
- **Плоттер:** кэширует семплированные сегменты и точки прилипания по `(version, bounds)` и передаёт их в `egui_plot` через `PlotPoints::Borrowed` без копирования.
- **Программист:** кэширует результат конвертера единиц по `(cat, from, to, val)`.
- **Автосохранение и тосты:** дебаунс записи `state.toml` (`0.8s`) и тосты используют `request_repaint_after`, не нагружая CPU/GPU в простое.

---

## Лицензия

Распространяется под лицензией **MIT**. Подробности — в файле [LICENSE](LICENSE).
