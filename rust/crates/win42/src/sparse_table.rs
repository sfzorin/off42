//! Immutable grid text without allocating empty cells. Geometry and document editing belong to their owners.
use std::{collections::BTreeMap, sync::Arc};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SparseTable {
    rows: usize,
    columns: usize,
    cells: Arc<BTreeMap<(usize, usize), String>>,
}
impl SparseTable {
    pub fn new(
        rows: usize,
        columns: usize,
        cells: BTreeMap<(usize, usize), String>,
    ) -> Result<Self, String> {
        if rows > 1_048_576
            || columns > 16_384
            || cells.len() > 1_000_000
            || cells.keys().any(|&(r, c)| r >= rows || c >= columns)
            || cells.values().map(String::len).sum::<usize>() > 32 * 1024 * 1024
        {
            return Err("Grid data exceeds supported dimensions or text size".into());
        }
        Ok(Self {
            rows,
            columns,
            cells: Arc::new(cells),
        })
    }
    pub fn len(&self) -> usize {
        self.rows
    }
    pub fn is_empty(&self) -> bool {
        self.rows == 0 || self.columns == 0
    }
    pub fn columns(&self) -> usize {
        self.columns
    }
    pub fn cell(&self, row: usize, column: usize) -> &str {
        self.cells.get(&(row, column)).map_or("", String::as_str)
    }
    #[cfg(any(windows, test))]
    pub(crate) fn from_dense(rows: Vec<Vec<String>>) -> Option<Self> {
        let columns = rows.first().map_or(0, Vec::len);
        let count = rows.len();
        if rows.iter().any(|r| r.len() != columns) {
            return None;
        }
        Self::new(
            count,
            columns,
            rows.into_iter()
                .enumerate()
                .flat_map(|(r, row)| {
                    row.into_iter()
                        .enumerate()
                        .filter_map(move |(c, text)| (!text.is_empty()).then_some(((r, c), text)))
                })
                .collect(),
        )
        .ok()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_size_empty_grid_and_far_cells_share_storage_and_validate_coordinates() {
        let data = SparseTable::new(
            1_048_576,
            16_384,
            BTreeMap::from([
                ((0, 0), "FIRST".into()),
                ((1_048_575, 16_383), "LAST".into()),
            ]),
        )
        .unwrap();
        assert_eq!(data.len(), 1_048_576);
        assert_eq!(data.cell(1_048_575, 16_383), "LAST");
        assert_eq!(data.cell(500_000, 12_000), "");
        let clone = data.clone();
        assert!(Arc::ptr_eq(&data.cells, &clone.cells));
        assert!(SparseTable::new(1, 1, BTreeMap::from([((1, 0), "bad".into())])).is_err());
        let data = SparseTable::from_dense(vec![
            vec!["a".into(), String::new()],
            vec![String::new(), "b".into()],
        ])
        .unwrap();
        assert_eq!(data.cell(1, 1), "b");
        assert_eq!(data.cells.len(), 2);
        assert!(SparseTable::from_dense(vec![vec![], vec!["bad".into()]]).is_none());
    }
}
