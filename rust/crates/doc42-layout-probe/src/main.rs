//! Bounded, offline pagination experiment; this is not a DOCX engine.
use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};
use std::{error::Error, fs, path::Path};

const FONT: &[u8] = include_bytes!("../fixtures/NotoSans.ttf");
const W: usize = 600;
const H: usize = 420;
const M: f32 = 32.0;
const MAX_PAGES: usize = 64;
const MAX_BYTES: usize = 48_000;

struct Page {
    pixels: Vec<u8>,
}
impl Page {
    fn new() -> Self {
        Self {
            pixels: vec![255; W * H * 3],
        }
    }
    fn pixel(&mut self, x: i32, y: i32, a: u8) -> Result<(), String> {
        if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
            return Err("glyph extends outside page".into());
        }
        let i = (y as usize * W + x as usize) * 3;
        for c in 0..3 {
            self.pixels[i + c] = ((self.pixels[i + c] as u16 * (255 - a as u16)) / 255) as u8;
        }
        Ok(())
    }
    fn line(&mut self, x1: usize, y1: usize, x2: usize, y2: usize) {
        for y in y1..=y2 {
            for x in x1..=x2 {
                if x == x1 || x == x2 || y == y1 || y == y2 {
                    let i = (y * W + x) * 3;
                    self.pixels[i..i + 3].fill(100);
                }
            }
        }
    }
}
struct Line {
    glyphs: Vec<cosmic_text::LayoutGlyph>,
    baseline: f32,
    start: usize,
    end: usize,
}
struct Shaped {
    lines: Vec<Line>,
}
fn shape(fs: &mut FontSystem, text: &str, width: f32, size: f32) -> Result<Shaped, String> {
    if text.contains('\n') || text.is_empty() {
        return Err("blocks must be nonempty single paragraphs".into());
    }
    let mut b = Buffer::new(fs, Metrics::new(size, size * 1.5));
    b.set_size(fs, Some(width), None);
    b.set_text(
        fs,
        text,
        &Attrs::new().family(Family::Name("Noto Sans")),
        Shaping::Advanced,
    );
    b.shape_until_scroll(fs, false);
    let mut covered = vec![false; text.len()];
    let mut lines = Vec::new();
    for run in b.layout_runs() {
        let mut start = text.len();
        let mut end = 0;
        for g in run.glyphs {
            if g.glyph_id == 0 {
                return Err(format!("font lacks glyph near byte {}", g.start));
            }
            if g.end > text.len() || g.start >= g.end {
                return Err("invalid shaped cluster".into());
            }
            for byte in &mut covered[g.start..g.end] {
                *byte = true;
            }
            start = start.min(g.start);
            end = end.max(g.end);
            if g.x < -0.01 || g.x + g.w > width + 0.5 {
                return Err("line exceeds available width".into());
            }
        }
        if start == text.len() {
            return Err("empty shaped line".into());
        }
        lines.push(Line {
            glyphs: run.glyphs.to_vec(),
            baseline: run.line_y - run.line_top,
            start,
            end,
        });
    }
    // Shaping omits spaces consumed at wrap boundaries; assign each such byte to
    // the preceding visual line so the trace covers the complete source.
    for (i, &seen) in covered.iter().enumerate() {
        if !seen {
            if text.as_bytes()[i] != b' ' {
                return Err(format!("uncovered non-space byte {i}"));
            }
            let line = lines
                .iter_mut()
                .rev()
                .find(|line| line.end == i)
                .ok_or_else(|| format!("unassigned space at byte {i}"))?;
            line.end = i + 1;
        }
    }
    let mut next = 0;
    for line in &lines {
        if line.start != next {
            return Err("non-contiguous visual line coverage".into());
        }
        next = line.end;
    }
    if next != text.len() {
        return Err("incomplete source coverage".into());
    }
    Ok(Shaped { lines })
}
struct Layout {
    fs: FontSystem,
    cache: SwashCache,
    pages: Vec<Page>,
    trace: Vec<String>,
    y: f32,
    size: f32,
    width: f32,
}
impl Layout {
    fn new(width: f32, size: f32) -> Result<Self, String> {
        if !(160.0..=536.0).contains(&width) || !(10.0..=40.0).contains(&size) {
            return Err("width must be 160..536 and font size 10..40".into());
        }
        let mut db = cosmic_text::fontdb::Database::new();
        db.load_font_data(FONT.to_vec());
        if db.faces().next().is_none() {
            return Err("bundled Noto Sans font unavailable".into());
        }
        Ok(Self {
            fs: FontSystem::new_with_locale_and_db("en-US".into(), db),
            cache: SwashCache::new(),
            pages: vec![Page::new()],
            trace: Vec::new(),
            y: M,
            size,
            width,
        })
    }
    fn line_height(&self) -> f32 {
        self.size * 1.5
    }
    fn ensure(&mut self, h: f32) -> Result<(), String> {
        if h > H as f32 - 2.0 * M {
            return Err("table row or line taller than page".into());
        }
        if self.y + h > H as f32 - M {
            if self.pages.len() == MAX_PAGES {
                return Err("page limit exceeded".into());
            }
            self.pages.push(Page::new());
            self.y = M;
        }
        Ok(())
    }
    fn draw_line(
        &mut self,
        line: &Line,
        x: f32,
        y: f32,
        width: f32,
        id: &str,
    ) -> Result<(), String> {
        let page = self.pages.len() - 1;
        let mut failure = None;
        for g in &line.glyphs {
            let p = g.physical((x, 0.0), 1.0);
            self.cache.with_pixels(
                &mut self.fs,
                p.cache_key,
                Color::rgb(0, 0, 0),
                |dx, dy, c| {
                    let px = p.x + dx;
                    let py = (y + line.baseline) as i32 + p.y + dy;
                    if px < x.floor() as i32
                        || px >= (x + width).ceil() as i32
                        || py < y.floor() as i32
                        || py >= (y + self.size * 1.5).ceil() as i32
                    {
                        failure = Some("glyph ink exceeds line box".to_string());
                    } else if let Err(e) = self.pages[page].pixel(px, py, c.a()) {
                        failure = Some(e);
                    }
                },
            );
        }
        if let Some(e) = failure {
            return Err(e);
        }
        self.trace.push(format!("{{\"block\":\"{}\",\"page\":{},\"start\":{},\"end\":{},\"x\":{:.2},\"y\":{:.2},\"width\":{:.2},\"height\":{:.2}}}",id,page+1,line.start,line.end,x,y,width,self.line_height()));
        Ok(())
    }
    fn paragraph(&mut self, id: &str, text: &str) -> Result<(), String> {
        let shaped = shape(&mut self.fs, text, self.width, self.size)?;
        for line in &shaped.lines {
            self.ensure(self.line_height())?;
            self.draw_line(line, M, self.y, self.width, id)?;
            self.y += self.line_height();
        }
        self.y += self.size * 0.5;
        Ok(())
    }
    fn row(&mut self, id: &str, cells: [&str; 2]) -> Result<(), String> {
        let width = self.width / 2.0;
        let inner = width - 12.0;
        let a = shape(&mut self.fs, cells[0], inner, self.size)?;
        let b = shape(&mut self.fs, cells[1], inner, self.size)?;
        let h = a.lines.len().max(b.lines.len()) as f32 * self.line_height() + 12.0;
        self.ensure(h)?;
        let page = self.pages.len() - 1;
        for (i, shaped) in [a, b].iter().enumerate() {
            let x = M + i as f32 * width;
            self.pages[page].line(
                x as usize,
                self.y as usize,
                (x + width) as usize,
                (self.y + h) as usize,
            );
            for (j, line) in shaped.lines.iter().enumerate() {
                self.draw_line(
                    line,
                    x + 6.0,
                    self.y + 6.0 + j as f32 * self.line_height(),
                    inner,
                    &format!("{}-cell{}", id, i + 1),
                )?;
            }
        }
        self.y += h;
        Ok(())
    }
}
fn fixture(width: f32, size: f32) -> Result<Layout, String> {
    let mut l = Layout::new(width, size)?;
    let body="Office affinity: cafe\u{301} naïve résumé — Русский and English. The quick brown fox moves through this paragraph, testing shaped clusters and line wrapping. ".repeat(8);
    if body.len() > MAX_BYTES {
        return Err("input byte limit exceeded".into());
    }
    l.paragraph("before", &body)?;
    for i in 0..5 {
        l.row(
            &format!("row{}", i + 1),
            [
                "A cell with wrapped text and café",
                "Second cell: office, Ελληνικά, and more text.",
            ],
        )?;
    }
    l.paragraph(
        "after",
        "Text after the table must reach the final character: END.",
    )?;
    Ok(l)
}
fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 || args.len() > 4 {
        return Err("usage: doc42-layout-probe OUTPUT_DIR [WIDTH_PX] [FONT_PX]".into());
    }
    let width = args.get(2).map(|v| v.parse()).transpose()?.unwrap_or(450.0);
    let size = args.get(3).map(|v| v.parse()).transpose()?.unwrap_or(18.0);
    let l = fixture(width, size)?;
    let out = Path::new(&args[1]);
    if out.exists() && fs::read_dir(out)?.next().is_some() {
        return Err("output directory must be empty".into());
    }
    fs::create_dir_all(out)?;
    for (i, p) in l.pages.iter().enumerate() {
        let f = fs::File::create(out.join(format!("page-{:03}.png", i + 1)))?;
        let mut e = png::Encoder::new(f, W as u32, H as u32);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        e.write_header()?.write_image_data(&p.pixels)?;
    }
    fs::write(out.join("trace.json"),format!("{{\"page_width\":{},\"page_height\":{},\"content_width\":{},\"font_size\":{},\"pages\":{},\"lines\":[{}]}}\n",W,H,width,size,l.pages.len(),l.trace.join(",")))?;
    println!(
        "{} pages; trace and PNGs written to {}",
        l.pages.len(),
        out.display()
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("doc42-layout-probe: {e}");
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn covers_and_reflows() {
        let a = fixture(450.0, 18.0).unwrap();
        let b = fixture(450.0, 18.0).unwrap();
        let narrow = fixture(220.0, 18.0).unwrap();
        let larger = fixture(450.0, 26.0).unwrap();
        assert!(a.pages.len() > 1);
        assert_eq!(a.trace, b.trace);
        assert!(narrow.pages.len() > a.pages.len());
        assert!(larger.pages.len() > a.pages.len());
        assert!(a.trace.last().unwrap().contains("\"block\":\"after\""));
        assert!(a.trace.last().unwrap().contains(&format!(
            "\"end\":{}",
            "Text after the table must reach the final character: END.".len()
        )));
    }
    #[test]
    fn rejects_missing_glyph_and_impossible_geometry() {
        let mut l = Layout::new(450.0, 18.0).unwrap();
        assert!(shape(&mut l.fs, "🦀", 200.0, 18.0).is_err());
        assert!(Layout::new(20.0, 18.0).is_err());
        assert!(l.ensure(1000.0).is_err());
    }
}
