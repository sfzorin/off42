//! Shared sparse lengths and exact 64-bit coordinates for a worksheet axis.
use std::{collections::BTreeMap, ops::Index, sync::Arc};
#[derive(Clone, Debug)]
pub struct GridAxis(Arc<Axis>);
#[derive(Debug)]
struct Axis {
    count: usize,
    default: i32,
    lengths: BTreeMap<usize, i32>,
    deltas: Vec<(usize, i64)>,
}
impl GridAxis {
    pub fn new(count: usize, default: u16, lengths: BTreeMap<usize, u16>) -> Option<Self> {
        if count > 1_048_576 || lengths.keys().any(|i| *i >= count) {
            return None;
        }
        let lengths: BTreeMap<_, _> = lengths
            .into_iter()
            .filter(|(_, v)| *v != default)
            .map(|(i, v)| (i, i32::from(v)))
            .collect();
        let mut sum = 0;
        let deltas = lengths
            .iter()
            .map(|(&i, &v)| {
                sum += i64::from(v) - i64::from(default);
                (i, sum)
            })
            .collect();
        Some(Self(Arc::new(Axis {
            count,
            default: i32::from(default),
            lengths,
            deltas,
        })))
    }
    pub fn len(&self) -> usize {
        self.0.count
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn offset(&self, index: usize) -> i64 {
        assert!(index <= self.len());
        let n = self.0.deltas.partition_point(|(i, _)| *i < index);
        index as i64 * i64::from(self.0.default)
            + n.checked_sub(1).map_or(0, |i| self.0.deltas[i].1)
    }
    pub fn total(&self) -> i64 {
        self.offset(self.len())
    }
    pub fn index_at(&self, pixel: i64) -> Option<usize> {
        if pixel < 0 || pixel >= self.total() {
            return None;
        }
        let (mut lo, mut hi) = (0, self.len());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.offset(mid) <= pixel {
                lo = mid + 1
            } else {
                hi = mid
            }
        }
        lo.checked_sub(1)
    }
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &i32> + ExactSizeIterator {
        (0..self.len()).map(|i| &self[i])
    }
    /// Only positive-length cells intersecting this logical interval are visited, including long hidden runs.
    pub(crate) fn intersecting(&self, start: i64, end: i64) -> impl Iterator<Item = usize> + '_ {
        let mut pixel = start.max(0);
        std::iter::from_fn(move || {
            if pixel >= end {
                return None;
            }
            let i = self.index_at(pixel)?;
            pixel = self.offset(i + 1);
            Some(i)
        })
    }
}
impl Index<usize> for GridAxis {
    type Output = i32;
    fn index(&self, index: usize) -> &i32 {
        assert!(index < self.len());
        self.0.lengths.get(&index).unwrap_or(&self.0.default)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_height_axes_retain_far_coordinates_hidden_runs_and_shared_storage() {
        let axis =
            GridAxis::new(1_048_576, 65535, BTreeMap::from([(0, 0), (999_999, 23)])).unwrap();
        let last = axis.offset(1_048_575);
        assert!(last > i64::from(i32::MAX));
        assert_eq!(axis.index_at(last), Some(1_048_575));
        assert_eq!(axis.index_at(axis.total() - 1), Some(1_048_575));
        assert_eq!(axis.index_at(axis.total()), None);
        assert_eq!(axis.index_at(0), Some(1));
        assert_eq!(
            axis.intersecting(axis.offset(999_999), axis.offset(1_000_001))
                .collect::<Vec<_>>(),
            [999_999, 1_000_000]
        );
        assert!(Arc::ptr_eq(&axis.0, &axis.clone().0));
        let hidden = GridAxis::new(
            1_048_576,
            0,
            BTreeMap::from([(900_000, 20), (1_048_575, 40)]),
        )
        .unwrap();
        assert_eq!(
            hidden.intersecting(0, 60).collect::<Vec<_>>(),
            [900_000, 1_048_575]
        );
        assert_eq!(hidden.index_at(20), Some(1_048_575));
        assert_eq!(hidden.total(), 60);
    }
}
