use crate::i18n::Lang;
use crate::style::ThemeId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ActiveTab {
    #[default]
    Notebook,
    Scientific,
    Plotter,
    Programmer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedPlotCurve {
    pub expr: String,
    pub visible: bool,
    pub color_idx: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedPlotParam {
    pub name: String,
    pub value: f64,
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedSciHistoryItem {
    pub expr: String,
    pub result_display: String,
    pub result_plain: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppState {
    pub theme: ThemeId,
    pub lang: Lang,
    pub tab: ActiveTab,
    pub always_on_top: bool,
    pub font_size: f32,
    pub precision: u32,
    pub notebook_text: String,
    pub sci_history: Vec<SavedSciHistoryItem>,
    pub sci_angle_unit: u8,
    pub plot_curves: Vec<SavedPlotCurve>,
    pub plot_params: Vec<SavedPlotParam>,
    pub plot_snap_enabled: bool,
    pub programmer_value: u64,
    pub programmer_width: u8,
    pub programmer_signed: bool,
}

impl Default for AppState {
    fn default() -> Self {
        let lang = Lang::detect();
        let default_doc = crate::i18n::tr(lang).nb_default_doc.to_owned();
        let theme = if crate::win_theme::read_system_theme().dark {
            ThemeId::Dark
        } else {
            ThemeId::Light
        };
        Self {
            theme,
            lang,
            tab: ActiveTab::Notebook,
            always_on_top: false,
            font_size: 15.0,
            precision: 8,
            notebook_text: default_doc,
            sci_history: Vec::new(),
            sci_angle_unit: 0,
            plot_curves: vec![
                SavedPlotCurve {
                    expr: "y = a * sin(b * x)".into(),
                    visible: true,
                    color_idx: 0,
                },
                SavedPlotCurve {
                    expr: "y = 0.25 * x^2 - c".into(),
                    visible: true,
                    color_idx: 1,
                },
            ],
            plot_params: vec![
                SavedPlotParam { name: "a".into(), value: 2.0, min: -5.0, max: 5.0 },
                SavedPlotParam { name: "b".into(), value: 1.5, min: 0.1, max: 5.0 },
                SavedPlotParam { name: "c".into(), value: 1.5, min: -5.0, max: 5.0 },
            ],
            plot_snap_enabled: true,
            programmer_value: 0x0000_0000_0000_CAFE,
            programmer_width: 64,
            programmer_signed: false,
        }
    }
}

impl AppState {
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("LuminaCalc").join("state.toml"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(raw) => toml::from_str(&raw).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let Some(path) = Self::config_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(serialized) = toml::to_string_pretty(self) {
            let _ = std::fs::write(path, serialized);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_roundtrip() {
        let mut orig = AppState::default();
        orig.sci_history.push(SavedSciHistoryItem {
            expr: "(2)/(3) + (1)/(6)".into(),
            result_display: "5/6".into(),
            result_plain: "0.83333333".into(),
        });
        let raw = toml::to_string_pretty(&orig).expect("serialize");
        let parsed: AppState = toml::from_str(&raw).expect("deserialize");
        assert_eq!(parsed.theme, orig.theme);
        assert_eq!(parsed.lang, orig.lang);
        assert_eq!(parsed.precision, orig.precision);
        assert_eq!(parsed.sci_history, orig.sci_history);
        assert_eq!(parsed.plot_curves.len(), orig.plot_curves.len());
        assert_eq!(parsed.programmer_value, orig.programmer_value);
    }
}
