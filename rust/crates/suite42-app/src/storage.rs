//! Per-user local storage root shared by preferences, recent documents, recovery and libraries.
use std::ffi::OsString;
use std::path::PathBuf;

/// The per-user `off42` data folder, or `None` where the platform has no such location (WebAssembly)
/// or its environment is incomplete:
/// - Windows: `%LOCALAPPDATA%/off42`, the folder the Windows apps have always used;
/// - macOS: `~/Library/Application Support/off42`;
/// - Linux and other Unix systems: `$XDG_DATA_HOME/off42`, else `~/.local/share/off42`.
pub fn local_data() -> Option<PathBuf> {
    resolve(Platform::current(), |name| std::env::var_os(name))
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Platform {
    Windows,
    MacOs,
    Unix,
    None,
}

impl Platform {
    fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(unix) {
            Self::Unix
        } else {
            Self::None
        }
    }
}

fn resolve(platform: Platform, var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    // Relative values are ignored, as the XDG Base Directory specification requires. Unix paths
    // are checked textually so the rule is the same whichever host runs the tests.
    let absolute = |name: &str| {
        var(name)
            .filter(|value| value.to_string_lossy().starts_with('/'))
            .map(PathBuf::from)
    };
    match platform {
        Platform::Windows => Some(PathBuf::from(var("LOCALAPPDATA")?).join("off42")),
        Platform::MacOs => Some(absolute("HOME")?.join("Library/Application Support/off42")),
        Platform::Unix => absolute("XDG_DATA_HOME")
            .or_else(|| Some(absolute("HOME")?.join(".local/share")))
            .map(|root| root.join("off42")),
        Platform::None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        }
    }
    #[test]
    fn each_platform_uses_its_own_root() {
        let all = [
            ("LOCALAPPDATA", "C:\\Users\\a\\AppData\\Local"),
            ("HOME", "/home/a"),
            ("XDG_DATA_HOME", "/data/a"),
        ];
        assert_eq!(
            resolve(Platform::Windows, env(&all)),
            Some(PathBuf::from("C:\\Users\\a\\AppData\\Local").join("off42"))
        );
        assert_eq!(
            resolve(Platform::MacOs, env(&all)),
            Some(PathBuf::from("/home/a/Library/Application Support/off42"))
        );
        assert_eq!(resolve(Platform::Unix, env(&all)), Some(PathBuf::from("/data/a/off42")));
        assert_eq!(resolve(Platform::None, env(&all)), None);
    }
    #[test]
    fn unix_falls_back_to_home_and_ignores_relative_or_missing_values() {
        assert_eq!(
            resolve(Platform::Unix, env(&[("HOME", "/home/a"), ("XDG_DATA_HOME", "relative")])),
            Some(PathBuf::from("/home/a/.local/share/off42"))
        );
        assert_eq!(resolve(Platform::Unix, env(&[("HOME", "relative")])), None);
        assert_eq!(resolve(Platform::Unix, env(&[])), None);
        assert_eq!(resolve(Platform::MacOs, env(&[])), None);
        assert_eq!(resolve(Platform::Windows, env(&[("HOME", "/home/a")])), None);
    }
}
