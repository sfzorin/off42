use std::time::Instant;
use win42::{CanvasEvent, CanvasRect, CanvasScene, Point, SceneFont, SceneItem, TextAlign};

const COLUMNS: usize = 4;
const PAGE_SIZE: usize = 12;
pub(super) const THUMBNAIL: (u32, u32) = (162, 91);

pub(super) enum Outcome {
    Stay,
    Close,
    Jump(usize),
    End,
}

pub(super) struct Overview {
    indices: Vec<usize>,
    selected: usize,
    thumbnails: Vec<Vec<u8>>,
    pub(super) opened: Instant,
}

impl Overview {
    pub(super) fn new(indices: Vec<usize>, current: usize, opened: Instant) -> Option<Self> {
        let selected = indices.iter().position(|&index| index == current)?;
        Some(Self {
            indices,
            selected,
            opened,
            thumbnails: Vec::new(),
        })
    }

    pub(super) fn page(&self) -> usize {
        self.selected / PAGE_SIZE
    }

    fn visible(&self) -> &[usize] {
        let start = self.page() * PAGE_SIZE;
        &self.indices[start..(start + PAGE_SIZE).min(self.indices.len())]
    }

    pub(super) fn load(
        &mut self,
        mut render: impl FnMut(usize) -> Result<Vec<u8>, String>,
    ) -> Result<(), String> {
        self.thumbnails = self
            .visible()
            .iter()
            .copied()
            .map(&mut render)
            .collect::<Result<_, _>>()?;
        Ok(())
    }

    pub(super) fn handle(&mut self, event: CanvasEvent) -> Outcome {
        use CanvasEvent::*;
        let step = match event {
            Key { code: 27 | 71, .. } => return Outcome::Close,
            Key { code: 81, .. } => return Outcome::End,
            Key { code: 13 | 32, .. } => return Outcome::Jump(self.indices[self.selected]),
            Key { code: 36, .. } => -(self.indices.len() as isize),
            Key { code: 35, .. } => self.indices.len() as isize,
            Key { code: 37, .. } => -1,
            Key { code: 39, .. } => 1,
            Key { code: 38, .. } => -(COLUMNS as isize),
            Key { code: 40, .. } => COLUMNS as isize,
            Key { code: 33, .. } => -(PAGE_SIZE as isize),
            Key { code: 34, .. } => PAGE_SIZE as isize,
            PointerUp(point) => {
                return self
                    .visible()
                    .iter()
                    .copied()
                    .enumerate()
                    .find(|(slot, _)| contains(rect(*slot), point))
                    .map_or(Outcome::Stay, |(_, index)| Outcome::Jump(index));
            }
            _ => 0,
        };
        self.selected = self
            .selected
            .saturating_add_signed(step)
            .min(self.indices.len() - 1);
        Outcome::Stay
    }

    pub(super) fn scene(&self) -> CanvasScene {
        let mut scene = CanvasScene::default();
        scene.items.push(SceneItem::Rect {
            x: 0.0,
            y: 0.0,
            width: 720.0,
            height: 405.0,
            fill: [28, 30, 36],
            stroke: [28, 30, 36],
        });
        scene.items.push(label(
            win42::localization::text("Overview"),
            14.0,
            12.0,
            230.0,
            15.0,
            TextAlign::Left,
        ));
        let page = win42::localization::text("Page %d of %d")
            .replacen("%d", &(self.page() + 1).to_string(), 1)
            .replacen("%d", &self.indices.len().div_ceil(PAGE_SIZE).to_string(), 1);
        scene
            .items
            .push(label(page, 456.0, 12.0, 248.0, 13.0, TextAlign::Right));
        for (slot, (&index, pixels)) in self.visible().iter().zip(&self.thumbnails).enumerate() {
            let bounds = rect(slot);
            let selected = self.indices[self.selected] == index;
            if selected {
                scene.items.push(SceneItem::Rect {
                    x: bounds.x - 3.0,
                    y: bounds.y - 3.0,
                    width: bounds.width + 6.0,
                    height: bounds.height + 6.0,
                    fill: [122, 187, 255],
                    stroke: [122, 187, 255],
                });
            }
            scene.items.push(SceneItem::ImageScaled {
                x: bounds.x,
                y: bounds.y,
                width: bounds.width,
                height: bounds.height,
                pixel_width: THUMBNAIL.0,
                pixel_height: THUMBNAIL.1,
                rgba: pixels.clone(),
            });
            scene.items.push(label(
                (index + 1).to_string(),
                bounds.x,
                bounds.y + bounds.height + 6.0,
                bounds.width,
                12.0,
                TextAlign::Center,
            ));
        }
        scene
    }
}

fn label(text: String, x: f64, y: f64, width: f64, points: f64, align: TextAlign) -> SceneItem {
    SceneItem::TextBox {
        x,
        y,
        width,
        height: 20.0,
        text,
        color: [236; 3],
        align,
        wrap: false,
        font: SceneFont {
            family: "Arial".into(),
            points,
            bold: false,
            italic: false,
            underline: false,
        },
    }
}

fn rect(slot: usize) -> CanvasRect {
    CanvasRect {
        x: 18.0 + (slot % COLUMNS) as f64 * 174.0,
        y: 45.0 + (slot / COLUMNS) as f64 * 115.0,
        width: f64::from(THUMBNAIL.0),
        height: f64::from(THUMBNAIL.1),
    }
}

fn contains(rect: CanvasRect, point: Point) -> bool {
    (rect.x..rect.x + rect.width).contains(&point.x)
        && (rect.y..rect.y + rect.height).contains(&point.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paging_and_hit_testing_use_only_the_show_order() {
        let mut view =
            Overview::new((0..30).filter(|&i| i != 2).collect(), 0, Instant::now()).unwrap();
        let key = |code| CanvasEvent::Key { code, modifiers: 0 };
        assert_eq!(&view.visible()[..3], &[0, 1, 3]);
        assert!(matches!(view.handle(key(34)), Outcome::Stay));
        assert_eq!(view.page(), 1);
        assert!(matches!(view.handle(key(13)), Outcome::Jump(13)));
        assert!(matches!(
            view.handle(CanvasEvent::PointerUp(Point { x: 190.0, y: 80.0 })),
            Outcome::Stay
        ));
        assert!(matches!(
            view.handle(CanvasEvent::PointerUp(Point { x: 198.0, y: 80.0 })),
            Outcome::Jump(14)
        ));
        assert!(matches!(view.handle(key(27)), Outcome::Close));
    }
}
