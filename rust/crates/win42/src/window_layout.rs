//! Per-app native placement preferences; no document paths or content.
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct Panels {
    pub sidebar: bool,
    pub toolbar: bool,
    pub status: bool,
    pub rulers: bool,
}
impl Default for Panels {
    fn default() -> Self {
        Self {
            sidebar: true,
            toolbar: true,
            status: true,
            rulers: true,
        }
    }
}
impl Panels {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Layout {
    version: u8,
    pub normal: [i32; 4],
    pub maximized: bool,
    #[serde(default, skip_serializing_if = "Panels::is_default")]
    pub panels: Panels,
}
impl Layout {
    pub fn new(normal: [i32; 4], maximized: bool) -> Option<Self> {
        let layout = Self {
            version: 1,
            normal,
            maximized,
            panels: Panels::default(),
        };
        layout.valid().then_some(layout)
    }
    fn valid(&self) -> bool {
        let [l, t, r, b] = self.normal;
        self.version == 1
            && self.normal.iter().all(|n| n.unsigned_abs() <= 200_000)
            && r.checked_sub(l).is_some_and(|n| (64..=32768).contains(&n))
            && b.checked_sub(t).is_some_and(|n| (64..=32768).contains(&n))
    }
}
fn location(app: &str) -> Option<PathBuf> {
    if app.is_empty() || app.len() > 32 || !app.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    Some(
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("off42")
            .join("Window Layout")
            .join(format!("{app}.json")),
    )
}
fn read_at(path: &Path) -> Option<Layout> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 4096 {
        return None;
    }
    let layout: Layout = serde_json::from_slice(&bytes).ok()?;
    layout.valid().then_some(layout)
}
pub(crate) fn load(app: &str) -> Option<Layout> {
    read_at(&location(app)?)
}
fn write_at(path: &Path, layout: &Layout) -> Result<(), String> {
    if !layout.valid() {
        return Err("Invalid window layout".into());
    }
    std::fs::create_dir_all(path.parent().ok_or("Invalid layout location")?)
        .map_err(|e| e.to_string())?;
    let identity = if path.exists() {
        Some(suite42_core::session::FileIdentity::read(path).map_err(|e| e.to_string())?)
    } else {
        None
    };
    let main = serde_json::to_vec(layout).map_err(|e| e.to_string())?;
    suite42_core::session::publish_bundle(
        path,
        &suite42_core::session::SaveBundle {
            main,
            sidecars: vec![],
        },
        identity.as_ref(),
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}
pub(crate) fn save(app: &str, layout: &Layout) -> Result<(), String> {
    write_at(
        &location(app).ok_or("Window preferences unavailable")?,
        layout,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn placement_round_trips_atomically_and_rejects_corrupt_or_excessive_preferences() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("layout.json");
        let layout = Layout::new([-800, 40, 0, 640], true).unwrap();
        write_at(&path, &layout).unwrap();
        assert_eq!(read_at(&path), Some(layout));
        let mut second = Layout::new([40, 50, 940, 650], false).unwrap();
        second.panels = Panels {
            sidebar: false,
            toolbar: false,
            status: false,
            rulers: false,
        };
        write_at(&path, &second).unwrap();
        assert_eq!(read_at(&path), Some(second));
        let old = r#"{"version":1,"normal":[0,0,800,600],"maximized":false}"#;
        std::fs::write(&path, old).unwrap();
        assert_eq!(read_at(&path).unwrap().panels, Panels::default());
        for text in [
            r#"{"version":2,"normal":[0,0,800,600],"maximized":false}"#,
            r#"{"version":1,"normal":[0,0,-1,600],"maximized":false}"#,
            r#"{"version":1,"normal":[0,0,800,600],"maximized":false,"document":"private"}"#,
            "{broken",
        ] {
            std::fs::write(&path, text).unwrap();
            assert_eq!(read_at(&path), None);
        }
        std::fs::write(&path, vec![b' '; 4097]).unwrap();
        assert_eq!(read_at(&path), None);
        assert!(Layout::new([i32::MIN, 0, i32::MAX, 500], false).is_none());
        assert!(Layout::new([0, 0, 1, 1], false).is_none());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
