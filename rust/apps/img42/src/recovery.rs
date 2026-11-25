use super::*;
use std::io::Read;
const MAGIC: &[u8; 8] = b"IMG42R1\0";
const LIMIT: usize = 256 * 1024 * 1024;
fn number(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(super) fn snapshot(app: &Img) -> Result<Option<win42::recovery::Snapshot>, String> {
    if app.doc.is_none() {
        return Ok(None);
    }
    let count = app.frames.len();
    if count == 0
        || count > 256
        || app.frame_index >= count
        || app.delays.len() != count
        || app.original_16bit.len() != count
    {
        return Err("Invalid image frame state".into());
    }
    let mut bytes = MAGIC.to_vec();
    let (repeat, count_repeat) = match app.animation_loop {
        img42_core::AnimationLoop::Once => (0, 0),
        img42_core::AnimationLoop::Finite(n) => (1, u32::from(n)),
        img42_core::AnimationLoop::Infinite => (2, 0),
    };
    for value in [
        count as u32,
        app.frame_index as u32,
        u32::from(app.conversion_risk),
        repeat,
        count_repeat,
    ] {
        number(&mut bytes, value);
    }
    for (index, frame) in app.frames.iter().enumerate() {
        let frame = if index == app.frame_index {
            app.doc.as_ref()
        } else {
            frame.as_ref()
        }
        .ok_or("Missing image frame")?;
        let image = frame.recovery_bytes().map_err(|error| error.to_string())?;
        if bytes.len().saturating_add(image.len()).saturating_add(16) > LIMIT {
            return Err("Image recovery copy exceeds the size limit".into());
        }
        let (n, d) = app.delays[index].numer_denom_ms();
        for value in [
            n,
            d,
            u32::from(app.original_16bit[index]),
            image.len() as u32,
        ] {
            number(&mut bytes, value);
        }
        bytes.extend_from_slice(&image);
    }
    Ok(Some(win42::recovery::Snapshot::new("img42r", bytes)))
}

struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], String> {
        if length > self.0.len() {
            return Err("Truncated image recovery copy".into());
        }
        let (value, rest) = self.0.split_at(length);
        self.0 = rest;
        Ok(value)
    }
    fn number(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}
fn decode(bytes: &[u8]) -> Result<Img, String> {
    if bytes.len() > LIMIT {
        return Err("Image recovery copy exceeds the size limit".into());
    }
    let mut input = Reader(bytes);
    if input.take(8)? != MAGIC {
        return Err("Unsupported image recovery copy".into());
    }
    let count = input.number()? as usize;
    let index = input.number()? as usize;
    if count == 0 || count > 256 || index >= count {
        return Err("Invalid recovered frame count".into());
    }
    let risk = match input.number()? {
        0 => false,
        1 => true,
        _ => return Err("Invalid image conversion flags".into()),
    };
    let repeat = input.number()?;
    let repeats = input.number()?;
    let animation_loop = match (repeat, repeats) {
        (0, 0) => img42_core::AnimationLoop::Once,
        (1, 1..=65535) => img42_core::AnimationLoop::Finite(repeats as u16),
        (2, 0) => img42_core::AnimationLoop::Infinite,
        _ => return Err("Invalid image animation loop".into()),
    };
    let mut frames = Vec::with_capacity(count);
    let mut delays = Vec::with_capacity(count);
    let mut originals = Vec::with_capacity(count);
    let mut allocation = 0usize;
    for _ in 0..count {
        let n = input.number()?;
        let d = input.number()?;
        if d == 0 {
            return Err("Invalid recovered frame delay".into());
        }
        let original = match input.number()? {
            0 => false,
            1 => true,
            _ => return Err("Invalid recovered sample flags".into()),
        };
        let length = input.number()? as usize;
        let data = input.take(length)?;
        let frame = ImageDocument::from_recovery_bytes(data).map_err(|error| error.to_string())?;
        allocation =
            allocation.saturating_add(frame.pixels().len() * if frame.is_16bit() { 3 } else { 1 });
        if allocation > 192 * 1024 * 1024 {
            return Err("Recovered image frames exceed the memory limit".into());
        }
        frames.push(Some(frame));
        delays.push(image::Delay::from_numer_denom_ms(n, d));
        originals.push(original);
    }
    if !input.0.is_empty() {
        return Err("Unexpected image recovery data".into());
    }
    let doc = frames[index].take();
    Ok(Img {
        frames,
        frame_index: index,
        doc,
        delays,
        original_16bit: originals,
        animation_loop,
        conversion_risk: risk,
        ..Img::default()
    })
}
pub(super) fn open(path: &Path) -> Result<Img, String> {
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    if file.metadata().map_err(|error| error.to_string())?.len() > LIMIT as u64 {
        return Err("Image recovery copy exceeds the size limit".into());
    }
    let mut bytes = Vec::new();
    file.take(LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let mut app = decode(&bytes)?;
    app.source_path = Some(path.to_owned());
    Ok(app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use win42::App;
    fn fixture() -> Img {
        let first =
            ImageDocument::from_rgba(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8], Some((300, 150))).unwrap();
        let second = ImageDocument::from_rgba(1, 2, vec![9, 8, 7, 6, 5, 4, 3, 2], None).unwrap();
        Img {
            frames: vec![Some(first), None],
            doc: Some(second),
            frame_index: 1,
            delays: vec![
                image::Delay::from_numer_denom_ms(100, 3),
                image::Delay::from_numer_denom_ms(250, 7),
            ],
            animation_loop: img42_core::AnimationLoop::Finite(5),
            original_16bit: vec![false, true],
            conversion_risk: true,
            ..Img::default()
        }
    }
    #[test]
    fn all_frames_delays_active_frame_and_conversion_guards_survive_readonly_capture() {
        let mut app = fixture();
        app.doc.as_mut().unwrap().rotate_cw().unwrap();
        let saved = app.save_snapshot("tiff").unwrap().unwrap().0;
        let pending = app.pending.as_ref().unwrap().0;
        let dirty = app.dirty();
        let snapshot = snapshot(&app).unwrap().unwrap();
        let restored = decode(&snapshot.bytes).unwrap();
        assert_eq!(app.pending.as_ref().unwrap().0, pending);
        assert_eq!(app.next_save, saved);
        assert_eq!(app.dirty(), dirty);
        assert!(app.doc.as_ref().unwrap().can_undo());
        assert_eq!(restored.frame_index, 1);
        assert_eq!(
            restored.animation_loop,
            img42_core::AnimationLoop::Finite(5)
        );
        assert_eq!(
            restored
                .delays
                .iter()
                .map(|delay| delay.numer_denom_ms())
                .collect::<Vec<_>>(),
            vec![(100, 3), (250, 7)]
        );
        assert_eq!(restored.original_16bit, vec![false, true]);
        assert!(restored.conversion_risk);
        assert!(restored.loses_precision());
        assert_eq!(
            restored.doc.as_ref().unwrap().pixels(),
            app.doc.as_ref().unwrap().pixels()
        );
        assert_eq!(
            restored.frames[0].as_ref().unwrap().pixels(),
            app.frames[0].as_ref().unwrap().pixels()
        );
        assert_eq!(restored.frames[0].as_ref().unwrap().dpi(), Some((300, 150)));
        for length in [0, 7, 27, snapshot.bytes.len() - 1] {
            assert!(decode(&snapshot.bytes[..length]).is_err());
        }
        let mut trailing = snapshot.bytes.clone();
        trailing.push(0);
        assert!(decode(&trailing).is_err());
    }
    #[test]
    fn declared_counts_flags_and_delays_are_bounded() {
        let bytes = snapshot(&fixture()).unwrap().unwrap().bytes;
        for (offset, value) in [
            (8, 257u32),
            (12, 2),
            (16, 2),
            (20, 3),
            (24, 65536),
            (32, 0),
            (40, u32::MAX),
        ] {
            let mut broken = bytes.clone();
            broken[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(decode(&broken).is_err());
        }
        assert!(snapshot(&Img::default()).unwrap().is_none());
    }
}
