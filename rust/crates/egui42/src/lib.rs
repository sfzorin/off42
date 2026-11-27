//! Experimental host for Linux and other non-Windows systems.
//!
//! It drives the same `App` document models as the Win32 host through their portable surface:
//! item list, plain text, table cells, RGBA image, canvas scenes, commands, input forms and
//! Save As formats. The window follows the suite's shared web design (home screen, title row
//! with menu bar, one toolbar strip, paper canvas, sheet tabs, slide strip, snackbar). Rich
//! text pages, printing, scanners and Windows-only dialogs are not available here.
use suite42_app::{App, FileFormat};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use suite42_core::session::{publish_bundle, FileIdentity};

mod files;
mod icons;
mod scene;
mod theme;
mod ui;
pub use ui::Host;

#[cfg(feature = "kittest")]
pub mod testing;

pub(crate) const UNDO: u16 = 4;
pub(crate) const REDO: u16 = 5;

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
pub(crate) fn table_rows(app: &dyn App, max_rows: usize, max_columns: usize) -> Option<Vec<Vec<String>>> {
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
    let title = app.name().to_string();
    let host = Host::new(app, file);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(title.clone())
            .with_app_id(title.to_ascii_lowercase())
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([720.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        &title,
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(host))
        }),
    )
    .map_err(|e| format!("Cannot open a window ({e}). Use --help for command-line conversion."))
}
