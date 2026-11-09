//! Remove a color-matched background connected to the image border.
use img42_core::{ImageDocument, ImageSnapshot};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};

pub(super) const COMMAND: u16 = 52;

#[derive(Default)]
pub(super) struct Removal {
    worker: Option<Worker>,
    message: String,
}

struct Worker {
    source: ImageSnapshot,
    cancel: Arc<AtomicBool>,
    result: mpsc::Receiver<Result<Vec<bool>, String>>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Removal {
    pub(super) fn running(&self) -> bool {
        self.worker.is_some()
    }
    pub(super) fn status(&self) -> &str {
        &self.message
    }

    pub(super) fn start(&mut self, image: &ImageDocument, tolerance: u8) -> Result<(), String> {
        let source = image.capture_image();
        let snapshot = source.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let token = cancel.clone();
        let (send, result) = mpsc::channel();
        std::thread::Builder::new()
            .name("off42-image-background".into())
            .spawn(move || {
                let outcome = mask(&snapshot, tolerance, &token);
                let _ = send.send(outcome);
            })
            .map_err(|error| error.to_string())?;
        self.worker = Some(Worker {
            source,
            cancel,
            result,
        });
        self.message = "Removing uniform background…".into();
        Ok(())
    }

    pub(super) fn poll(&mut self, image: Option<&mut ImageDocument>) -> (bool, bool) {
        let Some(worker) = self.worker.as_ref() else {
            return (false, false);
        };
        let Some(image) = image.filter(|image| image.matches_image(&worker.source)) else {
            self.worker = None;
            self.message.clear();
            return (true, false);
        };
        let result = match worker.result.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return (false, false),
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("Background removal worker stopped".into())
            }
        };
        self.worker = None;
        let result =
            result.and_then(|mask| image.erase_mask(&mask).map_err(|error| error.to_string()));
        let applied = result.is_ok();
        self.message = match result {
            Ok(()) => "Uniform background removed".into(),
            Err(error) => error,
        };
        (true, applied)
    }
}

fn mask(
    source: &ImageSnapshot,
    tolerance: u8,
    cancelled: &AtomicBool,
) -> Result<Vec<bool>, String> {
    let (width, height) = (source.width() as usize, source.height() as usize);
    let count = width * height;
    if width < 3 || height < 3 {
        return Err("The image is too small to identify a border background".into());
    }
    let pixel = |index: usize| &source.pixels()[index * 4..index * 4 + 4];
    let similar = |a: &[u8], b: &[u8]| {
        a[..3]
            .iter()
            .zip(&b[..3])
            .all(|(a, b)| a.abs_diff(*b) <= tolerance)
    };
    let corners = [0, width - 1, count - width, count - 1];
    let perimeter = (0..width)
        .flat_map(|x| [x, count - width + x])
        .chain((1..height - 1).flat_map(|y| [y * width, (y + 1) * width - 1]));
    let samples = perimeter
        .clone()
        .step_by(((2 * width + 2 * height) / 768).max(1))
        .collect::<Vec<_>>();
    let background = corners
        .iter()
        .map(|&index| pixel(index))
        .filter(|p| p[3] >= 128)
        .filter(|candidate| {
            corners
                .iter()
                .filter(|&&index| {
                    let sample = pixel(index);
                    sample[3] >= 128 && similar(candidate, sample)
                })
                .count()
                >= 3
        })
        .max_by_key(|candidate| {
            samples
                .iter()
                .filter(|&&i| similar(candidate, pixel(i)))
                .count()
        })
        .filter(|candidate| {
            samples
                .iter()
                .filter(|&&i| pixel(i)[3] == 0 || similar(candidate, pixel(i)))
                .count()
                * 3
                >= samples.len() * 2
        })
        .ok_or("No uniform background found along the image border")?;

    let mut removed = vec![false; count];
    let mut queue = VecDeque::new();
    let enqueue = |index: usize, queue: &mut VecDeque<u32>, removed: &mut [bool]| {
        if !removed[index] && (pixel(index)[3] == 0 || similar(background, pixel(index))) {
            removed[index] = true;
            queue.push_back(index as u32);
        }
    };
    for index in perimeter {
        enqueue(index, &mut queue, &mut removed);
    }
    let mut visited = 0;
    let mut erased_opaque = false;
    while let Some(index) = queue.pop_front().map(|index| index as usize) {
        visited += 1;
        if visited % 4096 == 0 && cancelled.load(Ordering::Relaxed) {
            return Err("Background removal cancelled".into());
        }
        erased_opaque |= pixel(index)[3] != 0;
        if index % width > 0 {
            enqueue(index - 1, &mut queue, &mut removed);
        }
        if index % width < width - 1 {
            enqueue(index + 1, &mut queue, &mut removed);
        }
        if index >= width {
            enqueue(index - width, &mut queue, &mut removed);
        }
        if index < count - width {
            enqueue(index + width, &mut queue, &mut removed);
        }
    }
    if !erased_opaque
        || !removed
            .iter()
            .enumerate()
            .any(|(index, removed)| !removed && pixel(index)[3] != 0)
    {
        return Err("No separate foreground and uniform background found".into());
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> ImageDocument {
        let pixels = (0..1024)
            .flat_map(|index| {
                let (x, y) = (index % 32, index / 32);
                if (8..24).contains(&x) && (8..24).contains(&y) && (x, y) != (15, 15) {
                    [40, 80, 100, 255]
                } else {
                    [240 + (x % 8) as u8, 245, 246, 255]
                }
            })
            .collect();
        ImageDocument::from_rgba(32, 32, pixels, Some((300, 240))).unwrap()
    }

    #[test]
    fn preserves_disconnected_same_color_foreground_and_pixel_history() {
        let mut image = fixture();
        let original = image.pixels().to_vec();
        let selected = mask(&image.capture_image(), 16, &AtomicBool::new(false)).unwrap();
        let inside = 15 * 32 + 15;
        assert!(selected[0]);
        assert!(!selected[inside]);
        assert_eq!(
            selected.iter().filter(|&&selected| selected).count(),
            1024 - 16 * 16
        );
        image.erase_mask(&selected).unwrap();
        let removed = image.pixels().to_vec();
        assert_eq!(&removed[..4], &[0; 4]);
        assert_eq!(
            &removed[inside * 4..inside * 4 + 4],
            &original[inside * 4..inside * 4 + 4]
        );
        assert!(image.undo());
        assert_eq!(image.pixels(), original);
        assert!(image.redo());
        assert_eq!(image.pixels(), removed);
    }

    #[test]
    fn rejects_an_all_background_image_and_discards_results_for_replaced_pixels() {
        let plain = ImageDocument::from_rgba(32, 32, vec![255; 4096], None).unwrap();
        assert!(mask(&plain.capture_image(), 16, &AtomicBool::new(false)).is_err());
        let image = fixture();
        let mut removal = Removal::default();
        removal.start(&image, 16).unwrap();
        let mut replacement = fixture();
        assert_eq!(removal.poll(Some(&mut replacement)), (true, false));
        assert!(!replacement.is_dirty());
    }
}
