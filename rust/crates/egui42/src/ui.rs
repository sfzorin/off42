//! The Linux/macOS document window: home screen, title row with menu bar, one toolbar strip,
//! paper canvas, side strips, sheet tabs and a single snackbar. The layout and colours follow
//! the suite's web design so the web, Windows and Linux apps look alike.
use crate::files::{FileDialog, Outcome, Purpose};
use crate::icons::{self, Icon, Icons};
use crate::scene::{self, SceneTextures};
use crate::theme::{self, AppInfo, Palette};
use crate::{open_blocking, run_file_action, save_as, table_rows, REDO, UNDO};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Frame, Key, Layout, Margin, Modifiers,
    Pos2, Rect, Response, RichText, Role, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use suite42_app::{localization, recents, App, DialogRequest, DialogResult};

fn tr(source: &str) -> String {
    localization::text(source)
}

/// Menu and command titles without Windows mnemonics.
fn clean(title: &str) -> String {
    let t = title.replace("&&", "\u{1}").replace('&', "").replace('\u{1}', "&");
    tr(&t)
}

/// Height of the spreadsheet's sheet-tab bar at the bottom of the window.
const SHEET_TABS_HEIGHT: f32 = 40.0;

struct Snack {
    pub(crate) text: String,
    error: bool,
    at: Instant,
}

struct InputForm {
    id: u16,
    title: String,
    fields: Vec<(String, String)>,
    choices: Vec<(usize, Vec<String>)>,
    multiline: Vec<usize>,
    password: bool,
    /// Settings collected for a file action; the file chooser follows.
    file_action: bool,
}

enum Dialog {
    Confirm { id: u16, title: String, text: String },
    Info { title: String, text: String },
    Choices { id: u16, values: Vec<String>, checked: Option<usize> },
    Color { id: u16, color: [u8; 3] },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Table,
    Image,
    Slides,
}

pub struct Host {
    pub(crate) app: Box<dyn App>,
    pub(crate) info: AppInfo,
    pub(crate) path: Option<PathBuf>,
    pub(crate) home: bool,
    pub(crate) text: String,
    pub(crate) text_edited: bool,
    edited_at: Option<Instant>,
    snack: Option<Snack>,
    texture: Option<egui::TextureHandle>,
    image_stamp: u64,
    icons: Icons,
    scenes: SceneTextures,
    input: Option<InputForm>,
    dialog: Option<Dialog>,
    files: Option<FileDialog>,
    /// Page/grid/image magnification; `None` fits images and slides to the window.
    zoom: Option<f32>,
    drawer: bool,
    language: bool,
    about: bool,
    search: String,
    pub(crate) recent: Vec<PathBuf>,
    styled: bool,
    /// Whether the recent-documents list is read from and written to the user's data folder.
    persist: bool,
}

impl Host {
    pub fn new(app: Box<dyn App>, file: Option<PathBuf>) -> Self {
        Self::with_storage(app, file, true)
    }

    /// `persist: false` keeps the recent list in memory only (headless tests).
    pub fn with_storage(app: Box<dyn App>, file: Option<PathBuf>, persist: bool) -> Self {
        let info = theme::app_info(app.name());
        let recent = if persist { recents::load(app.name()) } else { Vec::new() };
        let mut host = Host {
            app,
            info,
            path: None,
            home: file.is_none(),
            text: String::new(),
            text_edited: false,
            edited_at: None,
            snack: None,
            texture: None,
            image_stamp: 0,
            icons: Icons::default(),
            scenes: SceneTextures::default(),
            input: None,
            dialog: None,
            files: None,
            zoom: None,
            drawer: false,
            language: false,
            about: false,
            search: String::new(),
            recent,
            styled: false,
            persist,
        };
        host.zoom = host.default_zoom();
        if let Some(file) = file {
            host.open_path(&file);
        } else if host.app.supports_new() {
            let _ = host.app.new_document();
        }
        host.reload();
        host
    }

    fn default_zoom(&self) -> Option<f32> {
        if self.app.image().is_some() || self.app.canvas_scene().is_some() {
            None
        } else {
            Some(1.0)
        }
    }

    // ----- document state -------------------------------------------------------------

    fn notify(&mut self, text: impl Into<String>) {
        self.snack = Some(Snack { text: text.into(), error: false, at: Instant::now() });
    }

    fn report(&mut self, result: Result<(), String>, done: impl Into<String>) {
        match result {
            Ok(()) => self.notify(done),
            Err(e) => self.snack = Some(Snack { text: e, error: true, at: Instant::now() }),
        }
        self.reload();
    }

    fn reload(&mut self) {
        self.text = suite42_app::from_native_text(&self.app.text());
        self.text_edited = false;
        self.edited_at = None;
        self.image_stamp += 1;
        self.texture = None;
    }

    pub(crate) fn snack_text(&self) -> Option<String> {
        self.snack.as_ref().map(|s| s.text.clone())
    }

    /// Hand typed text to the model before any command, save or selection change. A model
    /// that refuses the edit (for example a rich document) keeps its text and the page reverts.
    pub(crate) fn commit_text(&mut self) {
        if !self.text_edited {
            return;
        }
        self.text_edited = false;
        self.edited_at = None;
        if let Err(e) = self.app.edit(suite42_app::to_native_text(&self.text)) {
            self.snack = Some(Snack { text: e, error: true, at: Instant::now() });
            self.text = suite42_app::from_native_text(&self.app.text());
        }
    }

    fn kind(&self) -> Kind {
        if self.app.table().is_some() || self.app.sparse_table().is_some() {
            Kind::Table
        } else if self.app.image().is_some() {
            Kind::Image
        } else if self.app.canvas_scene().is_some() {
            Kind::Slides
        } else {
            Kind::Text
        }
    }

    fn open_path(&mut self, path: &Path) {
        self.commit_text();
        let import = self.app.opens_as_import(path);
        let result = open_blocking(self.app.as_mut(), path);
        let ok = result.is_ok();
        let name = display_name(path);
        self.report(result, format!("{} {name}", tr("Opened")));
        if ok {
            self.home = false;
            if !import {
                self.path = Some(path.to_path_buf());
            }
            self.zoom = self.default_zoom();
            self.remember(path);
        }
    }

    fn remember(&mut self, path: &Path) {
        if !self.persist {
            if !self.recent.iter().any(|p| p == path) {
                self.recent.insert(0, path.to_path_buf());
            }
            return;
        }
        if let Ok(absolute) = std::path::absolute(path) {
            let _ = recents::remember(self.app.name(), &absolute);
        }
        self.recent = recents::load(self.app.name());
    }

    fn save_to(&mut self, path: &Path) {
        self.commit_text();
        let result = save_as(self.app.as_mut(), path);
        let ok = result.is_ok();
        self.report(result, format!("{} {}", tr("Saved"), display_name(path)));
        if ok {
            let native = ext_of(path) == self.app.extension().to_ascii_lowercase()
                || self.app.save_formats().iter().any(|f| f.extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext_of(path))));
            if native {
                self.path = Some(path.to_path_buf());
            }
            self.remember(path);
        }
    }

    /// Save in place when the current file has a writable format, otherwise ask for a name.
    fn save(&mut self) {
        if !self.app.can_save() {
            self.notify(tr("This document cannot be saved"));
            return;
        }
        let in_place = self.path.clone().filter(|p| {
            let ext = ext_of(p);
            ext == self.app.extension().to_ascii_lowercase()
                || self.app.save_formats().iter().any(|f| f.extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext)))
        });
        match in_place {
            Some(path) => self.save_to(&path),
            None => {
                let ext = self.default_save_extension();
                self.choose_save(ext);
            }
        }
    }

    fn default_save_extension(&self) -> String {
        let current = self.path.as_deref().map(ext_of);
        let formats = self.app.save_formats();
        if let Some(ext) = current.filter(|e| formats.iter().any(|f| f.extensions.contains(&e.as_str()))) {
            return ext;
        }
        formats
            .first()
            .and_then(|f| f.extensions.first().map(|e| e.to_string()))
            .unwrap_or_else(|| self.app.extension().to_string())
    }

    fn start_dir(&self) -> Option<PathBuf> {
        self.path.as_deref().and_then(Path::parent).map(Path::to_path_buf)
    }

    fn stem(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::file_stem)
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| tr("Untitled"))
    }

    fn choose_open(&mut self) {
        let mut extensions: Vec<String> = vec![self.app.extension().to_ascii_lowercase()];
        for f in self.app.open_formats() {
            extensions.extend(f.extensions.iter().map(|e| e.to_ascii_lowercase()));
        }
        extensions.dedup();
        self.files = Some(FileDialog::new(Purpose::Open, tr("Open"), self.start_dir().as_deref(), String::new(), extensions));
    }

    fn choose_save(&mut self, ext: String) {
        let name = format!("{}.{ext}", self.stem());
        self.files = Some(FileDialog::new(
            Purpose::SaveAs,
            format!("{} (.{ext})", tr("Save as")),
            self.start_dir().as_deref(),
            name,
            vec![ext.to_ascii_lowercase()],
        ));
    }

    fn choose_file_action(&mut self, id: u16, values: Option<Vec<String>>) {
        let Some(action) = self.app.file_actions().iter().find(|a| a.id == id).copied() else { return };
        let action = match &values {
            Some(v) => self.app.configured_file_action(id, v).unwrap_or(action),
            None => action,
        };
        let ext = action.extension.to_ascii_lowercase();
        let name = if action.save {
            let stem = self.app.suggested_export_name(id).unwrap_or_else(|| self.stem());
            format!("{stem}.{ext}")
        } else {
            String::new()
        };
        let extensions = if action.save {
            vec![ext]
        } else {
            action
                .pattern
                .split(';')
                .filter_map(|p| p.trim().strip_prefix("*."))
                .map(str::to_ascii_lowercase)
                .collect()
        };
        self.files = Some(FileDialog::new(
            Purpose::Action { id, save: action.save, values },
            clean(action.title),
            self.start_dir().as_deref(),
            name,
            extensions,
        ));
    }

    fn file_action(&mut self, id: u16) {
        self.commit_text();
        if let Some(request) = self.app.file_input_request(id) {
            self.input = Some(InputForm {
                id,
                title: clean(request.title),
                fields: request.fields.into_iter().map(|(l, v)| (clean(l), v)).collect(),
                choices: self.app.input_choices(id),
                multiline: Vec::new(),
                password: false,
                file_action: true,
            });
        } else {
            self.choose_file_action(id, None);
        }
    }

    fn file_chosen(&mut self, purpose: Purpose, path: PathBuf) {
        match purpose {
            Purpose::Open => self.open_path(&path),
            Purpose::SaveAs => self.save_to(&path),
            Purpose::Action { id, values, .. } => {
                self.commit_text();
                let result = match values {
                    Some(values) => self.app.file_action_with_input(id, &path, &values),
                    None => run_file_action(self.app.as_mut(), id, &path),
                };
                self.report(result, format!("{} {}", tr("Done:"), display_name(&path)));
                self.home = false;
            }
        }
    }

    fn new_document(&mut self) {
        self.commit_text();
        let result = self.app.new_document();
        if result.is_ok() {
            self.path = None;
            self.home = false;
            self.zoom = self.default_zoom();
        }
        self.report(result, tr("New document"));
    }

    // ----- commands -------------------------------------------------------------------

    fn is_print(&self, id: u16, title: &str) -> bool {
        self.app.default_print_command() == Some(id) || title.trim_start_matches('&').to_ascii_lowercase().starts_with("print")
    }

    /// Whether the Linux host can carry out a command (no printing or Windows-only dialogs).
    fn available(&self, id: u16) -> bool {
        if let Some(request) = self.app.clipboard_request(id) {
            return matches!(
                request,
                suite42_app::ClipboardRequest::WriteText(_) | suite42_app::ClipboardRequest::WriteDataAndText { .. }
            );
        }
        match self.app.dialog_request(id) {
            None => true,
            Some(d) => matches!(
                d,
                DialogRequest::Confirmation { .. }
                    | DialogRequest::Information { .. }
                    | DialogRequest::RichTextStatistics { .. }
                    | DialogRequest::GridChoices { .. }
                    | DialogRequest::Color(_)
            ),
        }
    }

    fn command_enabled(&self, id: u16) -> bool {
        match id {
            UNDO => self.app.can_undo(),
            REDO => self.app.can_redo(),
            _ => self.app.enabled(id) && self.app.command_state(id).enabled,
        }
    }

    pub(crate) fn command(&mut self, ctx: &egui::Context, id: u16) {
        self.commit_text();
        if let Some(request) = self.app.clipboard_request(id) {
            match request {
                suite42_app::ClipboardRequest::WriteText(text)
                | suite42_app::ClipboardRequest::WriteDataAndText { text, .. } => {
                    ctx.copy_text(text);
                    self.notify(tr("Copied"));
                }
                _ => self.notify(tr("Reading the clipboard from this command is not available on Linux yet")),
            }
            return;
        }
        if self.app.rich_preview(id).is_some() || self.app.print_job(id).is_some() {
            self.notify(tr("Printing is not available on Linux yet"));
            return;
        }
        if let Some(request) = self.app.dialog_request(id) {
            self.dialog = match request {
                DialogRequest::Confirmation { title, text } => Some(Dialog::Confirm { id, title: clean(title), text }),
                DialogRequest::Information { title, text } | DialogRequest::RichTextStatistics { title, text, .. } => {
                    Some(Dialog::Info { title: clean(title), text })
                }
                DialogRequest::GridChoices { values, checked } => Some(Dialog::Choices { id, values, checked }),
                DialogRequest::Color(color) => Some(Dialog::Color { id, color }),
                _ => {
                    self.notify(tr("This command needs a dialog that is not available on Linux yet"));
                    None
                }
            };
            return;
        }
        if let Some(request) = self.app.input_request(id) {
            let mut multiline = self.app.input_multiline_fields(id);
            if self.app.multiline_input(id) && !multiline.contains(&0) {
                multiline.push(0);
            }
            self.input = Some(InputForm {
                id,
                title: clean(request.title),
                fields: request.fields.into_iter().map(|(l, v)| (clean(l), v)).collect(),
                choices: self.app.input_choices(id),
                multiline,
                password: self.app.password_input(id),
                file_action: false,
            });
            return;
        }
        let result = self.app.action(id);
        self.report(result, tr("Done"));
    }

    /// App commands in menu order: (group, id, title, accelerator).
    fn app_commands(&self) -> Vec<(String, u16, String, Option<String>)> {
        let mut out: Vec<(String, u16, String, Option<String>)> = Vec::new();
        let mut push = |group: &str, id: u16, title: &str, acc: Option<&str>| {
            if id != UNDO && id != REDO && !out.iter().any(|c| c.1 == id) {
                out.push((group.to_string(), id, title.to_string(), acc.map(str::to_string)));
            }
        };
        for (id, title) in self.app.actions() {
            push("Edit", *id, title, None);
        }
        for (id, title) in self.app.view_actions() {
            push("View", *id, title, None);
        }
        for spec in self.app.command_specs() {
            push(spec.group, spec.id, spec.title, spec.accelerator);
        }
        out
    }

    /// Toolbar commands: the app's toolbar set, each as an icon button. A curated set (twelve
    /// or fewer) shows every command, with the generic command glyph where the suite has none;
    /// a larger set shows only the commands that have a suite icon.
    pub(crate) fn toolbar_commands(&self) -> Vec<(u16, String, Icon, bool)> {
        let all = self.app_commands();
        let visible: Vec<_> = all
            .iter()
            .filter(|c| self.app.toolbar_command_visible(c.1) && !self.is_print(c.1, &c.2))
            .collect();
        let curated = visible.len() <= 12;
        let mut out = Vec::new();
        let mut group = None;
        for (g, id, title, _) in visible {
            let icon = match icons::for_command(&title.replace('&', "")) {
                Some(icon) => icon,
                None if curated => Icon::Command,
                None => continue,
            };
            if !self.available(*id) {
                continue;
            }
            let new_group = group.as_ref() != Some(g);
            group = Some(g.clone());
            out.push((*id, clean(title), icon, new_group));
        }
        out
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.memory(|m| m.focused().is_some()) && ctx.egui_wants_keyboard_input();
        let pressed = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        if pressed(Modifiers::COMMAND, Key::O) {
            self.choose_open();
        }
        if pressed(Modifiers::COMMAND, Key::S) && !self.home {
            self.save();
        }
        if self.app.supports_new() && pressed(Modifiers::COMMAND, Key::N) {
            self.new_document();
        }
        if typing || self.home {
            return;
        }
        if pressed(Modifiers::COMMAND, Key::Z) && self.app.can_undo() {
            self.command(ctx, UNDO);
        }
        if (pressed(Modifiers::COMMAND, Key::Y) || pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)) && self.app.can_redo() {
            self.command(ctx, REDO);
        }
        for (_, id, title, acc) in self.app_commands() {
            let Some((m, k)) = acc.as_deref().and_then(parse_accelerator) else { continue };
            if self.command_enabled(id) && !self.is_print(id, &title) && pressed(m, k) {
                self.command(ctx, id);
            }
        }
    }

    // ----- frame ----------------------------------------------------------------------

    pub fn draw(&mut self, root: &mut Ui) {
        let ctx = root.ctx().clone();
        if !self.styled {
            theme::install(&ctx);
            self.styled = true;
        }
        self.icons.load(&ctx);
        if self.app.poll_background() {
            self.reload();
        }
        if let Some(path) = self.app.take_pending_open() {
            self.open_path(&path);
        }
        if let Some(id) = self.app.take_pending_command() {
            self.command(&ctx, id);
        }
        if self.text_edited && self.edited_at.is_some_and(|t| t.elapsed() > Duration::from_millis(900)) {
            self.commit_text();
        }
        ctx.request_repaint_after(Duration::from_millis(self.app.background_poll_ms().max(50) as u64));
        if self.files.is_none() && self.input.is_none() && self.dialog.is_none() {
            self.handle_shortcuts(&ctx);
        }
        let p = theme::palette(&ctx);
        if self.home {
            self.home_ui(root, &p);
        } else {
            self.document_ui(root, &p);
        }
        self.overlays(&ctx, &p);
        self.scenes.end_frame();
    }

    // ----- shared widgets -------------------------------------------------------------

    fn icon_button(&self, ui: &mut Ui, icon: Icon, label: &str, enabled: bool, checked: bool, p: &Palette) -> Response {
        let size = Vec2::splat(30.0);
        let (rect, response) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
        let label = label.to_string();
        response.widget_info(|| {
            let mut info = WidgetInfo::labeled(Role::Button, enabled, &label);
            info.selected = checked.then_some(true);
            info
        });
        let bg = if checked {
            p.toggled
        } else if enabled && response.is_pointer_button_down_on() {
            p.pressed
        } else if enabled && response.hovered() {
            p.hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 4.0, bg);
        let color = if !enabled {
            p.icon.gamma_multiply(0.38)
        } else if checked {
            p.toggled_text
        } else {
            p.icon
        };
        self.icons.paint(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(18.0)), icon, color);
        response.on_hover_text(label)
    }

    fn round_icon_button(&self, ui: &mut Ui, icon: Icon, label: &str, p: &Palette) -> Response {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
        let label = label.to_string();
        response.widget_info(|| WidgetInfo::labeled(Role::Button, true, &label));
        if response.hovered() {
            ui.painter().circle_filled(rect.center(), 20.0, p.hover);
        }
        self.icons.paint(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(20.0)), icon, p.icon);
        response.on_hover_text(label)
    }

    fn menu_item(&self, ui: &mut Ui, icon: Option<Icon>, title: &str, shortcut: Option<&str>, enabled: bool, checked: bool) -> Response {
        let image = if checked {
            self.icons.image(Icon::Check, 16.0)
        } else if let Some(icon) = icon {
            self.icons.image(icon, 16.0)
        } else {
            self.icons.image(Icon::Check, 16.0).tint(Color32::TRANSPARENT)
        };
        let mut button = egui::Button::new((image, title))
            .image_tint_follows_text_color(true)
            .min_size(Vec2::new(ui.available_width().max(200.0), 30.0));
        if let Some(s) = shortcut {
            button = button.shortcut_text(RichText::new(s).size(12.5));
        }
        ui.add_enabled(enabled, button)
    }

    fn separator(ui: &mut Ui, p: &Palette, vertical: bool) {
        if vertical {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(13.0, 20.0), Sense::hover());
            ui.painter().vline(rect.center().x, rect.y_range(), Stroke::new(1.0, p.separator));
        } else {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 13.0), Sense::hover());
            ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, p.line));
        }
    }

    /// An accessible landmark over a laid-out region, so tests and screen readers find it by name.
    fn landmark(ui: &Ui, rect: Rect, role: Role, name: &str) {
        let response = ui.interact(rect, ui.scope_id().with(("landmark", name)), Sense::hover());
        let name = name.to_string();
        response.widget_info(|| WidgetInfo::labeled(role, true, &name));
    }

    // ----- home screen ----------------------------------------------------------------

    fn home_ui(&mut self, root: &mut Ui, p: &Palette) {
        let surface = if p.dark { p.bg } else { Color32::WHITE };
        let band = if p.dark { p.surface } else { Color32::from_rgb(0xf1, 0xf3, 0xf5) };
        egui::Panel::top("home-bar")
            .frame(Frame::new().fill(surface).inner_margin(Margin { left: 8, right: 16, top: 12, bottom: 12 }))
            .show_separator_line(false)
            .show(root, |ui| {
                let response = ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if self.round_icon_button(ui, Icon::Menu, &tr("Main menu"), p).clicked() {
                        self.drawer = true;
                        self.language = false;
                    }
                    let (logo, _) = ui.allocate_exact_size(Vec2::new(40.0, 40.0), Sense::hover());
                    icons::paint_logo(ui.painter(), logo.shrink(5.0), self.info.logo, self.info.accent);
                    ui.label(RichText::new(tr(self.info.title)).size(20.0).color(p.icon));
                    let right = 236.0;
                    let search_w = (ui.available_width() - right - 40.0).clamp(160.0, 620.0);
                    let gap = ((ui.available_width() - right - search_w) / 2.0).max(8.0);
                    ui.add_space(gap);
                    Frame::new()
                        .fill(if p.dark { p.toolbar } else { Color32::from_rgb(0xee, 0xf1, 0xf5) })
                        .corner_radius(23)
                        .inner_margin(Margin::symmetric(18, 12))
                        .show(ui, |ui| {
                            ui.set_width(search_w - 36.0);
                            ui.horizontal(|ui| {
                                ui.add(self.icons.image(Icon::Search, 20.0).tint(p.muted));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.search)
                                        .frame(egui::Frame::NONE)
                                        .hint_text(tr("Search recent files"))
                                        .desired_width(f32::INFINITY)
                                        .font(FontId::proportional(15.0)),
                                );
                            });
                        });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let mark = ui.label(RichText::new("off42").font(theme::semibold(19.0)).color(p.text));
                        let star = mark.rect.right_top() + Vec2::new(1.0, 2.0);
                        ui.painter().text(star, Align2::LEFT_TOP, "*", FontId::proportional(16.0), Color32::from_rgb(0xc2, 0x46, 0x2f));
                        ui.add_space(10.0);
                        if self.round_icon_button(ui, Icon::Help, &tr("About"), p).clicked() {
                            self.about = true;
                        }
                        let globe = self.round_icon_button(ui, Icon::Globe, &tr("Language"), p);
                        if globe.clicked() {
                            self.language = !self.language;
                        }
                        if self.language {
                            self.language_popup(ui.ctx(), globe.rect, p);
                        }
                    });
                });
                Self::landmark(ui, response.response.rect, Role::Banner, "Home bar");
                let r = ui.max_rect();
                ui.painter().hline(r.x_range(), r.bottom() + 12.0, Stroke::new(1.0, p.line));
            });
        egui::CentralPanel::default().frame(Frame::new().fill(surface)).show(root, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let full = ui.available_width();
                let width = (full - 32.0).min(984.0);
                let margin = ((full - width) / 2.0).max(16.0);
                // Create / open strip: a full-width band with the content centred in it.
                let band_rect = Rect::from_min_size(ui.cursor().min, Vec2::new(full, 312.0));
                ui.painter().rect_filled(band_rect, 0.0, band);
                let strip = ui.scope_builder(egui::UiBuilder::new().max_rect(Rect::from_min_size(band_rect.min + Vec2::new(margin, 20.0), Vec2::new(width, 272.0))), |ui| {
                    ui.horizontal(|ui| {
                        ui.set_height(44.0);
                        let heading = if self.app.supports_new() { tr("Create a document") } else { tr("Open a file") };
                        ui.label(RichText::new(heading).size(16.0).color(p.text));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(self.format_summary()).size(13.0).color(p.muted));
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 20.0;
                        if self.app.supports_new() && self.tile(ui, Icon::Plus, &tr("Blank document"), None, p).clicked() {
                            self.new_document();
                        }
                        if self.tile(ui, Icon::Upload, &tr("Open from this computer"), Some(&tr("Choose a file")), p).clicked() {
                            self.choose_open();
                        }
                    });
                });
                let _ = strip;
                ui.allocate_rect(band_rect, Sense::hover());
                Self::landmark(ui, band_rect, Role::Region, "Create and open");
                ui.add_space(22.0);
                let top = ui.cursor().min.y;
                ui.scope_builder(egui::UiBuilder::new().max_rect(Rect::from_min_size(Pos2::new(band_rect.left() + margin, top), Vec2::new(width, f32::INFINITY))), |ui| {
                    self.recent_list(ui, p);
                });
            });
        });
        if self.drawer {
            self.drawer_ui(root.ctx(), p);
        }
    }

    fn format_summary(&self) -> String {
        let mut names: Vec<String> = Vec::new();
        for f in self.app.open_formats().iter().chain(self.app.save_formats().iter()) {
            for e in f.extensions.iter().take(1) {
                let e = e.to_ascii_uppercase();
                if !names.contains(&e) && names.len() < 8 {
                    names.push(e);
                }
            }
        }
        names.join(", ")
    }

    fn tile(&self, ui: &mut Ui, icon: Icon, label: &str, hint: Option<&str>, p: &Palette) -> Response {
        let response = ui
            .vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 10.0;
                let (rect, response) = ui.allocate_exact_size(Vec2::new(156.0, 200.0), Sense::click());
                let fill = if response.hovered() { p.menu_hover } else { p.surface };
                ui.painter().rect(rect, 4.0, fill, Stroke::new(1.0, if p.dark { p.line } else { Color32::from_rgb(0xda, 0xdc, 0xe0) }), StrokeKind::Inside);
                let color = if icon == Icon::Plus { self.info.accent } else { p.icon };
                let size = if icon == Icon::Plus { 56.0 } else { 44.0 };
                let center = if hint.is_some() { rect.center() - Vec2::new(0.0, 12.0) } else { rect.center() };
                self.icons.paint(ui.painter(), Rect::from_center_size(center, Vec2::splat(size)), icon, color);
                if let Some(hint) = hint {
                    ui.painter().text(rect.center() + Vec2::new(0.0, 30.0), Align2::CENTER_CENTER, hint, FontId::proportional(12.0), p.muted);
                }
                ui.label(RichText::new(label).font(theme::semibold(14.0)).color(p.text));
                response
            })
            .inner;
        let label = label.to_string();
        response.widget_info(|| WidgetInfo::labeled(Role::Button, true, &label));
        response
    }

    fn recent_list(&mut self, ui: &mut Ui, p: &Palette) {
        let start = ui.cursor().min;
        {
            let (head, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::hover());
            let y = head.center().y;
            let painter = ui.painter();
            painter.text(Pos2::new(head.left() + 4.0, y), Align2::LEFT_CENTER, tr("Recent"), theme::semibold(16.0), p.text);
            painter.text(Pos2::new(head.right() - 360.0, y), Align2::LEFT_CENTER, tr("Modified"), FontId::proportional(14.0), p.icon);
            painter.text(Pos2::new(head.right() - 160.0, y), Align2::LEFT_CENTER, tr("Size"), FontId::proportional(14.0), p.icon);
        }
        let query = self.search.trim().to_lowercase();
        let items: Vec<PathBuf> = self
            .recent
            .iter()
            .filter(|path| query.is_empty() || display_name(path).to_lowercase().contains(&query))
            .cloned()
            .collect();
        let mut open = None;
        for path in &items {
            let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.0), Sense::click());
            ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0, p.line));
            if response.hovered() {
                ui.painter().rect_filled(rect.shrink2(Vec2::new(0.0, 1.0)), 24.0, p.menu_hover);
            }
            let name = display_name(path);
            let label = name.clone();
            response.widget_info(|| WidgetInfo::labeled(Role::Button, true, &label));
            let mark = Rect::from_min_size(Pos2::new(rect.left() + 4.0, rect.center().y - 11.0), Vec2::new(18.0, 22.0));
            ui.painter().rect_filled(mark, 2.0, self.info.accent);
            let x0 = rect.left() + 40.0;
            let painter = ui.painter_at(rect);
            painter.text(Pos2::new(x0, rect.center().y), Align2::LEFT_CENTER, &name, theme::semibold(14.0), p.text);
            let meta = std::fs::metadata(path).ok();
            let modified = meta.as_ref().and_then(|m| m.modified().ok()).map(format_time).unwrap_or_else(|| tr("Missing"));
            let size = meta.as_ref().map(|m| format_size(m.len())).unwrap_or_default();
            painter.text(Pos2::new(rect.right() - 360.0, rect.center().y), Align2::LEFT_CENTER, modified, FontId::proportional(14.0), p.muted);
            painter.text(Pos2::new(rect.right() - 160.0, rect.center().y), Align2::LEFT_CENTER, size, FontId::proportional(14.0), p.muted);
            if response.clicked() {
                open = Some(path.clone());
            }
            response.on_hover_text(path.display().to_string());
        }
        if items.is_empty() {
            ui.add_space(12.0);
            let text = if self.recent.is_empty() { tr("Files you open or save appear here.") } else { tr("No recent files match the search.") };
            ui.label(RichText::new(text).color(p.muted));
        }
        ui.add_space(22.0);
        ui.horizontal(|ui| {
            ui.add(self.icons.image(Icon::Lock, 16.0).tint(p.muted));
            ui.label(RichText::new(tr("Files open on this computer and never leave it. The recent list is stored only here.")).size(13.0).color(p.muted));
        });
        let rect = Rect::from_min_max(start, ui.cursor().min.max(start + Vec2::new(ui.available_width(), 40.0)));
        Self::landmark(ui, rect, Role::List, "Recent files");
        if let Some(path) = open {
            self.open_path(&path);
        }
    }

    fn language_popup(&mut self, ctx: &egui::Context, anchor: Rect, p: &Palette) {
        let current = localization::current();
        let mut chosen = None;
        let area = egui::Area::new(egui::Id::unique("off42-language"))
            .order(egui::Order::Foreground)
            .fixed_pos(Pos2::new(anchor.right() - 248.0, anchor.bottom() + 8.0))
            .show(ctx, |ui| {
                Frame::popup(ui.style()).inner_margin(Margin::symmetric(0, 8)).show(ui, |ui| {
                    ui.set_width(248.0);
                    theme::flat(ui);
                    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                        for language in localization::LANGUAGES {
                            let checked = language == current;
                            if self.menu_item(ui, None, language.name, None, true, checked).clicked() {
                                chosen = Some(language);
                            }
                        }
                    });
                    Self::separator(ui, p, false);
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        ui.label(RichText::new(tr("Applies after restart")).size(13.0).color(p.muted));
                    });
                });
            });
        if area.response.clicked_elsewhere() && !anchor.contains(ctx.input(|i| i.pointer.interact_pos().unwrap_or_default())) {
            self.language = false;
        }
        if let Some(language) = chosen {
            self.language = false;
            match localization::save(language) {
                Ok(()) if language == current => self.notify(format!("{}: {}", tr("Language"), language.name)),
                Ok(()) => self.notify(format!("{} {} — {}", tr("Language"), language.name, tr("restart to apply"))),
                Err(e) => self.snack = Some(Snack { text: e, error: true, at: Instant::now() }),
            }
        }
    }

    fn drawer_ui(&mut self, ctx: &egui::Context, p: &Palette) {
        let screen = ctx.content_rect();
        let backdrop = egui::Area::new(egui::Id::unique("off42-drawer-backdrop"))
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, response) = ui.allocate_exact_size(screen.size(), Sense::click());
                ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(82));
                response
            });
        if backdrop.inner.clicked() || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.drawer = false;
        }
        let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
        egui::Area::new(egui::Id::unique("off42-drawer"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                Frame::new().fill(p.surface).shadow(theme::shadow(p.dark)).show(ui, |ui| {
                    ui.set_width(300.0);
                    ui.set_height(screen.height());
                    let header = ui.horizontal(|ui| {
                        ui.set_height(64.0);
                        ui.add_space(24.0);
                        let mark = ui.label(RichText::new("off42").font(theme::semibold(22.0)).color(p.text));
                        ui.painter().text(mark.rect.right_top() + Vec2::new(1.0, 2.0), Align2::LEFT_TOP, "*", FontId::proportional(18.0), Color32::from_rgb(0xc2, 0x46, 0x2f));
                    });
                    ui.painter().hline(0.0..=300.0, header.response.rect.bottom(), Stroke::new(1.0, p.line));
                    ui.add_space(8.0);
                    let start = ui.cursor().min;
                    for app in theme::APPS {
                        let active = app.exe == self.info.exe;
                        let target = exe_dir.as_ref().and_then(|dir| sibling(dir, app.exe));
                        let (rect, response) = ui.allocate_exact_size(Vec2::new(288.0, 48.0), if target.is_some() || active { Sense::click() } else { Sense::hover() });
                        let (pill, _) = theme::pill(&app, p);
                        let fill = if active {
                            pill
                        } else if response.hovered() && target.is_some() {
                            p.menu_hover
                        } else {
                            Color32::TRANSPARENT
                        };
                        ui.painter().rect_filled(rect, CornerRadius { nw: 0, sw: 0, ne: 24, se: 24 }, fill);
                        let square = Rect::from_min_size(Pos2::new(rect.left() + 24.0, rect.center().y - 10.0), Vec2::splat(20.0));
                        ui.painter().rect_filled(square, 4.0, app.accent);
                        let color = if target.is_some() || active { p.text } else { p.muted };
                        let font = if active { theme::semibold(14.0) } else { FontId::proportional(14.0) };
                        ui.painter().text(Pos2::new(square.right() + 18.0, rect.center().y), Align2::LEFT_CENTER, tr(app.title), font, color);
                        let title = tr(app.title);
                        response.widget_info(|| {
                            let mut info = WidgetInfo::labeled(Role::Button, true, &title);
                            info.selected = Some(active);
                            info
                        });
                        if response.clicked() {
                            if active {
                                self.drawer = false;
                            } else if let Some(target) = &target {
                                match std::process::Command::new(target).spawn() {
                                    Ok(_) => self.drawer = false,
                                    Err(e) => self.snack = Some(Snack { text: e.to_string(), error: true, at: Instant::now() }),
                                }
                            }
                        }
                        if target.is_none() && !active {
                            response.on_hover_text(tr("Not installed next to this app"));
                        }
                    }
                    let list = Rect::from_min_max(start, Pos2::new(start.x + 300.0, ui.cursor().min.y));
                    Self::landmark(ui, list, Role::Navigation, "Apps");
                    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                        ui.add_space(16.0);
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.label(RichText::new(format!("off42 {} · {}", env!("CARGO_PKG_VERSION"), tr("Linux preview"))).size(12.0).color(p.muted));
                        });
                        ui.horizontal(|ui| {
                            ui.add_space(18.0);
                            theme::flat(ui);
                            if ui.add(egui::Button::new(RichText::new(tr("About")).size(12.0).color(p.select))).clicked() {
                                self.about = true;
                                self.drawer = false;
                            }
                        });
                        let r = ui.cursor();
                        ui.painter().hline(0.0..=300.0, r.bottom() - 4.0, Stroke::new(1.0, p.line));
                    });
                });
            });
    }

    // ----- document window ------------------------------------------------------------

    fn document_ui(&mut self, root: &mut Ui, p: &Palette) {
        let ctx = root.ctx().clone();
        let kind = self.kind();
        egui::Panel::top("title")
            .frame(Frame::new().fill(p.bg).inner_margin(Margin { left: 12, right: 16, top: 8, bottom: 0 }))
            .show_separator_line(false)
            .show(root, |ui| {
                let response = ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.vertical(|ui| {
                        ui.add_space(4.0);
                        let (rect, home) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
                        let label = format!("{} — {}", tr("Home"), tr(self.info.title));
                        home.widget_info(|| WidgetInfo::labeled(Role::Button, true, &label));
                        if home.hovered() {
                            ui.painter().circle_filled(rect.center(), 20.0, p.hover);
                        }
                        icons::paint_logo(ui.painter(), rect.shrink(6.0), self.info.logo, self.info.accent);
                        if home.on_hover_text(tr("Home")).clicked() {
                            self.commit_text();
                            self.home = true;
                            if self.persist {
                                self.recent = recents::load(self.app.name());
                            }
                        }
                    });
                    let right_width = 132.0;
                    ui.allocate_ui_with_layout(Vec2::new(ui.available_width() - right_width, 60.0), Layout::top_down(Align::Min), |ui| {
                        self.title_line(ui, p);
                        self.menu_bar(ui, p);
                    });
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        ui.add_space(0.0);
                        ui.vertical(|ui| {
                            ui.add_space(8.0);
                            self.save_pill(ui, p);
                        });
                    });
                });
                Self::landmark(ui, response.response.rect, Role::Banner, "Title bar");
            });
        egui::Panel::top("toolbar")
            .frame(Frame::new().fill(p.bg).inner_margin(Margin { left: 16, right: 16, top: 6, bottom: 8 }))
            .show_separator_line(false)
            .show(root, |ui| self.toolbar(ui, &ctx, kind, p));
        match kind {
            Kind::Table => {
                self.sheet_tabs(root, p);
                self.formula_bar(root, p);
                egui::CentralPanel::default().frame(Frame::new().fill(p.grid_cell)).show(root, |ui| self.grid(ui, p));
            }
            Kind::Slides => {
                self.slide_strip(root, p);
                egui::CentralPanel::default().frame(Frame::new().fill(p.bg)).show(root, |ui| self.slide_canvas(ui, p));
            }
            Kind::Image => {
                self.item_list(root, p);
                egui::CentralPanel::default().frame(Frame::new().fill(p.bg)).show(root, |ui| self.image_canvas(ui, p));
            }
            Kind::Text => {
                if self.app.name() == "PDF42" {
                    self.page_strip(root, p);
                } else {
                    self.item_list(root, p);
                }
                egui::CentralPanel::default().frame(Frame::new().fill(p.bg)).show(root, |ui| self.page(ui, p));
            }
        }
    }

    fn title_line(&mut self, ui: &mut Ui, p: &Palette) {
        ui.horizontal(|ui| {
            ui.set_height(30.0);
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.add_space(6.0);
            let name = ui.label(RichText::new(self.stem()).size(18.0).color(p.text));
            if let Some(path) = &self.path {
                name.on_hover_text(path.display().to_string());
            }
            let ext = self.path.as_deref().map(ext_of).unwrap_or_else(|| self.app.extension().to_ascii_lowercase());
            Frame::new()
                .stroke(Stroke::new(1.0, p.separator))
                .corner_radius(4)
                .inner_margin(Margin::symmetric(6, 1))
                .show(ui, |ui| {
                    ui.label(RichText::new(format!(".{ext}")).size(12.0).color(p.muted));
                });
            ui.add_space(6.0);
            let state = self.save_state();
            ui.add(egui::Label::new(RichText::new(&state).size(13.0).color(p.muted)).truncate());
        });
    }

    /// The human save state shown in the title row; app status details never go here.
    pub(crate) fn save_state(&self) -> String {
        if self.app.dirty() || self.text_edited {
            tr("Edited")
        } else if self.path.is_some() {
            tr("Saved on this computer")
        } else {
            tr("Not saved yet")
        }
    }

    /// The readable part of `App::status` for the status chip: its first field (word count,
    /// page or slide position, picture size), or nothing when that field looks like debug output.
    /// Spreadsheets show the sheet and cell in the tabs and the formula bar instead.
    pub(crate) fn status_summary(&self) -> Option<String> {
        if self.home || matches!(self.kind(), Kind::Table) {
            return None;
        }
        let status = self.app.status();
        let first = status.split('|').next().unwrap_or_default().trim().to_string();
        let debug = first.is_empty() || first.contains(['[', ']', '{', '}', '=']) || first.ends_with(':');
        (!debug).then_some(first)
    }

    fn menu_bar(&mut self, ui: &mut Ui, p: &Palette) {
        let ctx = ui.ctx().clone();
        let config = egui::containers::menu::MenuConfig::new().style(|style: &mut egui::Style| {
            egui::containers::menu::menu_style(style);
            style.spacing.button_padding = Vec2::new(14.0, 6.0);
            style.spacing.item_spacing.y = 0.0;
            style.spacing.menu_margin = Margin::symmetric(0, 6);
        });
        let bar = egui::MenuBar::new().config(config).style(|style: &mut egui::Style| {
            egui::containers::menu::menu_style(style);
            style.spacing.button_padding = Vec2::new(8.0, 4.0);
            style.spacing.item_spacing.x = 0.0;
        });
        let response = bar.ui(ui, |ui| {
            ui.set_height(30.0);
            ui.menu_button(tr("File"), |ui| self.file_menu(ui, &ctx, p));
            ui.menu_button(tr("Edit"), |ui| self.edit_menu(ui, &ctx, p));
            ui.menu_button(tr("View"), |ui| self.view_menu(ui, &ctx, p));
            let commands = self.app_commands();
            let mut groups: Vec<String> = Vec::new();
            for (g, ..) in &commands {
                if g != "Edit" && g != "View" && !groups.contains(g) {
                    groups.push(g.clone());
                }
            }
            let imports: Vec<_> = self.app.file_actions().iter().filter(|a| !a.save && a.title.trim_start_matches('&').starts_with("Insert")).copied().collect();
            if !imports.is_empty() && !groups.iter().any(|g| g == "Insert") {
                groups.insert(0, "Insert".into());
            }
            for group in groups {
                ui.menu_button(tr(&group), |ui| {
                    if group == "Insert" {
                        for action in &imports {
                            if self.menu_item(ui, icons::for_command(action.title), &clean(action.title), None, true, false).clicked() {
                                self.file_action(action.id);
                                ui.close();
                            }
                        }
                    }
                    self.command_items(ui, &ctx, &commands, &group);
                });
            }
            ui.menu_button(tr("Help"), |ui| {
                if self.menu_item(ui, Some(Icon::Info), &format!("{} {}", tr("About"), self.app.name()), None, true, false).clicked() {
                    self.about = true;
                    ui.close();
                }
            });
        });
        Self::landmark(ui, response.response.rect, Role::MenuBar, "Menu bar");
    }

    fn command_items(&mut self, ui: &mut Ui, ctx: &egui::Context, commands: &[(String, u16, String, Option<String>)], group: &str) {
        let items: Vec<_> = commands.iter().filter(|c| c.0 == group).cloned().collect();
        let scroll = items.len() > 18;
        let mut body = |ui: &mut Ui| {
            for (_, id, title, acc) in &items {
                if self.is_print(*id, title) {
                    continue;
                }
                let available = self.available(*id);
                let state = self.app.command_state(*id);
                let enabled = available && self.command_enabled(*id);
                let icon = icons::for_command(&title.replace('&', ""));
                let r = self.menu_item(ui, icon, &clean(title), acc.as_deref(), enabled, state.checked);
                let r = if available { r } else { r.on_disabled_hover_text(tr("Not available on Linux yet")) };
                if r.clicked() {
                    self.command(ctx, *id);
                    ui.close();
                }
            }
        };
        if scroll {
            egui::ScrollArea::vertical().max_height(ctx.content_rect().height() - 160.0).show(ui, |ui| body(ui));
        } else {
            body(ui);
        }
    }

    fn file_menu(&mut self, ui: &mut Ui, _ctx: &egui::Context, p: &Palette) {
        if self.app.supports_new() && self.menu_item(ui, Some(Icon::NewFile), &tr("New"), Some("Ctrl+N"), true, false).clicked() {
            self.new_document();
            ui.close();
        }
        if self.menu_item(ui, Some(Icon::Folder), &tr("Open…"), Some("Ctrl+O"), true, false).clicked() {
            self.choose_open();
            ui.close();
        }
        if !self.recent.is_empty() {
            let recent = self.recent.clone();
            ui.menu_button(tr("Open recent"), |ui| {
                for path in recent {
                    if self.menu_item(ui, None, &display_name(&path), None, path.exists(), false).on_hover_text(path.display().to_string()).clicked() {
                        self.open_path(&path);
                        ui.close();
                    }
                }
            });
        }
        Self::separator(ui, p, false);
        let save_label = format!("{} (.{})", tr("Save"), self.default_save_extension());
        if self.menu_item(ui, Some(Icon::Download), &save_label, Some("Ctrl+S"), self.app.can_save(), false).clicked() {
            self.save();
            ui.close();
        }
        let formats = self.app.save_formats();
        let exports: Vec<_> = self.app.file_actions().iter().filter(|a| a.save).copied().collect();
        ui.add_enabled_ui(self.app.can_save() || !exports.is_empty(), |ui| {
            ui.menu_button(tr("Save as"), |ui| {
                let mut native_listed = false;
                for f in &formats {
                    let ext = f.extensions.first().copied().unwrap_or_default();
                    native_listed |= ext.eq_ignore_ascii_case(self.app.extension());
                    if self.menu_item(ui, None, &tr(f.title), Some(&format!(".{ext}")), self.app.can_save(), false).clicked() {
                        self.choose_save(ext.to_string());
                        ui.close();
                    }
                }
                if !native_listed {
                    let ext = self.app.extension();
                    if self.menu_item(ui, None, &tr(self.app.format_name()), Some(&format!(".{ext}")), self.app.can_save(), false).clicked() {
                        self.choose_save(ext.to_string());
                        ui.close();
                    }
                }
                if !exports.is_empty() {
                    Self::separator(ui, p, false);
                    for action in &exports {
                        let enabled = self.app.enabled(action.id);
                        if self.menu_item(ui, None, &clean(action.title), Some(&format!(".{}", action.extension)), enabled, false).clicked() {
                            self.file_action(action.id);
                            ui.close();
                        }
                    }
                }
            });
        });
        let imports: Vec<_> = self.app.file_actions().iter().filter(|a| !a.save && !a.title.trim_start_matches('&').starts_with("Insert")).copied().collect();
        if !imports.is_empty() {
            ui.menu_button(tr("Import"), |ui| {
                for action in &imports {
                    if self.menu_item(ui, None, &clean(action.title), Some(&format!(".{}", action.extension)), self.app.enabled(action.id), false).clicked() {
                        self.file_action(action.id);
                        ui.close();
                    }
                }
            });
        }
        Self::separator(ui, p, false);
        if self.menu_item(ui, Some(Icon::Close), &tr("Close"), Some("Ctrl+Q"), true, false).clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            ui.close();
        }
    }

    fn edit_menu(&mut self, ui: &mut Ui, ctx: &egui::Context, p: &Palette) {
        if self.menu_item(ui, Some(Icon::Undo), &tr("Undo"), Some("Ctrl+Z"), self.app.can_undo(), false).clicked() {
            self.command(ctx, UNDO);
            ui.close();
        }
        if self.menu_item(ui, Some(Icon::Redo), &tr("Redo"), Some("Ctrl+Y"), self.app.can_redo(), false).clicked() {
            self.command(ctx, REDO);
            ui.close();
        }
        let commands = self.app_commands();
        if commands.iter().any(|c| c.0 == "Edit") {
            Self::separator(ui, p, false);
            self.command_items(ui, ctx, &commands, "Edit");
        }
    }

    fn view_menu(&mut self, ui: &mut Ui, ctx: &egui::Context, p: &Palette) {
        let commands = self.app_commands();
        if commands.iter().any(|c| c.0 == "View") {
            self.command_items(ui, ctx, &commands, "View");
            Self::separator(ui, p, false);
        }
        if self.menu_item(ui, Some(Icon::ZoomIn), &tr("Zoom in"), Some("Ctrl++"), true, false).clicked() {
            self.step_zoom(1);
            ui.close();
        }
        if self.menu_item(ui, Some(Icon::ZoomOut), &tr("Zoom out"), Some("Ctrl+-"), true, false).clicked() {
            self.step_zoom(-1);
            ui.close();
        }
        if self.menu_item(ui, None, &tr("Actual size"), Some("Ctrl+0"), true, self.zoom == Some(1.0)).clicked() {
            self.zoom = Some(1.0);
            ui.close();
        }
        Self::separator(ui, p, false);
        ui.menu_button(tr("Appearance"), |ui| {
            let pref = ctx.options(|o| o.theme_preference);
            for (label, value) in [("System", egui::ThemePreference::System), ("Light", egui::ThemePreference::Light), ("Dark", egui::ThemePreference::Dark)] {
                if self.menu_item(ui, None, &tr(label), None, true, pref == value).clicked() {
                    ctx.set_theme(value);
                    ui.close();
                }
            }
        });
        ui.menu_button(tr("Language"), |ui| {
            let current = localization::current();
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                for language in localization::LANGUAGES {
                    if self.menu_item(ui, None, language.name, None, true, language == current).clicked() {
                        match localization::save(language) {
                            Ok(()) => self.notify(format!("{} {} — {}", tr("Language"), language.name, tr("restart to apply"))),
                            Err(e) => self.snack = Some(Snack { text: e, error: true, at: Instant::now() }),
                        }
                        ui.close();
                    }
                }
            });
        });
    }

    fn step_zoom(&mut self, dir: i32) {
        const STEPS: [f32; 9] = [0.5, 0.67, 0.75, 0.9, 1.0, 1.25, 1.5, 2.0, 3.0];
        let current = self.zoom.unwrap_or(1.0);
        let i = STEPS.iter().position(|z| *z >= current - 0.001).unwrap_or(4) as i32;
        let next = (i + dir).clamp(0, STEPS.len() as i32 - 1) as usize;
        self.zoom = Some(STEPS[next]);
    }

    fn save_pill(&mut self, ui: &mut Ui, p: &Palette) {
        let (fill, text) = theme::pill(&self.info, p);
        let label = tr("Save");
        let galley = ui.painter().layout_no_wrap(label.clone(), theme::semibold(14.0), text);
        let size = Vec2::new(16.0 + 18.0 + 8.0 + galley.size().x + 20.0, 40.0);
        let enabled = self.app.can_save();
        let (rect, response) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
        response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, &label));
        let fill = if !enabled {
            fill.gamma_multiply(0.5)
        } else if response.hovered() {
            theme::mix(fill, text, 0.08)
        } else {
            fill
        };
        ui.painter().rect_filled(rect, 20.0, fill);
        self.icons.paint(ui.painter(), Rect::from_min_size(Pos2::new(rect.left() + 16.0, rect.center().y - 9.0), Vec2::splat(18.0)), Icon::Download, text);
        ui.painter().galley(Pos2::new(rect.left() + 42.0, rect.center().y - galley.size().y / 2.0), galley, text);
        if response.on_hover_text(format!("{} (Ctrl+S)", tr("Save"))).clicked() {
            self.save();
        }
    }

    fn toolbar(&mut self, ui: &mut Ui, ctx: &egui::Context, kind: Kind, p: &Palette) {
        let frame = Frame::new().fill(p.toolbar).corner_radius(20).inner_margin(Margin::symmetric(10, 5));
        let response = frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(30.0);
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                theme::flat(ui);
                let undo = self.icon_button(ui, Icon::Undo, &tr("Undo"), self.app.can_undo(), false, p);
                if undo.clicked() {
                    self.command(ctx, UNDO);
                }
                if self.icon_button(ui, Icon::Redo, &tr("Redo"), self.app.can_redo(), false, p).clicked() {
                    self.command(ctx, REDO);
                }
                Self::separator(ui, p, true);
                self.zoom_combo(ui, kind, p);
                let commands = self.toolbar_commands();
                let mut width_left = ui.available_width() - 40.0;
                let mut overflow = Vec::new();
                for (id, title, icon, new_group) in commands {
                    let w = 32.0 + if new_group { 13.0 } else { 0.0 };
                    if w > width_left || !overflow.is_empty() {
                        overflow.push((id, title, icon));
                        continue;
                    }
                    width_left -= w;
                    if new_group {
                        Self::separator(ui, p, true);
                    }
                    let enabled = self.command_enabled(id);
                    let checked = self.app.command_state(id).checked;
                    if self.icon_button(ui, icon, &title, enabled, checked, p).clicked() {
                        self.command(ctx, id);
                    }
                }
                if !overflow.is_empty() {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let more = egui::containers::menu::MenuButton::from_button(egui::Button::image(self.icons.image(Icon::More, 18.0).tint(p.icon))).ui(ui, |ui| {
                            for (id, title, icon) in &overflow {
                                if self.menu_item(ui, Some(*icon), title, None, self.command_enabled(*id), self.app.command_state(*id).checked).clicked() {
                                    self.command(ctx, *id);
                                    ui.close();
                                }
                            }
                        });
                        more.0.widget_info(|| WidgetInfo::labeled(Role::Button, true, "More tools"));
                        more.0.on_hover_text(tr("More"));
                    });
                }
            });
        });
        Self::landmark(ui, response.response.rect, Role::Toolbar, "Toolbar");
    }

    fn zoom_combo(&mut self, ui: &mut Ui, kind: Kind, p: &Palette) {
        let label = match self.zoom {
            None => tr("Fit"),
            Some(z) => format!("{}%", (z * 100.0).round()),
        };
        let mut chosen = None;
        let icons = &self.icons;
        let combo = egui::ComboBox::from_id_salt("zoom")
            .selected_text(RichText::new(label).color(p.icon))
            .width(70.0)
            .icon(|_, _, _, _| {})
            .show_ui(ui, |ui| {
                if matches!(kind, Kind::Image | Kind::Slides) && ui.selectable_label(self.zoom.is_none(), tr("Fit")).clicked() {
                    chosen = Some(None);
                }
                for z in [0.5f32, 0.75, 0.9, 1.0, 1.25, 1.5, 2.0] {
                    if ui.selectable_label(self.zoom == Some(z), format!("{}%", (z * 100.0).round())).clicked() {
                        chosen = Some(Some(z));
                    }
                }
            });
        // Chevron drawn with the suite's stroke icon, inset from the right edge.
        let r = combo.response.rect;
        icons.paint(ui.painter(), Rect::from_center_size(Pos2::new(r.right() - 12.0, r.center().y), Vec2::splat(14.0)), Icon::ChevronDown, p.icon);
        let z = self.zoom;
        combo.response.widget_info(|| WidgetInfo::labeled(Role::ComboBox, true, format!("Zoom {}", z.map_or("fit".into(), |z| format!("{}%", (z * 100.0).round())))));
        if let Some(z) = chosen {
            self.zoom = z;
        }
    }

    // ----- canvases -------------------------------------------------------------------

    fn page(&mut self, ui: &mut Ui, p: &Palette) {
        let zoom = self.zoom.unwrap_or(1.0);
        let readonly = self.app.text_readonly();
        let page_w = 816.0 * zoom;
        let page_h = 1056.0 * zoom;
        let (mx, my) = (96.0 * zoom, 84.0 * zoom);
        let mut changed = false;
        let mut lost = false;
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(20.0);
            let avail = ui.available_width();
            let left = ((avail - page_w) / 2.0).max(16.0);
            ui.horizontal(|ui| {
                ui.add_space(left);
                let pdf_pages = readonly && self.app.name() == "PDF42" && !self.app.items().is_empty();
                let frame = ui.vertical(|ui| {
                    // Paper grows with its text; at least one page tall.
                    let top = ui.cursor().min;
                    let paper = ui.painter().add(egui::Shape::Noop);
                    let content = ui.scope_builder(
                        egui::UiBuilder::new().max_rect(Rect::from_min_size(top + Vec2::new(mx, my), Vec2::new(page_w - 2.0 * mx, f32::INFINITY))),
                        |ui| {
                        ui.set_width(page_w - 2.0 * mx);
                        ui.set_min_height(page_h - 2.0 * my);
                        let v = ui.visuals_mut();
                        v.text_cursor.stroke = Stroke::new(1.5, p.paper_text);
                        v.selection.bg_fill = Color32::from_rgb(0xc9, 0xdc, 0xfb);
                        v.override_text_color = Some(p.paper_text);
                        if pdf_pages {
                            let (rect, _) = ui.allocate_exact_size(Vec2::new(page_w - 2.0 * mx, page_h - 2.0 * my), Sense::hover());
                            let page = self.app.items().get(self.app.selected()).cloned().unwrap_or_default();
                            let anchor = Rect::from_center_size(Pos2::new(rect.center().x, rect.top() + 220.0 * zoom), Vec2::new(rect.width(), 40.0));
                            ui.painter().text(anchor.center() - Vec2::new(0.0, 12.0), Align2::CENTER_CENTER, page, theme::semibold(16.0), p.paper_text);
                            scene::centered_text(ui.painter(), anchor.translate(Vec2::new(0.0, 14.0)),&tr("Page images need PDFium, which the Linux preview does not include yet"), Color32::from_rgb(0x5b, 0x5f, 0x66));
                        } else if readonly {
                            ui.add(egui::Label::new(RichText::new(&self.text).size(15.0 * zoom).color(p.paper_text)).wrap());
                        } else {
                            let edit = egui::TextEdit::multiline(&mut self.text)
                                .id_salt("document-text")
                                .frame(egui::Frame::NONE)
                                .font(FontId::proportional(15.0 * zoom))
                                .text_color(p.paper_text)
                                .desired_width(f32::INFINITY)
                                .desired_rows(((page_h - 2.0 * my) / (22.0 * zoom)) as usize)
                                .lock_focus(true);
                            let response = ui.add(edit);
                            response.widget_info(|| WidgetInfo::labeled(Role::MultilineTextInput, true, "Document text"));
                            changed = response.changed();
                            lost = response.lost_focus();
                        }
                    });
                    let bottom = (content.response.rect.bottom() + my).max(top.y + page_h);
                    let rect = Rect::from_min_max(top, Pos2::new(top.x + page_w, bottom));
                    ui.painter().set(
                        paper,
                        egui::Shape::Vec(vec![theme::paper_shadow(p.dark).as_shape(rect, 0.0).into(), egui::Shape::rect_filled(rect, 0.0, p.paper)]),
                    );
                    ui.allocate_rect(rect, Sense::hover());
                    rect
                });
                Self::landmark(ui, frame.inner, Role::Document, "Page");
            });
            ui.add_space(32.0);
        });
        if changed {
            self.text_edited = true;
            self.edited_at = Some(Instant::now());
        }
        if lost {
            self.commit_text();
        }
    }

    fn image_canvas(&mut self, ui: &mut Ui, p: &Palette) {
        let ctx = ui.ctx().clone();
        if let Some((w, h, pixels)) = self.app.image() {
            if self.texture.is_none() && pixels.len() == (w as usize) * (h as usize) * 4 {
                let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
                self.texture = Some(ctx.load_texture(format!("doc-{}", self.image_stamp), image, Default::default()));
            }
        }
        let Some(texture) = self.texture.clone() else { return };
        let size = texture.size_vec2();
        let area = ui.available_size() - Vec2::splat(48.0);
        let scale = match self.zoom {
            None => (area.x / size.x).min(area.y / size.y).min(1.0),
            Some(z) => z,
        };
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            let shown = size * scale;
            let full = Vec2::new(shown.x.max(ui.available_width() - 48.0), shown.y.max(ui.available_height() - 48.0)) + Vec2::splat(48.0);
            let (outer, _) = ui.allocate_exact_size(full, Sense::hover());
            let rect = Rect::from_center_size(outer.center(), shown);
            let shadow = theme::paper_shadow(p.dark).as_shape(rect, 0.0);
            ui.painter().add(shadow);
            ui.painter().image(texture.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            Self::landmark(ui, rect, Role::Image, "Picture");
        });
    }

    fn slide_canvas(&mut self, ui: &mut Ui, p: &Palette) {
        let notes_h = 120.0;
        let full = ui.available_rect_before_wrap();
        let canvas = Rect::from_min_max(full.min, Pos2::new(full.max.x, full.max.y - notes_h));
        if let Some(scene) = self.app.canvas_scene() {
            let size = scene::scene_size(&scene);
            let area = canvas.shrink2(Vec2::new(32.0, 20.0));
            let fit = (area.width() / size.x).min(area.height() / size.y);
            let scale = self.zoom.unwrap_or(fit);
            let rect = Rect::from_center_size(area.center(), size * scale);
            ui.painter().add(theme::paper_shadow(p.dark).as_shape(rect, 0.0));
            ui.painter().rect_filled(rect, 0.0, p.paper);
            scene::paint(ui, &mut self.scenes, &scene, rect);
            Self::landmark(ui, rect, Role::Document, "Slide");
        }
        // Slide text below the slide: first line is the title, the rest the body.
        let notes = Rect::from_min_max(Pos2::new(full.min.x, full.max.y - notes_h), full.max);
        ui.painter().rect_filled(notes, 0.0, if p.dark { p.surface } else { Color32::WHITE });
        ui.painter().hline(notes.x_range(), notes.top(), Stroke::new(1.0, p.line));
        let mut changed = false;
        let mut lost = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(notes.shrink2(Vec2::new(32.0, 12.0))), |ui| {
            ui.label(RichText::new(tr("Slide text — first line is the title")).size(12.0).color(p.muted));
            egui::ScrollArea::vertical().show(ui, |ui| {
                let response = ui.add(
                    egui::TextEdit::multiline(&mut self.text)
                        .id_salt("slide-text")
                        .frame(egui::Frame::NONE)
                        .desired_width(f32::INFINITY)
                        .desired_rows(3)
                        .text_color(p.text),
                );
                response.widget_info(|| WidgetInfo::labeled(Role::MultilineTextInput, true, "Slide text"));
                changed = response.changed();
                lost = response.lost_focus();
            });
        });
        if changed {
            self.text_edited = true;
            self.edited_at = Some(Instant::now());
        }
        if lost {
            self.commit_text();
            self.reload();
        }
    }

    fn slide_strip(&mut self, root: &mut Ui, p: &Palette) {
        let items = self.app.items();
        let selected = self.app.selected();
        let mut clicked = None;
        let panel = egui::Panel::left("slides")
            .exact_size(212.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(p.bg).inner_margin(Margin { left: 0, right: 12, top: 8, bottom: 8 }))
            .show(root, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 14.0;
                    for (i, item) in items.iter().enumerate().take(2000) {
                        let (row, response) = ui.allocate_exact_size(Vec2::new(200.0, 92.0), Sense::click());
                        let number = Pos2::new(row.left() + 22.0, row.top() + 2.0);
                        ui.painter().text(number, Align2::RIGHT_TOP, (i + 1).to_string(), FontId::proportional(12.0), p.muted);
                        let card = Rect::from_min_size(Pos2::new(row.left() + 30.0, row.top()), Vec2::new(164.0, 92.0));
                        let ring = if i == selected { Stroke::new(2.0, self.info.accent) } else { Stroke::new(1.0, if p.dark { p.line } else { Color32::from_rgb(0xda, 0xdc, 0xe0) }) };
                        ui.painter().rect_filled(card, 6.0, p.paper);
                        let preview = (card.is_positive() && ui.is_rect_visible(card)).then(|| self.app.presenter_preview(i)).flatten();
                        match preview {
                            Some(scene) => {
                                let size = scene::scene_size(&scene);
                                let s = (card.width() / size.x).min(card.height() / size.y);
                                let inner = Rect::from_center_size(card.center(), size * s);
                                let clip = ui.new_child(egui::UiBuilder::new().max_rect(card));
                                scene::paint(&clip, &mut self.scenes, &scene, inner);
                            }
                            None => {
                                let title = item.split_once(": ").map_or(item.as_str(), |(_, t)| t);
                                ui.painter().text(card.left_top() + Vec2::new(12.0, 10.0), Align2::LEFT_TOP, title, theme::semibold(9.0), p.paper_text);
                            }
                        }
                        ui.painter().rect_stroke(card, 6.0, ring, StrokeKind::Outside);
                        let label = format!("{} {}", tr("Slide"), i + 1);
                        response.widget_info(|| {
                            let mut info = WidgetInfo::labeled(Role::Button, true, &label);
                            info.selected = Some(i == selected);
                            info
                        });
                        if response.clicked() {
                            clicked = Some(i);
                        }
                    }
                });
            });
        let r = panel.response.rect;
        root.painter().vline(r.right(), r.y_range(), Stroke::new(1.0, p.line));
        Self::landmark(root, r, Role::List, "Slides");
        if let Some(i) = clicked {
            self.commit_text();
            self.app.select(i);
            self.reload();
        }
    }

    fn page_strip(&mut self, root: &mut Ui, p: &Palette) {
        let items = self.app.items();
        if items.is_empty() {
            return;
        }
        let selected = self.app.selected();
        let mut clicked = None;
        let panel = egui::Panel::left("pages")
            .exact_size(180.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(p.bg).inner_margin(Margin { left: 12, right: 12, top: 16, bottom: 12 }))
            .show(root, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 12.0;
                    for (i, item) in items.iter().enumerate().take(5000) {
                        let (row, response) = ui.allocate_exact_size(Vec2::new(156.0, 150.0), Sense::click());
                        let card = Rect::from_center_size(Pos2::new(row.center().x, row.top() + 64.0), Vec2::new(96.0, 124.0));
                        ui.painter().add(theme::paper_shadow(p.dark).as_shape(card, 0.0));
                        ui.painter().rect_filled(card, 0.0, p.paper);
                        for k in 0..6 {
                            let y = card.top() + 18.0 + k as f32 * 12.0;
                            let w = if k == 5 { 40.0 } else { 66.0 };
                            ui.painter().hline(card.left() + 15.0..=card.left() + 15.0 + w, y, Stroke::new(2.0, Color32::from_rgb(0xe3, 0xe6, 0xea)));
                        }
                        if i == selected {
                            ui.painter().rect_stroke(card.expand(3.0), 4.0, Stroke::new(2.0, self.info.accent), StrokeKind::Outside);
                        }
                        let label = item.split(" — ").next().unwrap_or(item).to_string();
                        ui.painter().text(Pos2::new(row.center().x, row.bottom() - 10.0), Align2::CENTER_CENTER, &label, FontId::proportional(12.0), if i == selected { p.text } else { p.muted });
                        let name = label.clone();
                        response.widget_info(|| {
                            let mut info = WidgetInfo::labeled(Role::Button, true, &name);
                            info.selected = Some(i == selected);
                            info
                        });
                        if response.on_hover_text(item).clicked() {
                            clicked = Some(i);
                        }
                    }
                });
            });
        let r = panel.response.rect;
        root.painter().vline(r.right(), r.y_range(), Stroke::new(1.0, p.line));
        Self::landmark(root, r, Role::List, "Pages");
        if let Some(i) = clicked {
            self.app.select(i);
            self.reload();
        }
    }

    fn item_list(&mut self, root: &mut Ui, p: &Palette) {
        if !self.app.list_visible() {
            return;
        }
        let items = self.app.items();
        let selected = self.app.selected();
        let mut clicked = None;
        let panel = egui::Panel::left("items")
            .default_size(220.0)
            .resizable(true)
            .show_separator_line(false)
            .frame(Frame::new().fill(p.bg).inner_margin(Margin::symmetric(8, 8)))
            .show(root, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    theme::flat(ui);
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (i, item) in items.iter().enumerate().take(5000) {
                        let on = i == selected;
                        let text = RichText::new(item).color(if on { p.toggled_text } else { p.text });
                        let button = egui::Button::new(text).fill(if on { p.toggled } else { Color32::TRANSPARENT }).corner_radius(16).min_size(Vec2::new(ui.available_width(), 32.0)).truncate();
                        if ui.add(button).clicked() {
                            clicked = Some(i);
                        }
                    }
                });
            });
        let r = panel.response.rect;
        root.painter().vline(r.right(), r.y_range(), Stroke::new(1.0, p.line));
        Self::landmark(root, r, Role::List, "Side list");
        if let Some(i) = clicked {
            self.commit_text();
            self.app.select(i);
            self.reload();
        }
    }

    // ----- spreadsheet ----------------------------------------------------------------

    fn formula_bar(&mut self, root: &mut Ui, p: &Palette) {
        let cell = self.app.selected_cell();
        let readonly = self.app.text_readonly();
        egui::Panel::top("formula")
            .exact_size(31.0)
            .show_separator_line(false)
            .frame(Frame::new().fill(p.grid_cell))
            .show(root, |ui| {
                let rect = ui.max_rect();
                ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0, p.grid_line));
                ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, p.grid_line));
                let name_box = Rect::from_min_size(rect.min, Vec2::new(84.0, rect.height()));
                let fx = Rect::from_min_size(Pos2::new(name_box.right(), rect.top()), Vec2::new(44.0, rect.height()));
                ui.painter().vline(name_box.right(), rect.y_range(), Stroke::new(1.0, p.grid_line));
                ui.painter().vline(fx.right(), rect.y_range(), Stroke::new(1.0, p.grid_line));
                let reference = cell.map(|(r, c)| format!("{}{}", column_name(c), r + 1)).unwrap_or_default();
                ui.painter().text(Pos2::new(name_box.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, &reference, FontId::proportional(13.0), p.text);
                Self::landmark(ui, name_box, Role::Label, &format!("Cell {reference}"));
                ui.painter().text(fx.center(), Align2::CENTER_CENTER, "fx", FontId::proportional(14.0), p.muted);
                let edit_rect = Rect::from_min_max(Pos2::new(fx.right() + 10.0, rect.top() + 4.0), Pos2::new(rect.right() - 8.0, rect.bottom() - 4.0));
                let mut commit = false;
                ui.scope_builder(egui::UiBuilder::new().max_rect(edit_rect), |ui| {
                    let response = ui.add_enabled(
                        !readonly,
                        egui::TextEdit::singleline(&mut self.text)
                            .id_salt("formula")
                            .frame(egui::Frame::NONE)
                            .font(FontId::monospace(13.0))
                            .desired_width(f32::INFINITY),
                    );
                    response.widget_info(|| WidgetInfo::labeled(Role::TextInput, !readonly, "Formula bar"));
                    if response.changed() {
                        self.text_edited = true;
                    }
                    if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        commit = true;
                    }
                });
                if commit && self.text_edited {
                    self.text_edited = false;
                    let result = self.app.edit(suite42_app::to_native_text(&self.text));
                    match result {
                        Ok(()) => self.reload(),
                        Err(e) => self.report(Err(e), ""),
                    }
                }
            });
    }

    fn grid(&mut self, ui: &mut Ui, p: &Palette) {
        let zoom = self.zoom.unwrap_or(1.0);
        let sparse = self.app.sparse_table();
        let dense = if sparse.is_none() { table_rows(self.app.as_ref(), 5000, 200) } else { None };
        let (data_rows, data_cols) = match (&sparse, &dense) {
            (Some(s), _) => (s.len(), s.columns()),
            (None, Some(d)) => (d.len(), d.iter().map(Vec::len).max().unwrap_or(0)),
            _ => (0, 0),
        };
        let rows = (data_rows + 50).max(100).min(100_000);
        let cols = (data_cols + 4).max(26).min(702);
        let cell = |r: usize, c: usize| -> &str {
            match (&sparse, &dense) {
                (Some(s), _) if r < s.len() && c < s.columns() => s.cell(r, c),
                (None, Some(d)) => d.get(r).and_then(|row| row.get(c)).map_or("", String::as_str),
                _ => "",
            }
        };
        let custom = self.app.uses_custom_grid();
        let widths: Vec<f32> = (0..cols)
            .map(|c| if custom { f32::from(self.app.grid_column_width(c)).max(24.0) } else { 100.0 } * zoom)
            .collect();
        let row_h = 24.0 * zoom;
        let head_w = 46.0;
        let selected = self.app.selected_cell();
        let range = self.app.grid_selection();
        let mut click: Option<(usize, usize, bool)> = None;
        let total_w = head_w + widths.iter().sum::<f32>();
        let font = FontId::proportional(13.0 * zoom);
        let line = Stroke::new(1.0, p.grid_line);
        let area = egui::ScrollArea::both().auto_shrink([false, false]).show_viewport(ui, |ui, viewport| {
            let (rect, response) = ui.allocate_exact_size(Vec2::new(total_w, row_h * (rows + 1) as f32), Sense::click());
            let origin = rect.min;
            let painter = ui.painter();
            let first_row = ((viewport.min.y / row_h).floor() as usize).saturating_sub(1).min(rows);
            let last_row = (((viewport.max.y / row_h).ceil() as usize) + 1).min(rows);
            // Column x positions.
            let mut xs = Vec::with_capacity(cols + 1);
            let mut x = head_w;
            for w in &widths {
                xs.push(x);
                x += w;
            }
            xs.push(x);
            let first_col = xs.iter().position(|x| *x + 200.0 * zoom >= viewport.min.x).unwrap_or(0).min(cols);
            let last_col = xs.iter().position(|x| *x > viewport.max.x).unwrap_or(cols).min(cols);
            let in_range = |r: usize, c: usize| range.is_some_and(|g| r >= g.top && r <= g.bottom && c >= g.left && c <= g.right);
            for r in first_row..last_row {
                let y = origin.y + row_h * (r + 1) as f32;
                for c in first_col..last_col {
                    let cell_rect = Rect::from_min_max(Pos2::new(origin.x + xs[c], y), Pos2::new(origin.x + xs[c + 1], y + row_h));
                    if in_range(r, c) && range.is_some_and(|g| g.top != g.bottom || g.left != g.right) {
                        painter.rect_filled(cell_rect, 0.0, p.select_soft.gamma_multiply(0.6));
                    }
                    let value = cell(r, c);
                    if !value.is_empty() {
                        let numeric = value.trim().replace([' ', '\u{a0}'], "").parse::<f64>().is_ok();
                        let clip = painter.with_clip_rect(cell_rect.shrink(1.0).intersect(painter.clip_rect()));
                        let (pos, align) = if numeric {
                            (Pos2::new(cell_rect.right() - 6.0, cell_rect.center().y), Align2::RIGHT_CENTER)
                        } else {
                            (Pos2::new(cell_rect.left() + 6.0, cell_rect.center().y), Align2::LEFT_CENTER)
                        };
                        clip.text(pos, align, value, font.clone(), p.text);
                    }
                    painter.vline(cell_rect.right(), cell_rect.y_range(), line);
                }
                painter.hline(origin.x + head_w..=origin.x + xs[last_col], y + row_h, line);
            }
            // Column headers, row headers and corner stay visible while scrolling.
            let top = viewport.min.y.max(0.0) + origin.y;
            for c in first_col..last_col {
                let head = Rect::from_min_max(Pos2::new(origin.x + xs[c], top), Pos2::new(origin.x + xs[c + 1], top + row_h));
                let on = selected.is_some_and(|(_, sc)| sc == c) || in_range(usize::MAX, c) && false;
                painter.rect_filled(head, 0.0, if on { p.select_soft } else { p.grid_head });
                painter.vline(head.right(), head.y_range(), line);
                painter.hline(head.x_range(), head.bottom(), line);
                let f = if on { theme::semibold(13.0 * zoom) } else { font.clone() };
                painter.text(head.center(), Align2::CENTER_CENTER, column_name(c), f, p.icon);
            }
            let left = viewport.min.x.max(0.0) + origin.x;
            for r in first_row..last_row {
                let y = origin.y + row_h * (r + 1) as f32;
                let head = Rect::from_min_size(Pos2::new(left, y), Vec2::new(head_w, row_h));
                if head.bottom() <= top + row_h {
                    continue;
                }
                let on = selected.is_some_and(|(sr, _)| sr == r);
                painter.rect_filled(head, 0.0, if on { p.select_soft } else { p.grid_head });
                painter.vline(head.right(), head.y_range(), line);
                painter.hline(head.x_range(), head.bottom(), line);
                let f = if on { theme::semibold(12.0 * zoom) } else { FontId::proportional(12.0 * zoom) };
                painter.text(head.center(), Align2::CENTER_CENTER, self.app.grid_row_label(r), f, p.icon);
            }
            let corner = Rect::from_min_size(Pos2::new(left, top), Vec2::new(head_w, row_h));
            painter.rect_filled(corner, 0.0, p.grid_head);
            painter.vline(corner.right(), corner.y_range(), line);
            painter.hline(corner.x_range(), corner.bottom(), line);
            // Focused cell.
            if let Some((r, c)) = selected.filter(|(r, c)| *r < rows && *c < cols) {
                let y = origin.y + row_h * (r + 1) as f32;
                let focus = Rect::from_min_max(Pos2::new(origin.x + xs[c], y), Pos2::new(origin.x + xs[c + 1], y + row_h));
                if focus.top() >= top + row_h - 1.0 && focus.left() >= left + head_w - 1.0 {
                    painter.rect_stroke(focus, 0.0, Stroke::new(2.0, p.select), StrokeKind::Inside);
                }
            }
            if response.clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    let local = pos - origin;
                    let r = (local.y / row_h).floor() as usize;
                    if r >= 1 && local.x >= head_w {
                        if let Some(c) = xs.windows(2).position(|w| local.x >= w[0] && local.x < w[1]) {
                            let shift = ui.input(|i| i.modifiers.shift);
                            click = Some((r - 1, c, shift));
                        }
                    }
                }
            }
            rect
        });
        Self::landmark(ui, area.inner_rect, Role::Grid, "Cells");
        if let Some((r, c, shift)) = click {
            self.commit_text();
            match (shift, selected) {
                (true, Some((sr, sc))) => self.app.select_grid_range_with_focus(
                    suite42_app::GridRange { top: sr.min(r), left: sc.min(c), bottom: sr.max(r), right: sc.max(c) },
                    (sr, sc),
                ),
                _ => self.app.select_cell(r, c),
            }
            self.reload();
        }
    }

    fn sheet_tabs(&mut self, root: &mut Ui, p: &Palette) {
        let sheets = self.app.grid_sheets();
        let add = self.app_commands().into_iter().find(|c| c.2.replace('&', "").eq_ignore_ascii_case("Add sheet")).map(|c| c.1);
        let sum = self.selection_sum();
        let ctx = root.ctx().clone();
        let mut select = None;
        let panel = egui::Panel::bottom("sheets")
            .exact_size(SHEET_TABS_HEIGHT)
            .show_separator_line(false)
            .frame(Frame::new().fill(p.bg).inner_margin(Margin::symmetric(8, 0)))
            .show(root, |ui| {
                let r = ui.max_rect();
                ui.painter().hline(r.x_range(), r.top(), Stroke::new(1.0, p.grid_line));
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    theme::flat(ui);
                    if let Some(id) = add {
                        if self.icon_button(ui, Icon::Plus, &tr("Add sheet"), self.command_enabled(id), false, p).clicked() {
                            self.command(&ctx, id);
                        }
                    }
                    if let Some((names, current)) = &sheets {
                        let all = egui::containers::menu::MenuButton::from_button(egui::Button::image(self.icons.image(Icon::List, 18.0).tint(p.icon))).ui(ui, |ui| {
                            for (i, name) in names.iter().enumerate() {
                                if self.menu_item(ui, None, name, None, true, i == *current).clicked() {
                                    select = Some(i);
                                    ui.close();
                                }
                            }
                        });
                        all.0.widget_info(|| WidgetInfo::labeled(Role::Button, true, "All sheets"));
                        ui.add_space(8.0);
                        for (i, name) in names.iter().enumerate() {
                            let on = i == *current;
                            let font = if on { theme::semibold(14.0) } else { FontId::proportional(14.0) };
                            let galley = ui.painter().layout_no_wrap(name.clone(), font, p.text);
                            let (rect, response) = ui.allocate_exact_size(Vec2::new(galley.size().x + 28.0, 32.0), Sense::click());
                            let (fill, color) = if on {
                                (if p.dark { p.select_soft } else { Color32::from_rgb(0xe3, 0xec, 0xfb) }, if p.dark { p.select } else { Color32::from_rgb(0x0b, 0x57, 0xd0) })
                            } else if response.hovered() {
                                (p.hover, p.icon)
                            } else {
                                (Color32::TRANSPARENT, p.icon)
                            };
                            ui.painter().rect_filled(rect, CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, fill);
                            ui.painter().text(rect.center(), Align2::CENTER_CENTER, name, if on { theme::semibold(14.0) } else { FontId::proportional(14.0) }, color);
                            let label = name.clone();
                            response.widget_info(|| {
                                let mut info = WidgetInfo::labeled(Role::Tab, true, &label);
                                info.selected = Some(on);
                                info
                            });
                            if response.clicked() {
                                select = Some(i);
                            }
                        }
                    }
                    if let Some(sum) = &sum {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(8.0);
                            let chip = Frame::new().stroke(Stroke::new(1.0, p.separator)).corner_radius(4).inner_margin(Margin::symmetric(10, 4)).show(ui, |ui| {
                                ui.label(RichText::new(sum).size(13.0).color(p.icon));
                            });
                            Self::landmark(ui, chip.response.rect, Role::Status, "Selection sum");
                        });
                    }
                });
            });
        Self::landmark(root, panel.response.rect, Role::TabList, "Sheets");
        if let Some(i) = select {
            self.commit_text();
            self.app.select_grid_sheet(i);
            self.reload();
        }
    }

    /// "Sum: …" for a selected range with at least two numeric cells.
    fn selection_sum(&self) -> Option<String> {
        let range = self.app.grid_selection()?;
        if range.top == range.bottom && range.left == range.right {
            return None;
        }
        let sparse = self.app.sparse_table();
        let dense = if sparse.is_none() { self.app.table() } else { None };
        let mut sum = 0.0;
        let mut count = 0;
        for r in range.top..=range.bottom.min(range.top + 10_000) {
            for c in range.left..=range.right.min(range.left + 500) {
                let v = match (&sparse, &dense) {
                    (Some(s), _) if r < s.len() && c < s.columns() => s.cell(r, c).to_string(),
                    (None, Some(d)) => d.get(r).and_then(|row| row.get(c)).cloned().unwrap_or_default(),
                    _ => String::new(),
                };
                if let Ok(n) = v.trim().replace([' ', '\u{a0}'], "").parse::<f64>() {
                    sum += n;
                    count += 1;
                }
            }
        }
        (count >= 2).then(|| format!("{}: {}", tr("Sum"), format_number(sum)))
    }

    // ----- overlays -------------------------------------------------------------------

    /// Height of a bar along the window's bottom edge (the sheet tabs) that floating chips must clear.
    fn bottom_bar(&self) -> f32 {
        if !self.home && matches!(self.kind(), Kind::Table) {
            SHEET_TABS_HEIGHT
        } else {
            0.0
        }
    }

    fn status_chip(&self, ctx: &egui::Context, p: &Palette) {
        let Some(summary) = self.status_summary() else { return };
        egui::Area::new(egui::Id::unique("off42-status-chip"))
            .order(egui::Order::Foreground)
            .interactable(false)
            .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-24.0, -16.0 - self.bottom_bar()))
            .show(ctx, |ui| {
                let chip = Frame::new()
                    .fill(p.toolbar)
                    .corner_radius(12)
                    .inner_margin(Margin::symmetric(10, 4))
                    .show(ui, |ui| {
                        ui.add(egui::Label::new(RichText::new(&summary).size(12.0).color(p.icon)).extend());
                    });
                Self::landmark(ui, chip.response.rect, Role::Status, "Document status");
            });
    }

    fn overlays(&mut self, ctx: &egui::Context, p: &Palette) {
        self.status_chip(ctx, p);
        if let Some(mut dialog) = self.files.take() {
            match dialog.show(ctx, &self.icons, p) {
                Outcome::None => self.files = Some(dialog),
                Outcome::Cancel => {}
                Outcome::Chosen(path) => self.file_chosen(dialog.purpose.clone(), path),
            }
        }
        if let Some(form) = self.input.take() {
            self.input_form(ctx, form, p);
        }
        if let Some(dialog) = self.dialog.take() {
            self.dialog_ui(ctx, dialog, p);
        }
        if self.about {
            let modal = egui::Modal::new(egui::Id::unique("off42-about")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
                    icons::paint_logo(ui.painter(), rect, self.info.logo, self.info.accent);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(format!("{} — {}", self.app.name(), tr(self.info.title))).font(theme::semibold(18.0)));
                        ui.label(RichText::new(format!("off42 {} · {}", env!("CARGO_PKG_VERSION"), tr("Linux preview"))).color(p.muted));
                    });
                });
                ui.add_space(12.0);
                ui.label(RichText::new(tr("Documents are processed on this computer. Interface font: Inter (SIL Open Font License).")).color(p.muted));
                ui.add_space(16.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| primary_button(ui, &tr("Close"), p).clicked()).inner
            });
            if modal.inner || modal.should_close() {
                self.about = false;
            }
        }
        if let Some(snack) = &self.snack {
            let life = if snack.error { 10.0 } else { 6.0 };
            if snack.at.elapsed().as_secs_f32() > life {
                self.snack = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(500));
            }
        }
        if let Some(snack) = &self.snack {
            let mut close = false;
            let text = snack.text.clone();
            let error = snack.error;
            let screen = ctx.content_rect();
            egui::Area::new(egui::Id::unique("off42-snackbar"))
                .order(egui::Order::Tooltip)
                .anchor(Align2::LEFT_BOTTOM, Vec2::new(24.0, -24.0 - self.bottom_bar()))
                .show(ctx, |ui| {
                    let frame = Frame::new()
                        .fill(p.snack)
                        .corner_radius(6)
                        .inner_margin(Margin { left: 16, right: 8, top: 8, bottom: 8 })
                        .shadow(egui::Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(64) })
                        .show(ui, |ui| {
                            ui.set_max_width((screen.width() - 96.0).min(560.0));
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 16.0;
                                let color = if error { theme::mix(p.snack_text, Color32::from_rgb(0xf2, 0x8b, 0x82), 0.6) } else { p.snack_text };
                                ui.add(egui::Label::new(RichText::new(&text).color(color)).wrap());
                                theme::flat(ui);
                                let dismiss = ui.add(egui::Button::new(RichText::new(tr("Dismiss")).font(theme::semibold(14.0)).color(p.snack_action)));
                                close = dismiss.clicked();
                            });
                        });
                    Self::landmark(ui, frame.response.rect, Role::Status, "Notification");
                });
            if close {
                self.snack = None;
            }
        }
    }

    fn input_form(&mut self, ctx: &egui::Context, mut form: InputForm, p: &Palette) {
        let mut result: Option<bool> = None;
        let modal = egui::Modal::new(egui::Id::unique(("off42-input", form.id))).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(RichText::new(&form.title).font(theme::semibold(18.0)));
            ui.add_space(12.0);
            for (i, (label, value)) in form.fields.iter_mut().enumerate() {
                let caption = ui.label(RichText::new(label.as_str()).color(p.muted));
                if let Some((_, choices)) = form.choices.iter().find(|(f, _)| *f == i) {
                    egui::ComboBox::from_id_salt(("choice", i))
                        .selected_text(tr(value))
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            for choice in choices {
                                ui.selectable_value(value, choice.clone(), tr(choice));
                            }
                        });
                } else if form.multiline.contains(&i) {
                    ui.add(egui::TextEdit::multiline(value).desired_width(f32::INFINITY).desired_rows(5)).labelled_by(caption.id);
                } else {
                    ui.add(egui::TextEdit::singleline(value).password(form.password && i == 0).desired_width(f32::INFINITY)).labelled_by(caption.id);
                }
                ui.add_space(6.0);
            }
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if primary_button(ui, &tr("OK"), p).clicked() {
                    result = Some(true);
                }
                if ui.add(egui::Button::new(tr("Cancel")).corner_radius(16).min_size(Vec2::new(88.0, 32.0))).clicked() {
                    result = Some(false);
                }
            });
        });
        if modal.should_close() && result.is_none() {
            result = Some(false);
        }
        match result {
            None => self.input = Some(form),
            Some(false) => {}
            Some(true) => {
                let values: Vec<String> = form.fields.iter().map(|(_, v)| v.clone()).collect();
                if form.file_action {
                    self.choose_file_action(form.id, Some(values));
                } else {
                    let result = self.app.input_action(form.id, &values);
                    self.report(result, tr("Done"));
                }
            }
        }
    }

    fn dialog_ui(&mut self, ctx: &egui::Context, mut dialog: Dialog, p: &Palette) {
        let mut done: Option<Option<(u16, DialogResult)>> = None;
        let modal = egui::Modal::new(egui::Id::unique("off42-dialog")).show(ctx, |ui| {
            ui.set_width(420.0);
            match &mut dialog {
                Dialog::Confirm { id, title, text } => {
                    ui.label(RichText::new(title.as_str()).font(theme::semibold(18.0)));
                    ui.add_space(8.0);
                    ui.label(text.as_str());
                    ui.add_space(16.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if primary_button(ui, &tr("Yes"), p).clicked() {
                            done = Some(Some((*id, DialogResult::Confirmed(true))));
                        }
                        if ui.add(egui::Button::new(tr("No")).corner_radius(16).min_size(Vec2::new(88.0, 32.0))).clicked() {
                            done = Some(Some((*id, DialogResult::Confirmed(false))));
                        }
                    });
                }
                Dialog::Info { title, text } => {
                    ui.label(RichText::new(title.as_str()).font(theme::semibold(18.0)));
                    ui.add_space(8.0);
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| ui.label(text.as_str()));
                    ui.add_space(16.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if primary_button(ui, &tr("OK"), p).clicked() {
                            done = Some(None);
                        }
                    });
                }
                Dialog::Choices { id, values, checked } => {
                    ui.label(RichText::new(tr("Choose a value")).font(theme::semibold(18.0)));
                    ui.add_space(8.0);
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        for (i, v) in values.iter().enumerate() {
                            if ui.selectable_label(*checked == Some(i), v).clicked() {
                                done = Some(Some((*id, DialogResult::GridChoice(v.clone()))));
                            }
                        }
                    });
                    ui.add_space(12.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new(tr("Cancel")).corner_radius(16).min_size(Vec2::new(88.0, 32.0))).clicked() {
                            done = Some(None);
                        }
                    });
                }
                Dialog::Color { id, color } => {
                    ui.label(RichText::new(tr("Color")).font(theme::semibold(18.0)));
                    ui.add_space(8.0);
                    let mut c = Color32::from_rgb(color[0], color[1], color[2]);
                    egui::color_picker::color_picker_color32(ui, &mut c, egui::color_picker::Alpha::Opaque);
                    *color = [c.r(), c.g(), c.b()];
                    ui.add_space(12.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if primary_button(ui, &tr("OK"), p).clicked() {
                            done = Some(Some((*id, DialogResult::Color(*color))));
                        }
                        if ui.add(egui::Button::new(tr("Cancel")).corner_radius(16).min_size(Vec2::new(88.0, 32.0))).clicked() {
                            done = Some(None);
                        }
                    });
                }
            }
        });
        if modal.should_close() && done.is_none() {
            done = Some(None);
        }
        match done {
            None => self.dialog = Some(dialog),
            Some(None) => {}
            Some(Some((id, result))) => {
                let result = self.app.dialog_result(id, result);
                self.report(result, tr("Done"));
            }
        }
    }
}

impl eframe::App for Host {
    fn ui(&mut self, root: &mut Ui, _frame: &mut eframe::Frame) {
        self.draw(root);
    }
}

fn primary_button(ui: &mut Ui, label: &str, p: &Palette) -> Response {
    let text = if p.dark { Color32::from_rgb(0x06, 0x2e, 0x6f) } else { Color32::WHITE };
    ui.add(egui::Button::new(RichText::new(label).color(text)).fill(p.select).stroke(Stroke::NONE).corner_radius(16).min_size(Vec2::new(88.0, 32.0)))
}

fn sibling(dir: &Path, exe: &str) -> Option<PathBuf> {
    [exe.to_string(), exe.to_ascii_lowercase()]
        .into_iter()
        .map(|name| dir.join(name))
        .find(|p| p.is_file())
}

pub(crate) fn display_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

fn ext_of(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase()
}

pub(crate) fn column_name(mut c: usize) -> String {
    let mut name = String::new();
    loop {
        name.insert(0, (b'A' + (c % 26) as u8) as char);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    name
}

fn format_number(n: f64) -> String {
    if n.fract().abs() < 1e-9 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{:.2}", n)
    }
}

fn format_size(bytes: u64) -> String {
    match bytes {
        b if b < 1024 => format!("{b} B"),
        b if b < 1024 * 1024 => format!("{} KB", b.div_ceil(1024)),
        b => format!("{:.1} MB", b as f64 / 1048576.0),
    }
}

fn format_time(time: std::time::SystemTime) -> String {
    let Ok(age) = std::time::SystemTime::now().duration_since(time) else { return tr("Today") };
    let days = age.as_secs() / 86_400;
    match days {
        0 => tr("Today"),
        1 => tr("Yesterday"),
        d => format!("{d} {}", tr("days ago")),
    }
}

/// "Ctrl+Shift+E", "F5", "Ctrl+=" → egui modifiers and key.
pub(crate) fn parse_accelerator(text: &str) -> Option<(Modifiers, Key)> {
    let mut modifiers = Modifiers::NONE;
    let mut key = None;
    for part in text.split('+').map(str::trim) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "cmd" => modifiers = modifiers | Modifiers::COMMAND,
            "shift" => modifiers = modifiers | Modifiers::SHIFT,
            "alt" => modifiers = modifiers | Modifiers::ALT,
            "" => key = Key::from_name("+"),
            other => key = Key::from_name(other).or_else(|| Key::from_name(&other.to_ascii_uppercase())),
        }
    }
    key.map(|k| (modifiers, k))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn column_names_follow_spreadsheet_letters() {
        assert_eq!(column_name(0), "A");
        assert_eq!(column_name(25), "Z");
        assert_eq!(column_name(26), "AA");
        assert_eq!(column_name(701), "ZZ");
    }
    #[test]
    fn accelerators_parse() {
        assert_eq!(parse_accelerator("Ctrl+B"), Some((Modifiers::COMMAND, Key::B)));
        assert_eq!(parse_accelerator("Ctrl+Shift+E"), Some((Modifiers::COMMAND | Modifiers::SHIFT, Key::E)));
        assert_eq!(parse_accelerator("F5"), Some((Modifiers::NONE, Key::F5)));
    }
}
