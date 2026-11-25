//! Device-independent refinement for footnotes anchored to a body range.
/// The callback measures a body page using the proposed end cap and reserved
/// twips. Each refinement can only shorten the cap, avoiding anchor oscillation.
pub(crate) fn refine_page(
    start: i32,
    initial_end: i32,
    notes: &[(usize, i32, usize)],
    separator: i32,
    per_note_gap: i32,
    maximum_reserve: i32,
    mut measure: impl FnMut(i32, i32) -> Result<i32, String>,
) -> Result<(i32, i32, Vec<usize>), String> {
    let mut end = initial_end;
    loop {
        let included = notes
            .iter()
            .filter(|(anchor, _, _)| *anchor >= start as usize && *anchor < end as usize)
            .collect::<Vec<_>>();
        let reserve = if included.is_empty() {
            0
        } else {
            included
                .iter()
                .try_fold(separator, |sum, (_, height, _)| {
                    sum.checked_add(*height)?.checked_add(per_note_gap)
                })
                .ok_or("Note height overflow")?
        };
        if reserve >= maximum_reserve {
            return Err("Footnotes do not fit on their page".into());
        }
        let next = measure(end, reserve)?;
        if next <= start && initial_end > start {
            return Err("Footnotes leave no room for their reference".into());
        }
        if next > end {
            return Err("Unexpected RichEdit pagination result".into());
        }
        if next == end {
            return Ok((
                end,
                reserve,
                included.into_iter().map(|(_, _, id)| *id).collect(),
            ));
        }
        end = next;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cap_never_reexpands_when_note_moves_to_next_page() {
        let mut caps = Vec::new();
        let (end, reserve, notes) =
            refine_page(0, 100, &[(85, 20, 7)], 10, 0, 200, |cap, space| {
                caps.push(cap);
                Ok(if space > 0 { cap.min(70) } else { cap })
            })
            .unwrap();
        assert_eq!(caps, [100, 70]);
        assert_eq!((end, reserve), (70, 0));
        assert!(notes.is_empty());
    }
}
