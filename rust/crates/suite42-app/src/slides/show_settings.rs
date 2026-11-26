use std::{io::Read, path::PathBuf};
use suite42_core::session::{publish_bundle, FileIdentity, SaveBundle};

#[derive(Clone, Copy, Default)]
pub struct ShowSettings {
    pub looped: bool,
    pub show_hidden: bool,
}

fn location() -> Option<PathBuf> {
    Some(crate::storage::local_data()?.join("PPT42 show settings"))
}

impl ShowSettings {
    /// Stored form: `1\n` then one digit each for looping and hidden slides, then `\n`.
    pub fn encode(self) -> [u8; 5] {
        [b'1', b'\n', b'0' + u8::from(self.looped), b'0' + u8::from(self.show_hidden), b'\n']
    }
    pub fn decode(data: &[u8]) -> Option<Self> {
        match data {
            [b'1', b'\n', looped @ (b'0' | b'1'), hidden @ (b'0' | b'1'), b'\n'] => Some(Self {
                looped: *looped == b'1',
                show_hidden: *hidden == b'1',
            }),
            _ => None,
        }
    }
    pub fn load() -> Self {
        location()
            .and_then(|path| {
                let mut data = Vec::new();
                std::fs::File::open(path)
                    .ok()?
                    .take(9)
                    .read_to_end(&mut data)
                    .ok()?;
                Self::decode(&data)
            })
            .unwrap_or_default()
    }

    pub fn save(self) -> Result<(), String> {
        let path = location().ok_or("PPT42 preferences directory is unavailable")?;
        std::fs::create_dir_all(path.parent().ok_or("Invalid preferences directory")?)
            .map_err(|error| error.to_string())?;
        let identity = match FileIdentity::read(&path) {
            Ok(identity) => Some(identity),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.to_string()),
        };
        let bundle = SaveBundle {
            main: self.encode().to_vec(),
            sidecars: Vec::new(),
        };
        publish_bundle(&path, &bundle, identity.as_ref())
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stored_show_settings_format_is_pinned() {
        let settings = ShowSettings { looped: true, show_hidden: false };
        assert_eq!(&settings.encode(), b"1\n10\n");
        let decoded = ShowSettings::decode(b"1\n01\n").unwrap();
        assert!(!decoded.looped && decoded.show_hidden);
        assert!(ShowSettings::decode(b"2\n01\n").is_none());
        assert!(ShowSettings::decode(b"1\n01\n\n").is_none());
    }
}
