//! Explicitly kept comparison marks are ordinary highlight annotations in one history transaction.
use super::*;
pub struct ComparisonMark {
    pub page: usize,
    pub replaced: bool,
    pub boxes: Vec<[f64; 4]>,
}
impl PdfDocument {
    /// View navigation and saving do not invalidate a read-only comparison snapshot.
    pub fn same_source(&self, snapshot: &SaveState) -> bool {
        self.source_identity == snapshot.1
    }
    pub fn matches_snapshot(&self, snapshot: &SaveState) -> bool {
        self.same_source(snapshot) && self.history.current_id() == snapshot.2
    }
    pub fn has_comparison_marks(&self) -> bool {
        self.history
            .current()
            .annotations
            .iter()
            .any(|a| a.comparison.is_some())
    }
    pub fn clear_comparison_marks(&mut self) {
        let mut state = self.history.current().clone();
        state.annotations.retain(|a| a.comparison.is_none());
        self.history.apply(state);
    }
    pub fn replace_comparison_marks(
        &mut self,
        tag: u64,
        marks: &[ComparisonMark],
    ) -> Result<(), String> {
        if tag == 0
            || marks.len() > 10_000
            || marks
                .iter()
                .map(|m| m.boxes.len())
                .try_fold(0usize, usize::checked_add)
                .is_none_or(|n| n > 100_000)
        {
            return Err("Comparison highlights exceed their limit".into());
        }
        let mut state = self.history.current().clone();
        state
            .annotations
            .retain(|a| !a.comparison.is_some_and(|(id, _)| id == tag));
        for mark in marks {
            if mark.boxes.is_empty() {
                continue;
            }
            let page = *state
                .pages
                .get(mark.page)
                .ok_or("Comparison page is unavailable")?;
            let page_box = self.page_box(mark.page)?;
            let mut quads = Vec::new();
            let mut rect = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            for b in &mark.boxes {
                if b.iter().any(|v| !v.is_finite()) || b[2] <= b[0] || b[3] <= b[1] {
                    return Err("Invalid comparison highlight geometry".into());
                }
                let b = [
                    b[0].max(page_box[0]),
                    b[1].max(page_box[1]),
                    b[2].min(page_box[2]),
                    b[3].min(page_box[3]),
                ];
                if b[2] <= b[0] || b[3] <= b[1] {
                    continue;
                }
                rect[0] = rect[0].min(b[0]);
                rect[1] = rect[1].min(b[1]);
                rect[2] = rect[2].max(b[2]);
                rect[3] = rect[3].max(b[3]);
                quads.push(b);
            }
            if quads.is_empty() {
                continue;
            }
            state.annotations.push(Annotation {
                page,
                kind: AnnotationKind::Highlight,
                rect,
                contents: String::new(),
                points: Vec::new(),
                quads,
                image: None,
                vector: None,
                comparison: Some((
                    tag,
                    if mark.replaced {
                        [1.0, 0.5, 0.0]
                    } else {
                        [1.0, 0.0, 0.0]
                    },
                )),
            });
        }
        self.history.apply(state);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source(text: &str) -> PdfDocument {
        use lopdf::dictionary;
        let mut d = Document::with_version("1.7");
        let root = d.new_object_id();
        let font =
            d.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
        let bytes = format!("BT /F1 12 Tf 10 100 Td ({text}) Tj ET").into_bytes();
        let content = d.add_object(lopdf::Stream::new(dictionary! {}, bytes));
        let page=d.add_object(dictionary!{"Type"=>"Page","Parent"=>root,"MediaBox"=>vec![0.into(),0.into(),200.into(),200.into()],"Resources"=>dictionary!{"Font"=>dictionary!{"F1"=>font}},"Contents"=>content});
        d.objects.insert(
            root,
            Object::Dictionary(dictionary! {"Type"=>"Pages","Kids"=>vec![page.into()],"Count"=>1}),
        );
        let catalog = d.add_object(dictionary! {"Type"=>"Catalog","Pages"=>root});
        d.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        d.save_to(&mut bytes).unwrap();
        PdfDocument::from_edited_snapshot(&bytes, 0).unwrap()
    }
    #[test]
    fn comparison_marks_are_atomic_colored_and_separate_from_other_annotations() {
        let mut d = source("ORIGINAL");
        d.add_annotation(0, AnnotationKind::Rectangle, [40.0, 40.0, 80.0, 80.0], "")
            .unwrap();
        let baseline = d.save_snapshot().unwrap().0;
        let marks = [
            ComparisonMark {
                page: 0,
                replaced: false,
                boxes: vec![[10.0, 90.0, 20.0, 105.0]],
            },
            ComparisonMark {
                page: 0,
                replaced: true,
                boxes: vec![[30.0, 90.0, 50.0, 105.0]],
            },
        ];
        d.replace_comparison_marks(7, &marks).unwrap();
        let (bytes, _) = d.save_snapshot().unwrap();
        let pdf = Document::load_mem(&bytes).unwrap();
        let page = *pdf.get_pages().values().next().unwrap();
        let ann = pdf
            .get_dictionary(page)
            .unwrap()
            .get(b"Annots")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(ann.len(), 3);
        let colors = ann
            .iter()
            .skip(1)
            .map(|a| {
                pdf.get_dictionary(a.as_reference().unwrap())
                    .unwrap()
                    .get(b"C")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| match v {
                        Object::Real(v) => *v as f64,
                        Object::Integer(v) => *v as f64,
                        _ => panic!(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(colors, vec![vec![1.0, 0.0, 0.0], vec![1.0, 0.5, 0.0]]);
        assert_eq!(pdf.extract_text(&[1]).unwrap().trim(), "ORIGINAL");
        d.undo();
        assert_eq!(d.save_snapshot().unwrap().0, baseline);
        d.redo();
        assert_eq!(d.save_snapshot().unwrap().0, bytes);
        let invalid = [ComparisonMark {
            page: 0,
            replaced: false,
            boxes: vec![[f64::NAN, 0.0, 2.0, 2.0]],
        }];
        assert!(d.replace_comparison_marks(7, &invalid).is_err());
        assert_eq!(d.save_snapshot().unwrap().0, bytes);
        d.replace_comparison_marks(8, &marks[..1]).unwrap();
        d.replace_comparison_marks(7, &[]).unwrap();
        assert_eq!(d.history.current().annotations.len(), 2);
        assert!(d
            .history
            .current()
            .annotations
            .iter()
            .any(|a| a.comparison.is_some_and(|(id, _)| id == 8)));
    }
    #[test]
    fn snapshot_identity_rejects_other_sources_and_tracks_content_history() {
        let mut a = source("ONE");
        let b = source("TWO");
        let (_, saved) = a.save_snapshot().unwrap();
        assert!(a.matches_snapshot(&saved));
        assert!(!b.matches_snapshot(&saved));
        assert!(!b.same_source(&saved));
        a.select(0).unwrap();
        assert!(a.matches_snapshot(&saved));
        a.mark_saved_snapshot(a.save_snapshot().unwrap().1);
        assert!(a.matches_snapshot(&saved));
        a.rotate(0, 90).unwrap();
        assert!(!a.matches_snapshot(&saved));
        a.undo();
        assert!(a.matches_snapshot(&saved));
        a.redo();
        assert!(!a.matches_snapshot(&saved));
    }
}
