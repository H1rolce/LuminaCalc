use lumina_core::ErrorKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Lang {
    Ru,
    En,
}

impl Lang {
    pub fn detect() -> Self {
        match sys_locale::get_locale() {
            Some(loc) if loc.to_ascii_lowercase().starts_with("ru") => Lang::Ru,
            _ => Lang::En,
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Lang::Ru => Lang::En,
            Lang::En => Lang::Ru,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::Ru => "RU",
            Lang::En => "EN",
        }
    }
}

pub struct Strings {
    pub tab_notebook: &'static str,
    pub tab_scientific: &'static str,
    pub tab_plot: &'static str,
    pub tab_programmer: &'static str,

    pub theme_tooltip: &'static str,
    pub lang_tooltip: &'static str,
    pub pin_tooltip: &'static str,
    pub hotkey_hint: &'static str,

    pub nb_placeholder: &'static str,
    pub nb_vars_label: &'static str,
    pub nb_no_vars: &'static str,
    pub nb_precision: &'static str,
    pub nb_clear: &'static str,
    pub nb_examples: &'static str,
    pub nb_copy_val: &'static str,
    pub nb_copy_full: &'static str,
    pub nb_insert_ref: &'static str,
    pub nb_send_to_plot: &'static str,
    pub nb_click_to_copy: &'static str,
    pub nb_copied: &'static str,
    pub nb_default_doc: &'static str,

    pub plot_functions: &'static str,
    pub plot_add_fn: &'static str,
    pub plot_params: &'static str,
    pub plot_no_params: &'static str,
    pub plot_reset_view: &'static str,
    pub plot_snap_toggle: &'static str,
    pub plot_snap_root: &'static str,
    pub plot_snap_intersection: &'static str,
    pub plot_snap_yintercept: &'static str,
    pub plot_snap_extremum: &'static str,
    pub plot_click_copy_coords: &'static str,
    pub plot_expr_hint: &'static str,

    pub prog_expr_label: &'static str,
    pub prog_expr_hint: &'static str,
    pub prog_apply: &'static str,
    pub prog_signed: &'static str,
    pub prog_unsigned: &'static str,
    pub prog_bits_grid: &'static str,
    pub prog_bits_hint: &'static str,
    pub prog_set_bits: &'static str,
    pub prog_bytes: &'static str,
    pub prog_converter_title: &'static str,
    pub prog_conv_value: &'static str,
    pub prog_send_notebook: &'static str,

    pub sci_history: &'static str,
    pub sci_history_empty: &'static str,
}

pub const RU: Strings = Strings {
    tab_notebook: "Блокнот",
    tab_scientific: "Инженерный",
    tab_plot: "Графики",
    tab_programmer: "Программист",

    theme_tooltip: "Сменить тему",
    lang_tooltip: "Сменить язык RU / EN",
    pin_tooltip: "Поверх всех окон",
    hotkey_hint: "Глобальный вызов: Ctrl+Alt+Space",

    nb_placeholder: "Введите выражение, например: 120 км/ч в м/с",
    nb_vars_label: "Переменные:",
    nb_no_vars: "Нет переменных — задайте через x = 42",
    nb_precision: "Точность:",
    nb_clear: "Очистить",
    nb_examples: "Примеры",
    nb_copy_val: "Копировать число",
    nb_copy_full: "Копировать с единицами",
    nb_insert_ref: "Вставить ссылку",
    nb_send_to_plot: "Построить график",
    nb_click_to_copy: "ЛКМ — копировать • ПКМ — действия",
    nb_copied: "Скопировано",
    nb_default_doc: "\
# Физика и конвертация единиц
dist = 420 км
speed = 120 км/ч
speed в м/с
time = dist / speed в мин

# Точная арифметика и проценты
0.1 + 0.2
price = 2490
price - 15%
20% от price

# Геометрия и константы
r = 5 см
area = pi * r^2 в см^2
sin(30°) + cos(60°)

# Итоги блока и ссылки на строки
120
340
540
сумма
среднее
#22 + #23

# Программист и данные
(0xFF << 4) | 0x0A в hex
1 GiB в MB
22 °C в °F",

    plot_functions: "Функции",
    plot_add_fn: "+ Функция",
    plot_params: "Параметры",
    plot_no_params: "Добавьте параметры в формулу (например, a * sin(b * x)) — слайдеры появятся автоматически.",
    plot_reset_view: "Сброс вида",
    plot_snap_toggle: "Прилипание",
    plot_snap_root: "Корень",
    plot_snap_intersection: "Пересечение",
    plot_snap_yintercept: "Пересечение с Y",
    plot_snap_extremum: "Экстремум",
    plot_click_copy_coords: "Клик — копировать координаты",
    plot_expr_hint: "y = a * sin(b * x)",

    prog_expr_label: "Выражение:",
    prog_expr_hint: "например: (0xFF << 4) | 0x0A или x xor 0xFF",
    prog_apply: "Применить",
    prog_signed: "Знаковое",
    prog_unsigned: "Беззнаковое",
    prog_bits_grid: "Битовая сетка",
    prog_bits_hint: "Клик или перетаскивание — переключить биты",
    prog_set_bits: "Единиц:",
    prog_bytes: "Байты (BE):",
    prog_converter_title: "Инженерный конвертер единиц",
    prog_conv_value: "Значение",
    prog_send_notebook: "В блокнот",

    sci_history: "История вычислений",
    sci_history_empty: "Нажмите «=» или Enter, чтобы сохранить расчёт в историю.",
};

pub const EN: Strings = Strings {
    tab_notebook: "Notebook",
    tab_scientific: "Scientific",
    tab_plot: "Plotter",
    tab_programmer: "Programmer",

    theme_tooltip: "Switch theme",
    lang_tooltip: "Switch language RU / EN",
    pin_tooltip: "Always on top",
    hotkey_hint: "Global hotkey: Ctrl+Alt+Space",

    nb_placeholder: "Type an expression, e.g. 120 km/h in m/s",
    nb_vars_label: "Variables:",
    nb_no_vars: "No variables yet — define with x = 42",
    nb_precision: "Precision:",
    nb_clear: "Clear",
    nb_examples: "Examples",
    nb_copy_val: "Copy number",
    nb_copy_full: "Copy with units",
    nb_insert_ref: "Insert reference",
    nb_send_to_plot: "Send to Plotter",
    nb_click_to_copy: "Left click to copy • Right click for actions",
    nb_copied: "Copied",
    nb_default_doc: "\
# Physics & Unit Conversions
dist = 420 km
speed = 120 km/h
speed in m/s
time = dist / speed in min

# Exact Decimal & Percentages
0.1 + 0.2
price = 2490
price - 15%
20% of price

# Geometry & Constants
r = 5 cm
area = pi * r^2 in cm^2
sin(30°) + cos(60°)

# Block Aggregates & Line Refs
120
340
540
sum
avg
#22 + #23

# Programmer & Data
(0xFF << 4) | 0x0A in hex
1 GiB in MB
22 °C in °F",

    plot_functions: "Functions",
    plot_add_fn: "+ Function",
    plot_params: "Parameters",
    plot_no_params: "Use unknown parameters (e.g. a * sin(b * x)) to auto-generate sliders.",
    plot_reset_view: "Reset view",
    plot_snap_toggle: "Snap points",
    plot_snap_root: "Root",
    plot_snap_intersection: "Intersection",
    plot_snap_yintercept: "Y-intercept",
    plot_snap_extremum: "Extremum",
    plot_click_copy_coords: "Click to copy coordinates",
    plot_expr_hint: "y = a * sin(b * x)",

    prog_expr_label: "Expression:",
    prog_expr_hint: "e.g. (0xFF << 4) | 0x0A or x xor 0xFF",
    prog_apply: "Apply",
    prog_signed: "Signed",
    prog_unsigned: "Unsigned",
    prog_bits_grid: "Bit Grid",
    prog_bits_hint: "Click or drag across bits to toggle",
    prog_set_bits: "Popcount:",
    prog_bytes: "Bytes (BE):",
    prog_converter_title: "Engineering Unit Converter",
    prog_conv_value: "Value",
    prog_send_notebook: "To Notebook",

    sci_history: "Calculation History",
    sci_history_empty: "Press '=' or Enter to record calculations here.",
};

#[inline]
pub fn tr(lang: Lang) -> &'static Strings {
    match lang {
        Lang::Ru => &RU,
        Lang::En => &EN,
    }
}

pub fn format_error(lang: Lang, kind: &ErrorKind) -> String {
    match lang {
        Lang::En => kind.to_string(),
        Lang::Ru => match kind {
            ErrorKind::UnexpectedChar => "Недопустимый символ".into(),
            ErrorKind::UnexpectedEnd => "Неожиданный конец выражения".into(),
            ErrorKind::UnexpectedToken => "Неожиданный токен".into(),
            ErrorKind::UnclosedParen => "Не закрыта скобка".into(),
            ErrorKind::InvalidNumber => "Некорректное число".into(),
            ErrorKind::UnknownIdent(s) => format!("Неизвестный идентификатор «{s}»"),
            ErrorKind::UnknownFunction(s) => format!("Неизвестная функция «{s}»"),
            ErrorKind::Arity { name, expected, got } => {
                format!("Функция «{name}» ожидает {expected} арг., получено {got}")
            }
            ErrorKind::DivisionByZero => "Деление на ноль".into(),
            ErrorKind::Overflow => "Числовое переполнение".into(),
            ErrorKind::Domain => "Выход за область определения".into(),
            ErrorKind::DimensionMismatch(a, b) => format!("Несовместимые единицы: {a} и {b}"),
            ErrorKind::ExpectedDimensionless => "Требуется безразмерное значение".into(),
            ErrorKind::ExpectedInteger => "Требуется целое число".into(),
            ErrorKind::BadLineRef(n) => format!("Строка #{n} не содержит значения"),
            ErrorKind::NoPrevious => "Нет предыдущего результата".into(),
            ErrorKind::BadAssignment(s) => format!("Нельзя переопределить «{s}»"),
            ErrorKind::BadConversionTarget => "Некорректная целевая единица".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ru_and_en_errors_are_non_empty() {
        let samples = [
            ErrorKind::DivisionByZero,
            ErrorKind::UnknownIdent("foo".into()),
            ErrorKind::DimensionMismatch("km".into(), "kg".into()),
            ErrorKind::BadLineRef(3),
            ErrorKind::Arity { name: "sin".into(), expected: "1", got: 2 },
        ];
        for err in &samples {
            let ru = format_error(Lang::Ru, err);
            let en = format_error(Lang::En, err);
            assert!(!ru.is_empty());
            assert!(!en.is_empty());
            assert_ne!(ru, en);
        }
    }
}
