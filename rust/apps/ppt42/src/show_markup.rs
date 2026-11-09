use std::collections::BTreeMap;
use win42::{CanvasEvent, CanvasRect, Point, SceneItem};

const RED: [u8; 3] = [230, 32, 36];
const MAX_POINTS: usize = 100_000;
const MAX_STROKE: usize = 4_096;

#[derive(Default, PartialEq, Eq)]
enum Tool {
    #[default]
    Cursor,
    Laser,
    Pen,
}

#[derive(Default)]
pub(super) struct Markup {
    tool: Tool,
    cursor_hidden: bool,
    laser: Option<Point>,
    ink: BTreeMap<usize, Vec<Vec<Point>>>,
    stroke: Vec<Point>,
    points: usize,
}

impl Markup {
    pub(super) fn track_hover(&self) -> bool {
        self.tool == Tool::Laser
    }

    pub(super) fn hide_cursor(&self) -> bool {
        self.cursor_hidden || self.track_hover()
    }

    pub(super) fn moved_slide(&mut self) {
        self.laser = None;
        self.stroke.clear();
    }

    pub(super) fn handle(&mut self, event: CanvasEvent, index: usize, blank: bool) -> bool {
        use CanvasEvent::*;
        match event {
            Key {
                code: 76,
                modifiers: 0,
            } => {
                self.tool = Tool::Laser;
                self.moved_slide();
            }
            Key {
                code: 80,
                modifiers: 0,
            } => {
                self.tool = Tool::Pen;
                self.moved_slide();
            }
            Key {
                code: 67,
                modifiers: 0,
            } => self.cursor_hidden = !self.cursor_hidden,
            Key {
                code: 69,
                modifiers: 0,
            } => {
                if let Some(strokes) = self.ink.remove(&index) {
                    self.points -= strokes.iter().map(Vec::len).sum::<usize>();
                }
                self.stroke.clear();
            }
            Key { code: 27, .. } if self.tool != Tool::Cursor => {
                self.tool = Tool::Cursor;
                self.moved_slide();
            }
            Cancel => self.moved_slide(),
            _ if blank => return false,
            PointerMove(point) if self.tool == Tool::Laser => {
                self.laser = inside(point).then_some(point);
            }
            PointerDown(point) | PointerDownModified { point, .. } if self.tool == Tool::Pen => {
                self.stroke.clear();
                if inside(point) && self.points < MAX_POINTS {
                    self.stroke.push(point);
                }
            }
            PointerMove(point) if self.tool == Tool::Pen => self.extend_stroke(point),
            PointerUp(point) if self.tool == Tool::Pen => {
                self.extend_stroke(point);
                if !self.stroke.is_empty() {
                    self.points += self.stroke.len();
                    self.ink
                        .entry(index)
                        .or_default()
                        .push(std::mem::take(&mut self.stroke));
                }
            }
            _ => return false,
        }
        true
    }

    fn extend_stroke(&mut self, point: Point) {
        if inside(point)
            && self.stroke.len() < MAX_STROKE
            && self.points + self.stroke.len() < MAX_POINTS
            && self
                .stroke
                .last()
                .is_some_and(|last| (point.x - last.x).hypot(point.y - last.y) >= 1.0)
        {
            self.stroke.push(point);
        }
    }

    pub(super) fn add_overlay(&self, index: usize, scene: &mut Vec<SceneItem>) {
        let mut items = Vec::new();
        for stroke in self
            .ink
            .get(&index)
            .into_iter()
            .flatten()
            .chain([&self.stroke])
        {
            match stroke.as_slice() {
                [] => {}
                [point] => items.push(dot(*point, 2.0)),
                points => items.push(SceneItem::Path {
                    points: points.to_vec(),
                    color: RED,
                    closed: false,
                }),
            }
        }
        if let Some(point) = self.laser {
            let diameter = 18.0;
            items.push(SceneItem::OpacityLayer {
                bounds: CanvasRect {
                    x: point.x - diameter / 2.0,
                    y: point.y - diameter / 2.0,
                    width: diameter,
                    height: diameter,
                },
                opacity: 90,
                items: vec![dot(point, diameter / 2.0)],
            });
            items.push(dot(point, 4.5));
        }
        if !items.is_empty() {
            scene.push(SceneItem::ClippedOffset {
                clip: CanvasRect {
                    x: 0.0,
                    y: 0.0,
                    width: 720.0,
                    height: 405.0,
                },
                offset: Point { x: 0.0, y: 0.0 },
                items,
            });
        }
    }
}

fn dot(point: Point, radius: f64) -> SceneItem {
    SceneItem::Ellipse {
        x: point.x - radius,
        y: point.y - radius,
        width: radius * 2.0,
        height: radius * 2.0,
        fill: Some(RED),
        stroke: None,
    }
}

fn inside(point: Point) -> bool {
    (0.0..720.0).contains(&point.x) && (0.0..405.0).contains(&point.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pen_and_laser_are_ephemeral_and_bound_to_the_current_slide() {
        let mut markup = Markup::default();
        let a = Point { x: 35.0, y: 80.0 };
        let b = Point { x: 48.0, y: 82.0 };
        let key = |code| CanvasEvent::Key { code, modifiers: 0 };
        assert!(markup.handle(key(80), 0, false));
        assert!(markup.handle(CanvasEvent::PointerDown(a), 0, false));
        assert!(markup.handle(CanvasEvent::PointerUp(b), 0, false));
        assert_eq!(markup.points, 2);
        assert_eq!(markup.ink[&0], vec![vec![a, b]]);
        markup.moved_slide();
        let mut scene = Vec::new();
        markup.add_overlay(1, &mut scene);
        assert!(scene.is_empty());
        markup.add_overlay(0, &mut scene);
        assert_eq!(scene.len(), 1);
        assert!(markup.handle(key(69), 0, false));
        assert_eq!(markup.points, 0);
        assert!(markup.handle(key(76), 0, false));
        assert!(markup.hide_cursor());
        assert!(markup.handle(CanvasEvent::PointerMove(a), 0, false));
        assert_eq!(markup.laser, Some(a));
        assert!(markup.handle(CanvasEvent::Cancel, 0, false));
        assert_eq!(markup.laser, None);
        assert!(markup.handle(key(27), 0, false));
        assert!(!markup.hide_cursor());
        assert!(!markup.handle(key(27), 0, false));
    }
}
