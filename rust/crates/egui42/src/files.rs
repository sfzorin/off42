//! In-window file chooser for Open, Save as, import and export (no system dialog dependency).
use crate::icons::{Icon, Icons};
use crate::theme::Palette;
use eframe::egui::{self, Align, Layout, RichText, Vec2};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Purpose {
    Open,
    SaveAs,
    /// Run a file action (import or export) with the given ID and collected settings.
    Action { id: u16, save: bool, values: Option<Vec<String>> },
}

pub struct FileDialog {
    pub purpose: Purpose,
    pub title: String,
    pub dir: PathBuf,
    pub dir_input: String,
    pub name: String,
    /// Lower-case extensions shown in the list; empty shows every file.
    pub extensions: Vec<String>,
    pub selected: Option<PathBuf>,
    entries: Vec<(bool, String)>,
    error: Option<String>,
}

pub enum Outcome {
    None,
    Cancel,
    Chosen(PathBuf),
}

impl FileDialog {
    pub fn new(purpose: Purpose, title: String, start: Option<&Path>, name: String, extensions: Vec<String>) -> Self {
        let dir = start
            .filter(|d| d.is_dir())
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/"));
        let mut dialog = FileDialog {
            purpose,
            title,
            dir_input: dir.display().to_string(),
            dir,
            name,
            extensions,
            selected: None,
            entries: Vec::new(),
            error: None,
        };
        dialog.refresh();
        dialog
    }

    pub fn saving(&self) -> bool {
        matches!(self.purpose, Purpose::SaveAs | Purpose::Action { save: true, .. })
    }

    fn refresh(&mut self) {
        self.entries.clear();
        self.error = None;
        match std::fs::read_dir(&self.dir) {
            Ok(read) => {
                for entry in read.flatten().take(5000) {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') {
                        continue;
                    }
                    let is_dir = entry.path().is_dir();
                    let ext = Path::new(&name)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    if is_dir || self.extensions.is_empty() || self.extensions.contains(&ext) {
                        self.entries.push((is_dir, name));
                    }
                }
                self.entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase())));
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        self.dir_input = self.dir.display().to_string();
    }

    fn enter(&mut self, dir: PathBuf) {
        self.dir = dir;
        self.selected = None;
        self.refresh();
    }

    fn target(&self) -> Option<PathBuf> {
        if self.saving() {
            let name = self.name.trim();
            if name.is_empty() {
                return None;
            }
            let mut path = self.dir.join(name);
            if let Some(ext) = self.extensions.first() {
                let has = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(ext));
                if !has {
                    path.set_extension(ext);
                }
            }
            Some(path)
        } else {
            self.selected.clone()
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, icons: &Icons, p: &Palette) -> Outcome {
        let mut outcome = Outcome::None;
        let modal = egui::Modal::new(egui::Id::unique("off42-file-dialog"))
            .backdrop_color(egui::Color32::from_black_alpha(80))
            .show(ctx, |ui| {
                ui.set_width(620.0);
                ui.label(RichText::new(&self.title).font(crate::theme::semibold(18.0)));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let up = ui
                        .add(egui::Button::new(icons.image(Icon::ChevronRight, 16.0).rotate(-std::f32::consts::FRAC_PI_2, Vec2::splat(0.5))).min_size(Vec2::splat(30.0)))
                        .on_hover_text("Parent folder");
                    if up.clicked() {
                        if let Some(parent) = self.dir.parent().map(Path::to_path_buf) {
                            self.enter(parent);
                        }
                    }
                    let edit = ui.add(egui::TextEdit::singleline(&mut self.dir_input).desired_width(f32::INFINITY).hint_text("Folder"));
                    if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let dir = PathBuf::from(self.dir_input.trim());
                        if dir.is_dir() {
                            self.enter(dir);
                        } else {
                            self.error = Some("Folder not found".into());
                        }
                    }
                });
                ui.add_space(6.0);
                let mut enter = None;
                let mut chosen = None;
                egui::Frame::new()
                    .fill(p.surface)
                    .stroke(egui::Stroke::new(1.0, p.line))
                    .corner_radius(8)
                    .inner_margin(egui::Margin::same(4))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical().max_height(300.0).min_scrolled_height(300.0).auto_shrink([false, false]).show(ui, |ui| {
                            crate::theme::flat(ui);
                            ui.spacing_mut().item_spacing.y = 0.0;
                            if let Some(error) = &self.error {
                                ui.colored_label(p.error, error);
                            }
                            for (is_dir, name) in &self.entries {
                                let path = self.dir.join(name);
                                let selected = self.selected.as_ref() == Some(&path);
                                let icon = if *is_dir { Icon::Folder } else { Icon::Page };
                                let row = ui.add_sized(
                                    [ui.available_width(), 30.0],
                                    egui::Button::selectable(selected, (icons.image(icon, 16.0), name.as_str()))
                                        .image_tint_follows_text_color(true),
                                );
                                if row.clicked() {
                                    if *is_dir {
                                        enter = Some(path.clone());
                                    } else {
                                        self.selected = Some(path.clone());
                                        if self.saving() {
                                            self.name = name.clone();
                                        }
                                    }
                                }
                                if row.double_clicked() && !*is_dir {
                                    chosen = Some(path);
                                }
                            }
                            if self.entries.is_empty() && self.error.is_none() {
                                ui.add_space(8.0);
                                ui.label(RichText::new("No matching files in this folder").color(p.muted));
                            }
                        });
                    });
                if let Some(dir) = enter {
                    self.enter(dir);
                }
                if self.saving() {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label("File name");
                        ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(f32::INFINITY));
                    });
                }
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = if self.saving() { "Save" } else { "Open" };
                    let target = self.target();
                    let primary = ui.add_enabled(
                        target.is_some(),
                        egui::Button::new(RichText::new(label).color(egui::Color32::WHITE))
                            .fill(p.select)
                            .corner_radius(16)
                            .min_size(Vec2::new(88.0, 32.0)),
                    );
                    if primary.clicked() {
                        chosen = target;
                    }
                    if ui.add(egui::Button::new("Cancel").corner_radius(16).min_size(Vec2::new(88.0, 32.0))).clicked() {
                        outcome = Outcome::Cancel;
                    }
                    if let Some(ext) = self.extensions.first().filter(|_| self.saving()) {
                        ui.label(RichText::new(format!(".{ext}")).color(p.muted));
                    }
                });
                chosen
            });
        if modal.should_close() && matches!(outcome, Outcome::None) {
            outcome = Outcome::Cancel;
        }
        if let Some(path) = modal.inner {
            outcome = Outcome::Chosen(path);
        }
        outcome
    }
}
