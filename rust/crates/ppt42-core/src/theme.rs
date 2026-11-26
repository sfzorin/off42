//! Deck colour and font themes (the six built-in themes of macOS PPT42).
use serde::{Deserialize, Serialize};

/// Scheme slot names in theme order.
pub const SLOTS: [&str; 12] = [
    "dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6", "hlink", "folHlink",
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    pub name: String,
    /// Colours in [`SLOTS`] order.
    pub colors: [[u8; 3]; 12],
    pub major_font: String,
    pub minor_font: String,
}

const fn rgb(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}
const fn colors(v: [u32; 12]) -> [[u8; 3]; 12] {
    let mut out = [[0; 3]; 12];
    let mut i = 0;
    while i < 12 {
        out[i] = rgb(v[i]);
        i += 1;
    }
    out
}

/// Name, colours, heading font and body font. Fonts are the Windows equivalents of the macOS choices.
pub const BUILT_IN: [(&str, [[u8; 3]; 12], &str, &str); 6] = [
    ("Classic", colors([0x1F1F1F, 0xFFFFFF, 0x2B3A55, 0xE8ECF2, 0x2F6FDB, 0xF0883E, 0x8E9AAF, 0xF2C14E, 0x3FB6C6, 0x5DAE5B, 0x2F6FDB, 0x7A4FBF]), "Segoe UI", "Segoe UI"),
    ("Midnight", colors([0xF2F4F8, 0x141B2D, 0xDCE3F0, 0x24304A, 0x4FC3F7, 0xFFB74D, 0x81C784, 0xF06292, 0xBA68C8, 0xFFF176, 0x4FC3F7, 0xBA68C8]), "Segoe UI Semibold", "Segoe UI"),
    ("Sunrise", colors([0x3B2A20, 0xFFF8F0, 0x8A3B12, 0xFBE3CF, 0xE5671A, 0xF2A541, 0xC0392B, 0x7D9D52, 0x4A7C9B, 0x9B59B6, 0xE5671A, 0x9B59B6]), "Gill Sans MT", "Gill Sans MT"),
    ("Forest", colors([0x1E2B22, 0xF6F8F3, 0x2E5E3E, 0xDDE8D6, 0x2E8B57, 0x8FB339, 0xD9A441, 0x4F7CAC, 0xB5563A, 0x6B8F71, 0x2E8B57, 0x6B8F71]), "Georgia", "Segoe UI"),
    ("Slate", colors([0xECEFF1, 0x263238, 0xCFD8DC, 0x37474F, 0xFF7043, 0x26C6DA, 0xFFCA28, 0x9CCC65, 0xAB47BC, 0x78909C, 0x26C6DA, 0xAB47BC]), "Century Gothic", "Segoe UI"),
    ("Paper", colors([0x2B2B2B, 0xFBF7EE, 0x5A4632, 0xEFE6D2, 0x8C5A3C, 0x3D6B8C, 0xA67C2E, 0x6E8B3D, 0x9C3D54, 0x5E5E5E, 0x3D6B8C, 0x9C3D54]), "Palatino Linotype", "Palatino Linotype"),
];

impl Theme {
    pub fn built_in(name: &str) -> Option<Self> {
        BUILT_IN.iter().find(|t| t.0.eq_ignore_ascii_case(name.trim())).map(|t| Self {
            name: t.0.into(),
            colors: t.1,
            major_font: t.2.into(),
            minor_font: t.3.into(),
        })
    }
    pub fn names() -> Vec<String> {
        BUILT_IN.iter().map(|t| t.0.to_owned()).collect()
    }
    pub fn text(&self) -> [u8; 3] {
        self.colors[0]
    }
    pub fn background(&self) -> [u8; 3] {
        self.colors[1]
    }
    pub fn accent(&self, index: usize) -> [u8; 3] {
        self.colors[4 + index % 6]
    }
    pub fn valid(&self) -> bool {
        !self.name.is_empty()
            && self.name.len() <= 256
            && [&self.major_font, &self.minor_font].iter().all(|f| !f.is_empty() && f.len() <= 256)
    }
    /// The `a:theme` part.
    pub fn drawingml(&self) -> String {
        let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;");
        let mut colors = String::new();
        for (slot, c) in SLOTS.iter().zip(self.colors) {
            colors.push_str(&format!("<a:{slot}><a:srgbClr val=\"{:02X}{:02X}{:02X}\"/></a:{slot}>", c[0], c[1], c[2]));
        }
        let solid = "<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>";
        let name = esc(&self.name);
        format!("<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"{name}\"><a:themeElements><a:clrScheme name=\"{name}\">{colors}</a:clrScheme><a:fontScheme name=\"{name}\"><a:majorFont><a:latin typeface=\"{}\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont><a:minorFont><a:latin typeface=\"{}\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont></a:fontScheme><a:fmtScheme name=\"{name}\"><a:fillStyleLst>{}</a:fillStyleLst><a:lnStyleLst>{}</a:lnStyleLst><a:effectStyleLst>{}</a:effectStyleLst><a:bgFillStyleLst>{}</a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>",
            esc(&self.major_font), esc(&self.minor_font), solid.repeat(3), format!("<a:ln>{solid}</a:ln>").repeat(3),
            "<a:effectStyle><a:effectLst/></a:effectStyle>".repeat(3), solid.repeat(3))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read};

    fn entry(bytes: &[u8], path: &str) -> String {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut out = String::new();
        archive.by_name(path).unwrap().read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn theme_is_one_undo_step_and_persists_in_project_pptx_and_odp() {
        let mut deck = crate::Presentation::new();
        deck.edit("Quarterly".into(), "Results".into()).unwrap();
        let midnight = Theme::built_in("midnight").unwrap();
        assert_eq!(midnight.background(), [0x14, 0x1B, 0x2D]);
        deck.set_theme(Some(midnight.clone())).unwrap();
        let id = deck.insert_object(crate::ObjectKind::Text, "Note".into()).unwrap();
        let note = deck.slides()[0].objects.iter().find(|o| o.id == id).unwrap();
        assert_eq!(note.text_style.color, midnight.text());
        assert!(deck.undo() && deck.undo());
        assert!(deck.theme().is_none());
        assert!(deck.redo());
        assert_eq!(deck.theme(), Some(&midnight));
        let (project, _) = deck.save_snapshot().unwrap();
        let path = std::env::temp_dir().join(format!("ppt42-theme-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, &project).unwrap();
        let reopened = crate::Presentation::open(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(reopened.theme(), Some(&midnight));

        let pptx = crate::pptx::export_new(&deck).unwrap();
        let theme = entry(&pptx, "ppt/theme/theme1.xml");
        assert!(theme.contains("name=\"Midnight\"") && theme.contains("<a:lt1><a:srgbClr val=\"141B2D\"/>"));
        assert!(entry(&pptx, "ppt/slides/slide1.xml").contains("F2F4F8"));
        let (opened, source) = crate::pptx::import(&pptx).unwrap();
        assert_eq!(source.background(opened.slides()[0].id), Some(midnight.background()));

        let odp = crate::odp::export(&deck).unwrap();
        let (_, source) = crate::odp::import_with_source(&odp).unwrap();
        assert_eq!(source.background(), Some(midnight.background()));
        assert!(entry(&odp, "content.xml").contains("#f2f4f8"));
        assert!(Theme::built_in("Nope").is_none());
        assert_eq!(Theme::names().len(), 6);
    }
}
