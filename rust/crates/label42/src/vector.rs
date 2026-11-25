//! Resource-free PDF paths for the built-in symbols, codes and outlined captions.
use crate::{TechnicalElement, PT_PER_MM};
use resvg::{tiny_skia, usvg};
use std::fmt::Write;

pub struct TechnicalVector {
    pub width_points: f64,
    pub height_points: f64,
    pub content: Vec<u8>,
}
impl TechnicalElement {
    pub fn pdf(&self) -> Result<Vec<u8>, String> {
        use lopdf::{dictionary, Document, Object, Stream};
        let vector = self.vector()?;
        let mut doc = Document::with_version("1.5");
        let pages = doc.new_object_id();
        let bbox = vec![
            0.into(),
            0.into(),
            Object::Real(vector.width_points as f32),
            Object::Real(vector.height_points as f32),
        ];
        let appearance = doc.add_object(Stream::new(dictionary! {"Type"=>"XObject", "Subtype"=>"Form", "BBox"=>bbox.clone(), "Resources"=>dictionary! {}}, vector.content));
        let annotation = doc.add_object(dictionary! {"Type"=>"Annot", "Subtype"=>"Stamp", "Rect"=>bbox.clone(), "F"=>4, "AP"=>dictionary! {"N"=>appearance}, "Suite42TechnicalElement"=>lopdf::text_string(&self.description()?), "Contents"=>lopdf::text_string(&self.value)});
        let contents = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
        let page = doc.add_object(dictionary! {"Type"=>"Page", "Parent"=>pages, "Contents"=>contents, "Resources"=>dictionary! {}, "MediaBox"=>bbox, "Annots"=>vec![Object::Reference(annotation)]});
        doc.objects.insert(
            pages,
            Object::Dictionary(
                dictionary! {"Type"=>"Pages", "Kids"=>vec![Object::Reference(page)], "Count"=>1},
            ),
        );
        let root = doc.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>pages, "ViewerPreferences"=>dictionary! {"PrintScaling"=>"None"}});
        doc.trailer.set("Root", root);
        doc.compress();
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).map_err(|e| e.to_string())?;
        Ok(bytes)
    }
    pub fn vector(&self) -> Result<TechnicalVector, String> {
        let mut options = usvg::Options::default();
        options.fontdb_mut().load_font_data(
            include_bytes!("../../doc42-layout-probe/fixtures/NotoSans.ttf").to_vec(),
        );
        let tree = usvg::Tree::from_str(&self.svg()?, &options).map_err(|e| e.to_string())?;
        let width_points = self.width_mm * PT_PER_MM;
        let height_points =
            width_points * f64::from(tree.size().height()) / f64::from(tree.size().width());
        let sx = width_points / f64::from(tree.size().width());
        let sy = height_points / f64::from(tree.size().height());
        let mut out = format!("q {sx:.9} 0 0 {:.9} 0 {height_points:.9} cm\n", -sy);
        group(tree.root(), tiny_skia::Transform::identity(), &mut out)?;
        out.push_str("Q\n");
        if out.len() > 8 * 1024 * 1024 {
            return Err("Technical vector is too large".into());
        }
        Ok(TechnicalVector {
            width_points,
            height_points,
            content: out.into_bytes(),
        })
    }
}
fn geometry(path: &tiny_skia::Path, out: &mut String) {
    use tiny_skia::PathSegment::*;
    let mut current = tiny_skia::Point::from_xy(0.0, 0.0);
    let mut start = current;
    for segment in path.segments() {
        match segment {
            MoveTo(p) => {
                writeln!(out, "{:.6} {:.6} m", p.x, p.y).unwrap();
                start = p;
                current = p;
            }
            LineTo(p) => {
                writeln!(out, "{:.6} {:.6} l", p.x, p.y).unwrap();
                current = p;
            }
            QuadTo(control, end) => {
                let a = tiny_skia::Point::from_xy(
                    current.x + (control.x - current.x) * 2.0 / 3.0,
                    current.y + (control.y - current.y) * 2.0 / 3.0,
                );
                let b = tiny_skia::Point::from_xy(
                    end.x + (control.x - end.x) * 2.0 / 3.0,
                    end.y + (control.y - end.y) * 2.0 / 3.0,
                );
                writeln!(
                    out,
                    "{:.6} {:.6} {:.6} {:.6} {:.6} {:.6} c",
                    a.x, a.y, b.x, b.y, end.x, end.y
                )
                .unwrap();
                current = end;
            }
            CubicTo(a, b, end) => {
                writeln!(
                    out,
                    "{:.6} {:.6} {:.6} {:.6} {:.6} {:.6} c",
                    a.x, a.y, b.x, b.y, end.x, end.y
                )
                .unwrap();
                current = end;
            }
            Close => {
                out.push_str("h\n");
                current = start;
            }
        }
    }
}
fn color(paint: &usvg::Paint, opacity: f32, out: &mut String) -> Result<bool, String> {
    if opacity == 0.0 {
        return Ok(false);
    }
    if opacity != 1.0 {
        return Err("Unsupported partial vector opacity".into());
    }
    let usvg::Paint::Color(color) = paint else {
        return Err("Unsupported vector gradient".into());
    };
    writeln!(
        out,
        "{:.6} {:.6} {:.6} rg",
        f64::from(color.red) / 255.0,
        f64::from(color.green) / 255.0,
        f64::from(color.blue) / 255.0
    )
    .unwrap();
    Ok(true)
}
fn fill(path: &usvg::Path, out: &mut String) -> Result<(), String> {
    let Some(fill) = path.fill() else {
        return Ok(());
    };
    if color(fill.paint(), fill.opacity().get(), out)? {
        geometry(path.data(), out);
        out.push_str(if fill.rule() == usvg::FillRule::EvenOdd {
            "f*\n"
        } else {
            "f\n"
        });
    }
    Ok(())
}
fn stroke(path: &usvg::Path, out: &mut String) -> Result<(), String> {
    let Some(stroke) = path.stroke() else {
        return Ok(());
    };
    if color(stroke.paint(), stroke.opacity().get(), out)? {
        let Some(outline) = path.data().stroke(&stroke.to_tiny_skia(), 10.0) else {
            return Err("Could not outline technical stroke".into());
        };
        geometry(&outline, out);
        out.push_str("f\n");
    }
    Ok(())
}
fn clip(
    group: &usvg::Group,
    prefix: tiny_skia::Transform,
    out: &mut String,
    rule: &mut Option<usvg::FillRule>,
) -> Result<(), String> {
    let transform = prefix.pre_concat(group.transform());
    for child in group.children() {
        match child {
            usvg::Node::Path(path) if path.is_visible() => {
                let Some(fill) = path.fill() else {
                    continue;
                };
                if rule.is_some_and(|old| old != fill.rule()) {
                    return Err("Unsupported mixed vector clipping rules".into());
                }
                *rule = Some(fill.rule());
                let data = path
                    .data()
                    .clone()
                    .transform(transform)
                    .ok_or("Invalid vector clipping transform")?;
                geometry(&data, out);
            }
            usvg::Node::Group(group) => {
                if group.clip_path().is_some() {
                    return Err("Nested vector clip groups are unsupported".into());
                }
                clip(group, transform, out, rule)?;
            }
            usvg::Node::Text(text) => clip(text.flattened(), transform, out, rule)?,
            _ => {}
        }
    }
    Ok(())
}
fn group(
    parent: &usvg::Group,
    prefix: tiny_skia::Transform,
    out: &mut String,
) -> Result<(), String> {
    if parent.opacity().get() == 0.0 {
        return Ok(());
    }
    if parent.opacity().get() != 1.0
        || parent.mask().is_some()
        || !parent.filters().is_empty()
        || parent.blend_mode() != usvg::BlendMode::Normal
    {
        return Err("Unsupported technical vector group effect".into());
    }
    let transform = prefix.pre_concat(parent.transform());
    out.push_str("q\n");
    if let Some(clipping) = parent.clip_path() {
        let mut current = Some(clipping);
        while let Some(clipping) = current {
            let mut rule = None;
            clip(
                clipping.root(),
                transform.pre_concat(clipping.transform()),
                out,
                &mut rule,
            )?;
            out.push_str(if rule == Some(usvg::FillRule::EvenOdd) {
                "W* n\n"
            } else {
                "W n\n"
            });
            current = clipping.clip_path();
        }
    }
    for child in parent.children() {
        match child {
            usvg::Node::Group(child) => group(child, transform, out)?,
            usvg::Node::Text(text) => group(text.flattened(), transform, out)?,
            usvg::Node::Path(path) if path.is_visible() => {
                let t = transform;
                writeln!(
                    out,
                    "q {:.9} {:.9} {:.9} {:.9} {:.9} {:.9} cm",
                    t.sx, t.ky, t.kx, t.sy, t.tx, t.ty
                )
                .unwrap();
                if path.paint_order() == usvg::PaintOrder::StrokeAndFill {
                    stroke(path, out)?;
                    fill(path, out)?;
                } else {
                    fill(path, out)?;
                    stroke(path, out)?;
                }
                out.push_str("Q\n");
            }
            usvg::Node::Image(_) => {
                return Err("Technical vector unexpectedly contains a raster".into())
            }
            _ => {}
        }
    }
    out.push_str("Q\n");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{symbols, Format};
    #[test]
    fn every_symbol_and_code_is_resource_free_vector_with_physical_geometry() {
        let mut elements = symbols()
            .iter()
            .map(|symbol| TechnicalElement {
                symbol_id: Some(symbol.id.into()),
                code_type: Format::Qr,
                value: String::new(),
                width_mm: 20.0,
                caption: false,
            })
            .collect::<Vec<_>>();
        elements.extend(
            [
                Format::Qr,
                Format::DataMatrix,
                Format::Code128,
                Format::Aztec,
            ]
            .map(|format| TechnicalElement {
                symbol_id: None,
                code_type: format,
                value: "PCB-0007".into(),
                width_mm: 55.0,
                caption: true,
            }),
        );
        for element in elements {
            let vector = element
                .vector()
                .unwrap_or_else(|e| panic!("{:?}: {e}", element.symbol_id));
            assert!((vector.width_points - element.width_mm * 72.0 / 25.4).abs() < 1e-9);
            assert!(vector.height_points > 0.0);
            let commands = lopdf::content::Content::decode(&vector.content).unwrap();
            assert!(commands.operations.iter().any(|op| op.operator == "f"));
            let mut depth = 0;
            for op in commands.operations {
                assert!(
                    matches!(
                        op.operator.as_str(),
                        "q" | "Q"
                            | "cm"
                            | "rg"
                            | "m"
                            | "l"
                            | "c"
                            | "h"
                            | "f"
                            | "f*"
                            | "W"
                            | "W*"
                            | "n"
                    ),
                    "{}",
                    op.operator
                );
                if op.operator == "q" {
                    depth += 1;
                } else if op.operator == "Q" {
                    depth -= 1;
                    assert!(depth >= 0);
                }
            }
            assert_eq!(depth, 0);
            let pdf = lopdf::Document::load_mem(&element.pdf().unwrap()).unwrap();
            let page = *pdf.get_pages().values().next().unwrap();
            let page = pdf.get_dictionary(page).unwrap();
            let annotation = page.get(b"Annots").unwrap().as_array().unwrap()[0]
                .as_reference()
                .unwrap();
            let annotation = pdf.get_dictionary(annotation).unwrap();
            let recipe =
                lopdf::decode_text_string(annotation.get(b"Suite42TechnicalElement").unwrap())
                    .unwrap();
            assert_eq!(TechnicalElement::from_description(&recipe), Some(element));
        }
    }
}
