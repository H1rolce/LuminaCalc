//! Root application state, keyboard shortcuts, global hotkey integration,
//! clipboard access, debounced TOML persistence, and top-level layout.

use crate::i18n::{Lang, tr};
use crate::notebook_view::{NotebookAction, NotebookView};
use crate::plot_view::{PlotAction, PlotView};
use crate::prog_view::{ProgAction, ProgrammerView};
use crate::sci_view::{SciAction, ScientificView};
use crate::state::{ActiveTab, AppState};
use crate::style::{self, Palette, ThemeId};
use crate::titlebar;
use crate::toast::ToastManager;
use egui::{Context, Frame, Key, ViewportCommand, WindowLevel};
use global_hotkey::hotkey::{Code, HotKey, Modifiers as GhMod};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::time::{Duration, Instant};

pub struct LuminaApp {
    theme: ThemeId,
    lang: Lang,
    tab: ActiveTab,
    always_on_top: bool,
    font_size: f32,
    precision: u32,
    palette: Palette,

    notebook: NotebookView,
    scientific: ScientificView,
    plotter: PlotView,
    programmer: ProgrammerView,

    toasts: ToastManager,
    clipboard: Option<arboard::Clipboard>,
    _hotkey_mgr: Option<GlobalHotKeyManager>,

    dirty_since: Option<Instant>,
    last_sys_theme: crate::win_theme::SystemTheme,
    last_sys_check: Instant,
}

impl LuminaApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        style::install_fonts(&cc.egui_ctx);

        let saved = AppState::load();
        let last_sys_theme = crate::win_theme::read_system_theme();
        let palette = style::apply_theme(&cc.egui_ctx, saved.theme);

        if saved.always_on_top {
            cc.egui_ctx
                .send_viewport_cmd(ViewportCommand::WindowLevel(WindowLevel::AlwaysOnTop));
        }

        // Register global summon hotkey (Ctrl+Alt+Space) without ever panicking if unavailable.
        let hotkey_mgr = setup_global_hotkey(&cc.egui_ctx);

        let notebook = NotebookView::new(saved.notebook_text);
        let scientific = ScientificView::new(&saved.sci_history, saved.sci_angle_unit);
        let plotter = PlotView::new(&saved.plot_curves, &saved.plot_params, saved.plot_snap_enabled);
        let programmer = ProgrammerView::new(
            saved.programmer_value,
            saved.programmer_width,
            saved.programmer_signed,
        );

        Self {
            theme: saved.theme,
            lang: saved.lang,
            tab: saved.tab,
            always_on_top: saved.always_on_top,
            font_size: saved.font_size.clamp(11.0, 26.0),
            precision: saved.precision.clamp(1, 15),
            palette,
            notebook,
            scientific,
            plotter,
            programmer,
            toasts: ToastManager::default(),
            clipboard: arboard::Clipboard::new().ok(),
            _hotkey_mgr: hotkey_mgr,
            dirty_since: None,
            last_sys_theme,
            last_sys_check: Instant::now(),
        }
    }

    fn copy_to_clipboard(&mut self, ctx: &Context, text: &str, display_label: &str) {
        let copied = self
            .clipboard
            .as_mut()
            .and_then(|cb| cb.set_text(text.to_owned()).ok())
            .is_some();
        if !copied {
            ctx.copy_text(text.to_owned());
        }
        let s = tr(self.lang);
        self.toasts.success(format!("{}: {display_label}", s.nb_copied));
    }

    fn mark_dirty(&mut self) {
        self.dirty_since = Some(Instant::now());
    }

    fn build_snapshot(&self) -> AppState {
        let (plot_curves, plot_params, plot_snap_enabled) = self.plotter.to_saved();
        let (sci_history, sci_angle_unit) = self.scientific.to_saved();
        AppState {
            theme: self.theme,
            lang: self.lang,
            tab: self.tab,
            always_on_top: self.always_on_top,
            font_size: self.font_size,
            precision: self.precision,
            notebook_text: self.notebook.text.clone(),
            sci_history,
            sci_angle_unit,
            plot_curves,
            plot_params,
            plot_snap_enabled,
            programmer_value: self.programmer.reg.masked(),
            programmer_width: self.programmer.reg.width.bits() as u8,
            programmer_signed: self.programmer.reg.signed,
        }
    }

    fn flush_save_if_due(&mut self, ctx: &Context) {
        const SAVE_DELAY: Duration = Duration::from_millis(800);
        if let Some(since) = self.dirty_since {
            let elapsed = since.elapsed();
            if elapsed >= SAVE_DELAY {
                self.build_snapshot().save();
                self.dirty_since = None;
            } else {
                ctx.request_repaint_after(SAVE_DELAY - elapsed);
            }
        }
    }

    fn handle_shortcuts(&mut self, ctx: &Context) {
        let mut cycle_theme = false;
        let mut toggle_lang = false;
        let mut next_tab: Option<ActiveTab> = None;
        let mut font_delta = 0.0f32;

        ctx.input(|i| {
            let ctrl = i.modifiers.command || i.modifiers.ctrl;
            if ctrl && !i.modifiers.alt {
                if i.key_pressed(Key::Num1) {
                    next_tab = Some(ActiveTab::Notebook);
                } else if i.key_pressed(Key::Num2) {
                    next_tab = Some(ActiveTab::Scientific);
                } else if i.key_pressed(Key::Num3) {
                    next_tab = Some(ActiveTab::Plotter);
                } else if i.key_pressed(Key::Num4) {
                    next_tab = Some(ActiveTab::Programmer);
                } else if i.key_pressed(Key::T) {
                    cycle_theme = true;
                } else if i.key_pressed(Key::L) {
                    toggle_lang = true;
                } else if i.key_pressed(Key::Equals) || i.key_pressed(Key::Plus) {
                    font_delta = 1.0;
                } else if i.key_pressed(Key::Minus) {
                    font_delta = -1.0;
                }
            }
        });

        if let Some(t) = next_tab
            && t != self.tab
        {
            self.tab = t;
            self.mark_dirty();
        }
        if cycle_theme {
            self.cycle_theme(ctx);
        }
        if toggle_lang {
            self.toggle_lang();
        }
        if font_delta != 0.0 {
            self.font_size = (self.font_size + font_delta).clamp(11.0, 26.0);
            self.mark_dirty();
        }
    }

    fn cycle_theme(&mut self, ctx: &Context) {
        self.theme = self.theme.next();
        self.palette = style::apply_theme(ctx, self.theme);
        let prefix = if self.lang == Lang::Ru { "Тема" } else { "Theme" };
        self.toasts.info(format!("{prefix}: {}", self.theme.label(self.lang)));
        self.mark_dirty();
    }

    fn toggle_lang(&mut self) {
        self.lang = self.lang.toggle();
        let msg = match self.lang {
            Lang::Ru => "Язык: Русский",
            Lang::En => "Language: English",
        };
        self.toasts.info(msg);
        self.mark_dirty();
    }

    fn toggle_pin(&mut self, ctx: &Context) {
        self.always_on_top = !self.always_on_top;
        let level = if self.always_on_top {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        };
        ctx.send_viewport_cmd(ViewportCommand::WindowLevel(level));
        self.mark_dirty();
    }
}

impl eframe::App for LuminaApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.palette.bg_base.to_normalized_gamma_f32()
    }

    fn on_exit(&mut self) {
        self.build_snapshot().save();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        if self.last_sys_check.elapsed().as_secs_f32() >= 2.0 {
            self.last_sys_check = Instant::now();
            let cur = crate::win_theme::read_system_theme();
            if cur.accent != self.last_sys_theme.accent {
                self.last_sys_theme = cur;
                self.palette = style::apply_theme(&ctx, self.theme);
            }
        }

        self.handle_shortcuts(&ctx);

        // ── Custom frameless titlebar ──
        let mut tb_action = None;
        egui::Panel::top("lumina_titlebar")
            .frame(Frame::NONE)
            .show(ui, |ui| {
                tb_action = Some(titlebar::show_titlebar(
                    ui,
                    self.tab,
                    self.theme,
                    self.lang,
                    self.always_on_top,
                    &self.palette,
                ));
            });

        if let Some(act) = tb_action {
            if let Some(new_tab) = act.tab_changed {
                self.tab = new_tab;
                self.mark_dirty();
            }
            if act.theme_cycled {
                self.cycle_theme(&ctx);
            }
            if act.lang_toggled {
                self.toggle_lang();
            }
            if act.pin_toggled {
                self.toggle_pin(&ctx);
            }
        }

        // ── Main content area ──
        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(self.palette.bg_base))
            .show(ui, |ui| {
                match self.tab {
                    ActiveTab::Notebook => {
                        let prev_prec = self.precision;
                        let (act, changed) = self.notebook.show(
                            ui,
                            self.theme,
                            self.lang,
                            self.font_size,
                            &mut self.precision,
                            &self.palette,
                        );
                        if changed || self.precision != prev_prec {
                            self.mark_dirty();
                        }
                        match act {
                            NotebookAction::None => {}
                            NotebookAction::Copy { text, label } => {
                                self.copy_to_clipboard(&ctx, &text, &label);
                            }
                            NotebookAction::SendToPlot(expr) => {
                                self.plotter.add_expression(expr);
                                self.tab = ActiveTab::Plotter;
                                self.mark_dirty();
                            }
                        }
                    }
                    ActiveTab::Scientific => {
                        let (act, changed) =
                            self.scientific.show(ui, self.lang, self.precision, &self.palette);
                        if changed {
                            self.mark_dirty();
                        }
                        match act {
                            SciAction::None => {}
                            SciAction::Copy { text, label } => {
                                self.copy_to_clipboard(&ctx, &text, &label);
                            }
                            SciAction::SendToNotebook(line) => {
                                self.notebook.append_line(&line);
                                self.tab = ActiveTab::Notebook;
                                self.mark_dirty();
                            }
                        }
                    }
                    ActiveTab::Plotter => {
                        let (act, changed) =
                            self.plotter.show(ui, self.lang, &self.palette);
                        if changed {
                            self.mark_dirty();
                        }
                        if let PlotAction::Copy(coords) = act {
                            self.copy_to_clipboard(&ctx, &coords, &coords);
                        }
                    }
                    ActiveTab::Programmer => {
                        let (act, changed) =
                            self.programmer.show(ui, self.lang, &self.palette);
                        if changed {
                            self.mark_dirty();
                        }
                        match act {
                            ProgAction::None => {}
                            ProgAction::Copy(val) => {
                                self.copy_to_clipboard(&ctx, &val, &val);
                            }
                            ProgAction::SendToNotebook(line) => {
                                self.notebook.append_line(&line);
                                self.tab = ActiveTab::Notebook;
                                self.mark_dirty();
                            }
                        }
                    }
                }
            });

        // Frameless 8-way resize edges & crisp outer 1px border
        titlebar::handle_frameless_resize(&ctx, &self.palette);

        // Animated foreground toast overlay
        self.toasts.paint(&ctx, &self.palette);

        // Debounced TOML persistence
        self.flush_save_if_due(&ctx);
    }
}

fn setup_global_hotkey(ctx: &Context) -> Option<GlobalHotKeyManager> {
    let mgr = GlobalHotKeyManager::new().ok()?;
    let hk = HotKey::new(Some(GhMod::CONTROL | GhMod::ALT), Code::Space);
    let hk_id = hk.id();
    if mgr.register(hk).is_err() {
        return None;
    }
    let ctx_clone = ctx.clone();
    GlobalHotKeyEvent::set_event_handler(Some(move |ev: GlobalHotKeyEvent| {
        if ev.id() == hk_id && ev.state() == HotKeyState::Pressed {
            ctx_clone.send_viewport_cmd(ViewportCommand::Minimized(false));
            ctx_clone.send_viewport_cmd(ViewportCommand::Visible(true));
            ctx_clone.send_viewport_cmd(ViewportCommand::Focus);
            ctx_clone.request_repaint();
        }
    }));
    Some(mgr)
}
