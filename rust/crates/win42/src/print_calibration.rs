use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrintCorrection {
    pub offset_x: f64,
    pub offset_y: f64,
    pub scale_x: f64,
    pub scale_y: f64,
}

impl Default for PrintCorrection {
    fn default() -> Self {
        Self {
            offset_x: 0.0,
            offset_y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
        }
    }
}

impl PrintCorrection {
    pub fn validate(self) -> Result<Self, String> {
        if !self.offset_x.is_finite()
            || !self.offset_y.is_finite()
            || self.offset_x.abs() > 20.0
            || self.offset_y.abs() > 20.0
            || !(0.95..=1.05).contains(&self.scale_x)
            || !(0.95..=1.05).contains(&self.scale_y)
        {
            return Err(
                "Use offsets within ±20 mm and scale corrections between 95% and 105%.".into(),
            );
        }
        Ok(self)
    }

    pub fn measured(
        horizontal: f64,
        vertical: f64,
        offset_x: f64,
        offset_y: f64,
    ) -> Result<Self, String> {
        Self {
            offset_x,
            offset_y,
            scale_x: 100.0 / horizontal,
            scale_y: 100.0 / vertical,
        }
        .validate()
    }

    pub fn device_origin(
        self,
        origin: (i32, i32),
        physical_offset: (i32, i32),
        dpi: (i32, i32),
    ) -> (i32, i32) {
        (
            ((f64::from(origin.0) + f64::from(physical_offset.0)) * self.scale_x
                - f64::from(physical_offset.0)
                + self.offset_x * f64::from(dpi.0) / 25.4)
                .round() as i32,
            ((f64::from(origin.1) + f64::from(physical_offset.1)) * self.scale_y
                - f64::from(physical_offset.1)
                + self.offset_y * f64::from(dpi.1) / 25.4)
                .round() as i32,
        )
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    version: u8,
    key: String,
    correction: PrintCorrection,
}

fn profile_path(root: &Path, key: &str) -> PathBuf {
    root.join(format!("{:x}.json", Sha256::digest(key.as_bytes())))
}

pub fn load(root: &Path, key: &str) -> Result<Option<PrintCorrection>, String> {
    let file = match File::open(profile_path(root, key)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not read printer calibration: {error}")),
    };
    let mut bytes = Vec::new();
    file.take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 65_536 {
        return Err("Printer calibration file is too large.".into());
    }
    let profile: Profile = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Invalid printer calibration: {error}"))?;
    if profile.version != 1 || profile.key != key {
        return Err("Printer calibration identity does not match.".into());
    }
    Ok(Some(profile.correction.validate()?))
}

pub fn save(root: &Path, key: &str, correction: PrintCorrection) -> Result<(), String> {
    correction.validate()?;
    std::fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let path = profile_path(root, key);
    let expected = match suite42_core::session::FileIdentity::read(&path) {
        Ok(identity) => Some(identity),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.to_string()),
    };
    let profile = Profile {
        version: 1,
        key: key.into(),
        correction,
    };
    let bytes = serde_json::to_vec(&profile).map_err(|error| error.to_string())?;
    suite42_core::session::publish(&path, &bytes, expected.as_ref())
        .map_err(|error| format!("Could not save printer calibration: {error}"))?;
    Ok(())
}

#[cfg(windows)]
pub fn directory() -> Result<PathBuf, String> {
    let root = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("Windows local app data folder is unavailable")?;
    Ok(root.join("off42").join("Printer Calibration"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_are_separate_by_printer_and_paper_and_validate_measured_scale() {
        let root = tempfile::tempdir().unwrap();
        let correction = PrintCorrection::measured(99.0, 101.0, 1.5, -2.0).unwrap();
        save(root.path(), "Printer/A4", correction).unwrap();
        assert_eq!(load(root.path(), "Printer/A4").unwrap(), Some(correction));
        assert_eq!(load(root.path(), "Printer/Letter").unwrap(), None);
        assert!(PrintCorrection::measured(0.0, 100.0, 0.0, 0.0).is_err());
        assert!(PrintCorrection::measured(90.0, 100.0, 0.0, 0.0).is_err());
    }

    #[test]
    fn correction_uses_the_physical_top_left_and_separate_axis_scales() {
        let correction = PrintCorrection {
            offset_x: 2.54,
            offset_y: -2.54,
            scale_x: 1.02,
            scale_y: 0.98,
        };
        assert_eq!(
            correction.device_origin((300, 600), (20, 40), (300, 600)),
            (336, 527)
        );
        assert_eq!(
            PrintCorrection::default().device_origin((300, 600), (20, 40), (300, 600)),
            (300, 600)
        );
    }
}
