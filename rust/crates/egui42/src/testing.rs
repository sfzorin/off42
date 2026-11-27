//! Headless harness for the egui host: structure assertions on every platform and, with the
//! `shots` feature and `OFF42_SHOTS_DIR` set, PNG renders of each window state for CI review.
use crate::Host;
pub use egui_kittest;
use egui_kittest::kittest::{NodeT as _, Queryable};
use egui_kittest::Harness;
use std::path::{Path, PathBuf};
use suite42_app::App;

pub const SIZE: [f32; 2] = [1280.0, 800.0];

/// A host window in a headless harness. Rendering is enabled only when screenshots are requested.
pub fn harness(app: Box<dyn App>, file: Option<&Path>, dark: bool) -> Harness<'static, Host> {
    let host = Host::with_storage(app, file.map(Path::to_path_buf), false);
    let theme = if dark { eframe::egui::Theme::Dark } else { eframe::egui::Theme::Light };
    #[allow(unused_mut)]
    let mut builder = Harness::builder()
        .with_size(SIZE)
        .with_theme(theme)
        .allow_missing_glyphs()
        .with_max_steps(8)
        .with_context_setup(crate::theme::install);
    #[cfg(feature = "shots")]
    if shots_dir().is_some() {
        builder = builder.wgpu();
    }
    let mut harness = builder.build_eframe(move |_| host);
    harness.run_steps(4);
    harness
}

pub fn shots_dir() -> Option<PathBuf> {
    std::env::var_os("OFF42_SHOTS_DIR").map(PathBuf::from).filter(|p| !p.as_os_str().is_empty())
}

/// Save the current frame as `<OFF42_SHOTS_DIR>/<name>.png` when screenshots are requested.
pub fn shot(harness: &mut Harness<'static, Host>, name: &str) {
    harness.run_steps(3);
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

#[track_caller]
fn expect(h: &Harness<'static, Host>, label: &str, state: &str) {
    assert!(h.query_all_by_label(label).next().is_some(), "{state}: no element labelled {label:?}");
}

/// Label and value texts of every node whose rect lies inside `area`.
fn texts_in(h: &Harness<'static, Host>, area: eframe::egui::Rect) -> Vec<String> {
    h.root()
        .children_recursive()
        .filter(|n| n.accesskit_node().bounding_box().is_some() && area.expand(1.0).contains_rect(n.rect()))
        .flat_map(|n| [n.accesskit_node().label(), n.accesskit_node().value()])
        .flatten()
        .filter(|t| !t.trim().is_empty())
        .collect()
}

/// The title row shows the human save state and nothing of the app's status line.
#[track_caller]
fn check_title_row(h: &Harness<'static, Host>, state: &str) {
    let title = h.get_by_label("Title bar").rect();
    let texts = texts_in(h, title);
    let save_state = h.state().save_state();
    assert!(texts.iter().any(|t| t == &save_state), "{state}: title row shows the save state {save_state:?}: {texts:?}");
    let status = h.state().app.status();
    for t in &texts {
        assert!(!t.contains('|') && !t.contains("[]"), "{state}: title row shows debug text {t:?}");
        assert!(status.trim().is_empty() || !t.contains(status.trim()), "{state}: title row shows App::status {t:?}");
    }
    // Readable status fields go to the status chip, never with "|" separators or "[]".
    if let Some(summary) = h.state().status_summary() {
        expect(h, "Document status", state);
        let chip = texts_in(h, h.get_by_label("Document status").rect());
        assert!(chip.iter().any(|t| t == &summary), "{state}: status chip shows {summary:?}: {chip:?}");
        assert!(!summary.contains('|') && !summary.contains("[]"), "{state}: status chip shows debug text {summary:?}");
    }
}

/// Every toolbar command is an icon button with its label as accessible name and tooltip; the
/// toolbar has no text buttons. Commands that do not fit are in the More… overflow.
#[track_caller]
fn check_toolbar(h: &mut Harness<'static, Host>, state: &str) {
    use eframe::egui::accesskit::Role;
    let commands = h.state().toolbar_commands();
    let bar = h.get_by_label("Toolbar").rect();
    let mut shown = Vec::new();
    for (_, title, _, _) in &commands {
        let node = h
            .query_all_by_label(title)
            .find(|n| bar.expand(1.0).contains_rect(n.rect()));
        match node {
            Some(node) => {
                assert_eq!(node.accesskit_node().role(), Role::Button, "{state}: toolbar {title:?} is a button");
                let r = node.rect();
                assert!(r.width() <= 32.0 && r.height() <= 32.0, "{state}: toolbar {title:?} is an icon button, not text ({r:?})");
                shown.push(title.clone());
            }
            None => assert!(
                h.query_all_by_label("More tools").next().is_some(),
                "{state}: toolbar command {title:?} is neither shown nor in More…"
            ),
        }
    }
    for node in h.root().children_recursive() {
        let ak = node.accesskit_node();
        if ak.bounding_box().is_none() || !bar.expand(1.0).contains_rect(node.rect()) {
            continue;
        }
        if let Some(label) = ak.label() {
            assert!(
                !(ak.role() == Role::Label && commands.iter().any(|c| c.1 == label)),
                "{state}: toolbar shows the command {label:?} as text"
            );
        }
        if ak.role() == Role::Button {
            let r = node.rect();
            assert!(r.width() <= 40.0, "{state}: toolbar button {:?} is wider than an icon ({r:?})", ak.label());
        }
    }
    // The label is the tooltip of the first command.
    if let Some(title) = shown.first() {
        let center = h.query_all_by_label(title).find(|n| bar.contains_rect(n.rect())).unwrap().rect().center();
        h.hover_at(center);
        h.run_steps(6);
        let tips = h.query_all_by_label(title).filter(|n| !bar.expand(1.0).contains_rect(n.rect())).count();
        assert!(tips > 0, "{state}: hovering the toolbar {title:?} shows a tooltip with its label");
        h.hover_at(eframe::egui::pos2(SIZE[0] / 2.0, SIZE[1] - 4.0));
        h.run_steps(3);
    }
}

#[track_caller]
fn expect_text(h: &Harness<'static, Host>, text: &str, state: &str) {
    let found = h.query_all(egui_kittest::kittest::By::new().label_contains(text).include_labels()).next().is_some();
    assert!(found, "{state}: no text containing {text:?}");
}

/// Render and check the standard window states of one app. `make` builds a fresh document
/// model, `sample` is a document to open, and `parts` are labels that the opened document
/// window must show in addition to the shared chrome (for example "Sheets" or "Slides").
pub fn capture_states(prefix: &str, make: impl Fn() -> Box<dyn App>, sample: Option<&Path>, parts: &[&str]) {
    // Home: top bar, create/open strip, recent list.
    let mut h = harness(make(), None, false);
    if let Some(sample) = sample {
        h.state_mut().recent = vec![sample.to_path_buf()];
        h.run_steps(2);
    }
    for label in ["Main menu", "Language", "About", "Home bar", "Create and open", "Recent files"] {
        expect(&h, label, "home");
    }
    expect_text(&h, "off42", "home");
    shot(&mut h, &format!("{prefix}-01-home"));
    h.get_by_label("Language").click();
    h.run_steps(3);
    expect(&h, "English", "language popover");
    shot(&mut h, &format!("{prefix}-02-language"));
    h.key_press(eframe::egui::Key::Escape);
    h.get_by_label("Language").click();
    h.run_steps(2);
    h.get_by_label("Main menu").click();
    h.run_steps(3);
    expect(&h, "Apps", "app drawer");
    for app in ["Documents", "Spreadsheets", "Presentations", "PDF", "Images", "Scans"] {
        expect(&h, app, "app drawer");
    }
    shot(&mut h, &format!("{prefix}-03-drawer"));

    // Document window.
    let mut h = harness(make(), sample, false);
    assert!(!h.state().home, "a document opened from the command line shows the document window");
    for label in ["Title bar", "Menu bar", "Toolbar", "File", "Edit", "View", "Help", "Undo", "Redo", "Save"] {
        expect(&h, label, "document");
    }
    for label in parts {
        expect(&h, label, "document");
    }
    for label in ["Document text", "Slide text"] {
        if let Some(node) = h.query_all_by_label(label).next() {
            let value = node.value().unwrap_or_default();
            assert!(!value.trim().is_empty(), "{label} shows the document's text");
        }
    }
    let ext = h.state().path.as_deref().and_then(|p| p.extension()).map(|e| e.to_string_lossy().to_lowercase());
    if let Some(ext) = ext {
        expect_text(&h, &format!(".{ext}"), "format badge");
    }
    check_title_row(&h, "document");
    check_toolbar(&mut h, "document");
    shot(&mut h, &format!("{prefix}-04-document"));

    // Functional checks on a separate window so the screenshots above stay deterministic.
    let mut b = harness(make(), None, false);
    if b.query_all_by_label("Blank document").next().is_some() {
        b.get_all_by_label("Blank document").next().unwrap().click();
        b.run_steps(3);
        assert!(!b.state().home, "Blank document opens the document window");
        check_title_row(&b, "blank document");
        check_toolbar(&mut b, "blank document");
        if b.query_all_by_label("Document text").next().is_some() {
            // Typed text on a blank page reaches the model as one undoable edit.
            b.get_by_label("Document text").focus();
            b.run_steps(2);
            b.get_by_label("Document text").type_text("typed on Linux");
            b.run_steps(2);
            assert!(b.state().text_edited, "typing marks the page as edited");
            b.state_mut().commit_text();
            assert!(b.state().app.text().contains("typed on Linux"), "typed text is committed: {:?}", b.state().snack_text());
            assert!(b.state().app.can_undo(), "a committed edit can be undone");
            b.get_by_label("Undo").click();
            b.run_steps(3);
            assert!(!b.state().app.text().contains("typed on Linux"), "the toolbar Undo reverts the edit");
            assert!(!b.state().text.contains("typed on Linux"), "the page shows the undone text");
        }
    }
    let mut f = harness(make(), sample, false);
    if f.query_all_by_label("Document text").next().is_some() {
        // Rich documents refuse plain-text edits; the page then reverts to the document's text.
        f.get_by_label("Document text").focus();
        f.run_steps(2);
        f.get_by_label("Document text").type_text(" typed");
        f.run_steps(2);
        f.state_mut().commit_text();
        let shown = f.state().text.clone();
        let model = suite42_app::from_native_text(&f.state().app.text());
        assert_eq!(shown, model, "after a commit the page shows exactly the document's text");
    }
    if f.query_all_by_label("Slide 2").next().is_some() {
        f.get_by_label("Slide 2").click();
        f.run_steps(3);
        assert_eq!(f.state().app.selected(), 1, "clicking a slide thumbnail selects it");
    }
    if f.query_all_by_label("Page 2").next().is_some() {
        f.get_all_by_label("Page 2").next().unwrap().click();
        f.run_steps(3);
        assert_eq!(f.state().app.selected(), 1, "clicking a page in the strip selects it");
    }
    if let Some((names, _)) = f.state().app.grid_sheets() {
        assert!(f.query_all_by_label(&names[0]).next().is_some(), "sheet tab {:?} is shown", names[0]);
        let before = names.len();
        if f.query_all_by_label("Add sheet").next().is_some() {
            f.get_by_label("Add sheet").click();
            f.run_steps(3);
            // A naming form may ask first: type a name into its labelled field and confirm.
            let has_form = f.query_all_by_label("OK").next().is_some();
            if has_form {
                if f.query_all_by_label("Name").next().is_some() {
                    f.get_all_by_label("Name").last().unwrap().focus();
                    f.run_steps(2);
                    f.get_all_by_label("Name").last().unwrap().type_text("Totals");
                    f.run_steps(2);
                }
                f.get_all_by_label("OK").next().unwrap().click();
                f.run_steps(3);
            }
            let after = f.state().app.grid_sheets().map_or(0, |(n, _)| n.len());
            assert_eq!(after, before + 1, "the + tab button adds a sheet ({:?}, form {has_form})", f.state().snack_text());
            f.run_steps(2);
            assert!(f.query_all_by_label("Totals").next().is_some(), "the new sheet appears as a tab");
        }
    }
    h.get_by_label("File").click();
    h.run_steps(3);
    if h.query_all_by_label_contains("Open…").next().is_none() {
        let labels: Vec<String> = h
            .root()
            .children_recursive()
            .filter_map(|n| n.accesskit_node().label())
            .collect();
        panic!("File menu: no Open… item; labels: {labels:?}");
    }
    let save_as = h.get_all_by_label_contains("Save as").last().expect("Save as submenu").rect();
    h.hover_at(save_as.center());
    h.run_steps(4);
    assert!(h.query_all_by_label_contains(".odt").next().is_some() || h.query_all_by_label_contains(".csv").next().is_some()
        || h.query_all_by_label_contains(".pdf").next().is_some() || h.query_all_by_label_contains(".png").next().is_some()
        || h.query_all_by_label_contains(".pptx").next().is_some() || h.query_all_by_label_contains(".json").next().is_some(),
        "Save as submenu lists formats");
    shot(&mut h, &format!("{prefix}-05-file-menu"));
    h.key_press(eframe::egui::Key::Escape);
    h.run_steps(2);

    // Snackbar after an action, and the dark theme.
    let mut h = harness(make(), sample, true);
    if sample.is_some() {
        expect(&h, "Notification", "snackbar after opening");
        // The snackbar stays clear of the sheet-tab bar.
        if let Some(tabs) = h.query_all_by_label("Sheets").next().map(|n| n.rect()) {
            let snack = h.get_by_label("Notification").rect();
            assert!(!snack.intersects(tabs), "the snackbar {snack:?} overlaps the sheet tabs {tabs:?}");
        }
    }
    shot(&mut h, &format!("{prefix}-06-dark"));
    let mut h = harness(make(), None, true);
    shot(&mut h, &format!("{prefix}-07-home-dark"));
}
