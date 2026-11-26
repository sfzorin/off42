//! Experimental host for Linux and other non-Windows systems.
//!
//! It drives the same `App` document models as the Win32 host, but only through
//! their portable surface: item list, plain text, table cells, RGBA image,
//! commands and Save As formats. Native canvases, rich text pages, printing,
//! scanners and Windows dialogs are not available here.
use suite42_app::{App, FileFormat};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use suite42_core::session::{publish_bundle, FileIdentity};

#[cfg(feature = "kittest")]
pub mod testing;

const UNDO: u16 = 4;
const REDO: u16 = 5;

pub fn run(app: Box<dyn App>) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match cli(app, &args) {
        Ok(()) => {}
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

fn usage(app: &dyn App) -> String {
    let exe = app.name();
    format!(
        "{exe} (off42 Linux preview, experimental)\n\n\
Usage:\n  {exe} [FILE]                 open the window (needs X11 or Wayland)\n  \
{exe} --text FILE            print the document text\n  \
{exe} --items FILE           print pages/sheets/slides/items\n  \
{exe} --table FILE           print the current table as tab-separated values\n  \
{exe} --convert IN OUT       open IN and save it as OUT (format from extension)\n  \
{exe} --export ID IN OUT     run a file action (see --formats) and write OUT\n  \
{exe} --formats              list open/save formats and file actions\n  \
{exe} --commands             list commands available in this build\n  \
{exe} --version\n"
    )
}

fn cli(mut app: Box<dyn App>, args: &[String]) -> Result<(), String> {
    let help = usage(app.as_ref());
    let arg = |i: usize| -> Result<&str, String> {
        args.get(i).map(String::as_str).ok_or_else(|| help.clone())
    };
    match args.first().map(String::as_str) {
        Some("-h" | "--help") => {
            print!("{}", usage(app.as_ref()));
            Ok(())
        }
        Some("--version") => {
            println!("{} {} (Linux preview)", app.name(), env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("--formats") => {
            let show = |label: &str, formats: Vec<FileFormat>| {
                for f in formats {
                    println!("{label}\t{}\t{}\t{}", f.id, f.title, f.extensions.join(","));
                }
            };
            println!("native\t{}\t{}\t{}", app.extension(), app.format_name(), app.extension());
            show("open", app.open_formats());
            show("save", app.save_formats());
            for a in app.file_actions() {
                let kind = if a.save { "export" } else { "import" };
                println!("{kind}\t{}\t{}\t{}", a.id, a.title.replace('&', ""), a.extension);
            }
            Ok(())
        }
        Some("--commands") => {
            for (id, title) in commands(app.as_ref()) {
                println!("{id}\t{title}");
            }
            Ok(())
        }
        Some("--text") => {
            open_blocking(app.as_mut(), Path::new(arg(1)?))?;
            println!("{}", suite42_app::from_native_text(&app.text()));
            Ok(())
        }
        Some("--items") => {
            open_blocking(app.as_mut(), Path::new(arg(1)?))?;
            for (i, item) in app.items().iter().enumerate() {
                println!("{}\t{item}", i + 1);
            }
            Ok(())
        }
        Some("--table") => {
            open_blocking(app.as_mut(), Path::new(arg(1)?))?;
            let rows = table_rows(app.as_ref(), usize::MAX, usize::MAX).ok_or("This document has no table view")?;
            for row in rows {
                println!("{}", row.join("\t"));
            }
            Ok(())
        }
        Some("--convert") => {
            let (input, output) = (PathBuf::from(arg(1)?), PathBuf::from(arg(2)?));
            open_blocking(app.as_mut(), &input)?;
            save_as(app.as_mut(), &output)?;
            eprintln!("Saved {}", output.display());
            Ok(())
        }
        Some("--export") => {
            let id: u16 = arg(1)?.parse().map_err(|_| "File action ID must be a number")?;
            let (input, output) = (PathBuf::from(arg(2)?), PathBuf::from(arg(3)?));
            open_blocking(app.as_mut(), &input)?;
            run_file_action(app.as_mut(), id, &output)?;
            eprintln!("Wrote {}", output.display());
            Ok(())
        }
        Some(flag) if flag.starts_with("--") => Err(usage(app.as_ref())),
        file => gui(app, file.map(PathBuf::from)),
    }
}

/// Dense rows from either table model, trimmed to a displayable range.
fn table_rows(app: &dyn App, max_rows: usize, max_columns: usize) -> Option<Vec<Vec<String>>> {
    if let Some(rows) = app.table() {
        return Some(rows);
    }
    let sparse = app.sparse_table()?;
    let rows = sparse.len().min(max_rows);
    let columns = sparse.columns().min(max_columns);
    Some(
        (0..rows)
            .map(|r| (0..columns).map(|c| sparse.cell(r, c).to_string()).collect())
            .collect(),
    )
}

fn commands(app: &dyn App) -> Vec<(u16, String)> {
    let mut out: Vec<(u16, String)> = vec![(UNDO, "Undo".into()), (REDO, "Redo".into())];
    let mut push = |id: u16, title: &str| {
        if !out.iter().any(|(known, _)| *known == id) {
            out.push((id, title.replace('&', "")));
        }
    };
    for (id, title) in app.actions().iter().chain(app.view_actions()) {
        push(*id, title);
    }
    for spec in app.command_specs() {
        push(spec.id, spec.title);
    }
    out
}

/// Open synchronously, waiting for apps that prepare documents on a worker.
pub fn open_blocking(app: &mut dyn App, path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("{}: file not found", path.display()));
    }
    if app.opens_as_import(path) {
        return app.import_open(path);
    }
    if !app.open_is_add() && app.begin_async_open(path)? {
        let deadline = Instant::now() + Duration::from_secs(300);
        loop {
            if let Some(result) = app.take_async_open_result() {
                result?;
                app.commit_async_open()?;
                break;
            }
            if app.take_async_open_password_request().is_some() {
                app.cancel_async_open();
                return Err("Password-protected files are not supported in the Linux preview".into());
            }
            if Instant::now() > deadline {
                app.cancel_async_open();
                return Err("Opening timed out".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    } else {
        app.open(path)?;
    }
    finish_background(app);
    Ok(())
}

/// Run an import/export command, accepting the default values of any settings prompt.
pub fn run_file_action(app: &mut dyn App, id: u16, path: &Path) -> Result<(), String> {
    let was_enabled = app.enabled(id);
    match app.file_input_request(id) {
        Some(request) => {
            let values: Vec<String> = request.fields.into_iter().map(|(_, value)| value).collect();
            app.file_action_with_input(id, path, &values)?;
        }
        None => app.file_action(id, path)?,
    }
    // Background exports disable their command until the worker finishes.
    let deadline = Instant::now() + Duration::from_secs(600);
    while was_enabled && !app.enabled(id) && Instant::now() < deadline {
        app.poll_background();
        std::thread::sleep(Duration::from_millis(20));
    }
    finish_background(app);
    Ok(())
}

fn finish_background(app: &mut dyn App) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while app.poll_background() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Save through the same snapshot formats the Windows Save As dialog offers.
pub fn save_as(app: &mut dyn App, path: &Path) -> Result<(), String> {
    let ext = extension(path);
    let format = app
        .save_formats()
        .into_iter()
        .find(|f| f.extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext)));
    if let Some(format) = format {
        if let Some((revision, bundle)) = app.save_bundle_to(format.id, path, false)? {
            let expected = if path.exists() {
                Some(FileIdentity::read(path).map_err(|e| e.to_string())?)
            } else {
                None
            };
            publish_bundle(path, &bundle, expected.as_ref()).map_err(|e| e.to_string())?;
            app.saved_snapshot(revision);
            return Ok(());
        }
    }
    if ext == app.extension().to_ascii_lowercase() {
        return app.save(path);
    }
    if let Some(action) = app
        .file_actions()
        .iter()
        .find(|a| a.save && a.extension.eq_ignore_ascii_case(&ext))
        .copied()
    {
        return run_file_action(app, action.id, path);
    }
    Err(format!("{} cannot save .{ext} files", app.name()))
}

fn gui(app: Box<dyn App>, file: Option<PathBuf>) -> Result<(), String> {
    let title = format!("{} — off42 Linux preview", app.name());
    let host = Host::new(app, file);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(title.clone())
            .with_inner_size([1100.0, 760.0]),
        ..Default::default()
    };
    eframe::run_native(&title, options, Box::new(|_| Ok(Box::new(host))))
        .map_err(|e| format!("Cannot open a window ({e}). Use --help for command-line conversion."))
}

pub struct Host {
    app: Box<dyn App>,
    path_input: String,
    export_path: String,
    text: String,
    status: String,
    texture: Option<eframe::egui::TextureHandle>,
    image_stamp: u64,
    input: Option<(u16, String, Vec<(String, String)>)>,
}

impl Host {
    pub fn new(app: Box<dyn App>, file: Option<PathBuf>) -> Self {
        let mut host = Host {
            app,
            path_input: String::new(),
            export_path: String::new(),
            text: String::new(),
            status: String::new(),
            texture: None,
            image_stamp: 0,
            input: None,
        };
        if let Some(file) = file {
            host.path_input = file.display().to_string();
            host.open();
        } else if host.app.supports_new() {
            let _ = host.app.new_document();
        }
        host.reload();
        host
    }
    fn report(&mut self, result: Result<(), String>, done: &str) {
        self.status = match result {
            Ok(()) => done.to_string(),
            Err(e) => format!("Error: {e}"),
        };
        self.reload();
    }
    fn reload(&mut self) {
        self.text = suite42_app::from_native_text(&self.app.text());
        self.image_stamp += 1;
        self.texture = None;
    }
    fn open(&mut self) {
        let path = PathBuf::from(self.path_input.trim());
        let result = open_blocking(self.app.as_mut(), &path);
        self.report(result, &format!("Opened {}", path.display()));
    }
    fn save(&mut self) {
        let path = PathBuf::from(self.path_input.trim());
        let result = save_as(self.app.as_mut(), &path);
        self.report(result, &format!("Saved {}", path.display()));
    }
    fn command(&mut self, id: u16) {
        if self.app.dialog_request(id).is_some() {
            self.status = "This command needs a Windows dialog and is not available on Linux yet".into();
            return;
        }
        if let Some(request) = self.app.input_request(id) {
            let fields = request
                .fields
                .into_iter()
                .map(|(label, value)| (label.to_string(), value))
                .collect();
            self.input = Some((id, request.title.to_string(), fields));
            return;
        }
        let result = self.app.action(id);
        self.report(result, "Done");
    }
}

impl eframe::App for Host {
    fn ui(&mut self, root: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(root);
    }
}

impl Host {
    pub fn draw(&mut self, root: &mut eframe::egui::Ui) {
        use eframe::egui;
        let ctx = &root.ctx().clone();
        if self.app.poll_background() {
            self.reload();
        }
        if let Some(path) = self.app.take_pending_open() {
            self.path_input = path.display().to_string();
            self.open();
        }
        if let Some(id) = self.app.take_pending_command() {
            self.command(id);
        }
        ctx.request_repaint_after(Duration::from_millis(self.app.background_poll_ms().max(50) as u64));

        egui::Panel::top("file").show(root, |ui| {
            ui.horizontal(|ui| {
                ui.label("File:");
                ui.add(egui::TextEdit::singleline(&mut self.path_input).desired_width(420.0));
                if ui.button("Open").clicked() {
                    self.open();
                }
                if ui.button("Save As").on_hover_text("Format is chosen from the file extension").clicked() {
                    self.save();
                }
                if self.app.supports_new() && ui.button("New").clicked() {
                    let result = self.app.new_document();
                    self.report(result, "New document");
                }
                ui.separator();
                if ui.add_enabled(self.app.can_undo(), egui::Button::new("Undo")).clicked() {
                    self.command(UNDO);
                }
                if ui.add_enabled(self.app.can_redo(), egui::Button::new("Redo")).clicked() {
                    self.command(REDO);
                }
                ui.menu_button("Commands", |ui| {
                    egui::ScrollArea::vertical().max_height(500.0).show(ui, |ui| {
                        for (id, title) in commands(self.app.as_ref()).into_iter().skip(2) {
                            if ui
                                .add_enabled(self.app.enabled(id), egui::Button::new(title))
                                .clicked()
                            {
                                self.command(id);
                                ui.close();
                            }
                        }
                    });
                });
                ui.menu_button("Export", |ui| {
                    ui.label("Output path:");
                    ui.text_edit_singleline(&mut self.export_path);
                    for action in self.app.file_actions().iter().copied() {
                        let title = action.title.replace('&', "");
                        if ui.button(format!("{title} (.{})", action.extension)).clicked() {
                            let path = PathBuf::from(self.export_path.trim());
                            let result = run_file_action(self.app.as_mut(), action.id, &path);
                            self.report(result, &format!("{title}: {}", path.display()));
                            ui.close();
                        }
                    }
                });
            });
        });
        egui::Panel::bottom("status").show(root, |ui| {
            let dirty = if self.app.dirty() { " • modified" } else { "" };
            ui.label(format!("{}{dirty}   {}", self.app.status(), self.status));
        });
        if self.app.list_visible() {
            egui::Panel::left("items").resizable(true).show(root, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let selected = self.app.selected();
                    for (i, item) in self.app.items().iter().enumerate().take(5000) {
                        if ui.selectable_label(i == selected, item).clicked() {
                            self.app.select(i);
                            self.reload();
                        }
                    }
                });
            });
        }
        if let Some((id, title, mut fields)) = self.input.take() {
            let mut keep = true;
            egui::Window::new(title.as_str()).collapsible(false).show(ctx, |ui| {
                for (label, value) in fields.iter_mut() {
                    ui.label(label.as_str());
                    ui.text_edit_singleline(value);
                }
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        let values: Vec<String> = fields.iter().map(|(_, v)| v.clone()).collect();
                        let result = self.app.input_action(id, &values);
                        self.report(result, "Done");
                        keep = false;
                    }
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                });
            });
            if keep {
                self.input = Some((id, title, fields));
            }
        }
        egui::CentralPanel::default().show(root, |ui| {
            if let Some((w, h, pixels)) = self.app.image() {
                if self.texture.is_none() && pixels.len() == (w as usize) * (h as usize) * 4 {
                    let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
                    self.texture = Some(ctx.load_texture(
                        format!("doc-{}", self.image_stamp),
                        image,
                        Default::default(),
                    ));
                }
                if let Some(texture) = &self.texture {
                    egui::ScrollArea::both().show(ui, |ui| {
                        ui.image((texture.id(), texture.size_vec2()));
                    });
                    return;
                }
            }
            if let Some(rows) = table_rows(self.app.as_ref(), 500, 60) {
                let selected = self.app.selected_cell();
                ui.horizontal(|ui| {
                    ui.label("Cell:");
                    let changed = ui.text_edit_singleline(&mut self.text).lost_focus();
                    if changed && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let result = self.app.edit(suite42_app::to_native_text(&self.text));
                        self.report(result, "Cell updated");
                    }
                });
                let mut clicked = None;
                egui::ScrollArea::both().show(ui, |ui| {
                    egui::Grid::new("table").striped(true).show(ui, |ui| {
                        for (r, row) in rows.iter().enumerate().take(500) {
                            for (c, cell) in row.iter().enumerate().take(60) {
                                if ui.selectable_label(selected == Some((r, c)), cell).clicked() {
                                    clicked = Some((r, c));
                                }
                            }
                            ui.end_row();
                        }
                    });
                });
                if let Some((r, c)) = clicked {
                    self.app.select_cell(r, c);
                    self.reload();
                }
                return;
            }
            ui.horizontal(|ui| {
                if ui.button("Apply text changes").clicked() {
                    let result = self.app.edit(suite42_app::to_native_text(&self.text));
                    self.report(result, "Text updated");
                }
            });
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.text)
                        .desired_width(f32::INFINITY)
                        .desired_rows(30),
                );
            });
        });
    }
}
