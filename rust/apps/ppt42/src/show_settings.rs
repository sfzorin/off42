use std::{io::Read, path::PathBuf};
use suite42_core::session::{publish_bundle, FileIdentity, SaveBundle};

#[derive(Clone, Copy, Default)]
pub(crate) struct ShowSettings {
    pub looped: bool,
    pub show_hidden: bool,
}

fn location() -> Option<PathBuf> {
    let root = std::env::var_os("LOCALAPPDATA")?;
    Some(
        PathBuf::from(root)
            .join("off42")
            .join("PPT42 show settings"),
    )
}

impl ShowSettings {
    pub fn load() -> Self {
        location()
            .and_then(|path| {
                let mut data = Vec::new();
                std::fs::File::open(path)
                    .ok()?
                    .take(9)
                    .read_to_end(&mut data)
                    .ok()?;
                match data.as_slice() {
                    [b'1', b'\n', looped @ (b'0' | b'1'), hidden @ (b'0' | b'1'), b'\n'] => {
                        Some(Self {
                            looped: *looped == b'1',
                            show_hidden: *hidden == b'1',
                        })
                    }
                    _ => None,
                }
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
            main: vec![
                b'1',
                b'\n',
                b'0' + u8::from(self.looped),
                b'0' + u8::from(self.show_hidden),
                b'\n',
            ],
            sidecars: Vec::new(),
        };
        publish_bundle(&path, &bundle, identity.as_ref())
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}
