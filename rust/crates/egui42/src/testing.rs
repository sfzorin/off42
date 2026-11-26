//! Headless harness for the egui host: structure assertions on every platform and, with the
//! `shots` feature and `OFF42_SHOTS_DIR` set, PNG renders of each window state for CI review.
use crate::Host;
pub use egui_kittest;
use egui_kittest::Harness;
use std::path::{Path, PathBuf};
use suite42_app::App;

pub const SIZE: [f32; 2] = [1280.0, 800.0];

/// A host window in a headless harness. Rendering is enabled only when screenshots are requested.
pub fn harness(app: Box<dyn App>, file: Option<&Path>, dark: bool) -> Harness<'static, Host> {
    let host = Host::new(app, file.map(Path::to_path_buf));
    let theme = if dark { eframe::egui::Theme::Dark } else { eframe::egui::Theme::Light };
    #[allow(unused_mut)]
    let mut builder = Harness::builder()
        .with_size(SIZE)
        .with_theme(theme)
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_max_steps(8);
    #[cfg(feature = "shots")]
    if shots_dir().is_some() {
        builder = builder.wgpu();
    }
    let mut harness = builder.build_ui_state(|ui, host: &mut Host| host.draw(ui), host);
    harness.run_steps(3);
    harness
}

pub fn shots_dir() -> Option<PathBuf> {
    std::env::var_os("OFF42_SHOTS_DIR").map(PathBuf::from).filter(|p| !p.as_os_str().is_empty())
}

/// Save the current frame as `<OFF42_SHOTS_DIR>/<name>.png` when screenshots are requested.
pub fn shot(harness: &mut Harness<'static, Host>, name: &str) {
    harness.run_steps(2);
    #[cfg(feature = "shots")]
    if let Some(dir) = shots_dir() {
        std::fs::create_dir_all(&dir).unwrap();
        harness.set_render_cursor(false);
        let image = harness.render().expect("render the host window");
        image.save(dir.join(format!("{name}.png"))).expect("write the screenshot");
    }
    #[cfg(not(feature = "shots"))]
    let _ = name;
}

/// Render the standard window states of one app. `make` builds a fresh document model and
/// `sample` is an optional document to open for the populated states.
pub fn capture_states(prefix: &str, make: impl Fn() -> Box<dyn App>, sample: Option<&Path>) {
    use egui_kittest::kittest::Queryable;
    let mut h = harness(make(), None, false);
    shot(&mut h, &format!("{prefix}-01-start"));
    if let Some(sample) = sample {
        let mut h = harness(make(), Some(sample), false);
        shot(&mut h, &format!("{prefix}-02-document"));
        if let Some(menu) = h.query_by_label("Commands") {
            menu.click();
            shot(&mut h, &format!("{prefix}-03-commands-menu"));
        }
        let mut h = harness(make(), Some(sample), true);
        shot(&mut h, &format!("{prefix}-04-dark"));
    }
}
