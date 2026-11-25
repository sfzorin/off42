use super::*;

/// A committed document snapshot reused by bounded page export workers.
/// Every extraction owns and prunes a separate clone; the source is untouched.
pub struct PageExtraction {
    document: Document,
    root: ObjectId,
    pages: Vec<ObjectId>,
    labels: Option<Vec<lopdf::Dictionary>>,
}
impl PageExtraction {
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
    pub fn extract(&self, indices: &[usize]) -> Result<Vec<u8>, String> {
        if indices.is_empty() || indices.len() > MAX_PAGES {
            return Err("Choose at least one page within the page limit".into());
        }
        let mut selected = HashSet::new();
        let mut kept = Vec::with_capacity(indices.len());
        for &index in indices {
            let page = *self.pages.get(index).ok_or("Page index out of range")?;
            if !selected.insert(page) {
                return Err("A page may appear only once in an extraction".into());
            }
            kept.push(page);
        }
        let mut doc = self.document.clone();
        flatten_pages(&mut doc, self.root, &kept)?;
        if let Some(labels) = &self.labels {
            let mut nums = Vec::with_capacity(indices.len() * 2);
            for (position, &index) in indices.iter().enumerate() {
                nums.extend([
                    Object::Integer(position as i64),
                    Object::Dictionary(labels[index].clone()),
                ]);
            }
            let tree = doc.add_object(lopdf::dictionary! {"Nums" => nums});
            doc.catalog_mut()
                .map_err(|e| e.to_string())?
                .set("PageLabels", tree);
        }
        let removed: HashSet<_> = self
            .pages
            .iter()
            .copied()
            .filter(|id| !selected.contains(id))
            .collect();
        prune_page_references(&mut doc, &kept, &removed)?;
        doc.prune_objects();
        if removed.iter().any(|id| doc.objects.contains_key(id)) {
            return Err("Other pages remain referenced; cannot safely extract".into());
        }
        let mut out = Vec::new();
        doc.save_to(&mut out).map_err(|e| e.to_string())?;
        if out.len() as u64 > MAX_FILE {
            return Err("Extracted PDF exceeds the file limit".into());
        }
        Ok(out)
    }
}
impl PdfDocument {
    pub fn prepare_page_extraction(&self) -> Result<PageExtraction, String> {
        self.ensure_position_safe()?;
        let (bytes, _) = self.save_snapshot()?;
        let document = Document::load_mem_with_options(
            &bytes,
            LoadOptions {
                max_decompressed_size: Some(32 * 1024 * 1024),
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())?;
        let pages = document.get_pages().values().copied().collect();
        let root = document
            .catalog()
            .map_err(|e| e.to_string())?
            .get(b"Pages")
            .map_err(|e| e.to_string())?
            .as_reference()
            .map_err(|e| e.to_string())?;
        let labels = self.page_label_rules()?.map(|rules| {
            self.history
                .current()
                .pages
                .iter()
                .enumerate()
                .map(|(index, page)| {
                    rules
                        .get(page)
                        .cloned()
                        .unwrap_or_else(|| lopdf::dictionary! {"S"=>"D","St"=>(index+1) as i64})
                })
                .collect()
        });
        Ok(PageExtraction {
            document,
            root,
            pages,
            labels,
        })
    }
    pub fn extract_pages(&self, indices: &[usize], path: &Path) -> Result<(), String> {
        let out = self.prepare_page_extraction()?.extract(indices)?;
        write_new(path, &out).map_err(|e| e.to_string())
    }
}
