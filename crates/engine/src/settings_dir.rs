//! The per-user Cad settings directory (logs, autosave, plotter configurations). The host
//! chooses it ([`set`], possibly to none); without a choice it is the platform's ([`platform`]).

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Mutex;

/// `None`: no choice yet; `Some(None)`: the host chose to have no settings directory.
static CHOSEN: Mutex<Option<Option<PathBuf>>> = Mutex::new(None);

/// The platform's settings directory, with `var` looking up the OS variables: `$XDG_CONFIG_HOME/iridium_cad`
/// or `~/.config/iridium_cad` on Linux and the BSDs, `~/Library/Application Support/Cad` on
/// macOS, `%APPDATA%\Cad` on Windows. An empty variable counts as unset, so a blank value
/// never means the working directory.
pub fn platform(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let set = |k: &str| var(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(target_os = "macos") {
        set("HOME").map(|h| h.join("Library/Application Support/Cad"))
    } else if cfg!(windows) {
        set("APPDATA").map(|a| a.join("Cad"))
    } else {
        // The XDG spec ignores a relative XDG_CONFIG_HOME.
        set("XDG_CONFIG_HOME").filter(|p| p.is_absolute()).or_else(|| set("HOME").map(|h| h.join(".config"))).map(|c| c.join("iridium_cad"))
    }
}

/// Use `dir` as the settings directory; `None` means there is none (plotter configurations are
/// neither read nor written).
pub fn set(dir: Option<PathBuf>) {
    if let Ok(mut chosen) = CHOSEN.lock() {
        *chosen = Some(dir);
    }
}

/// The settings directory: the host's choice, else the platform's.
pub fn current() -> Option<PathBuf> {
    match CHOSEN.lock().ok().and_then(|c| c.clone()) {
        Some(choice) => choice,
        None => platform(|k| std::env::var_os(k)),
    }
}

#[cfg(test)]
mod tests {
    use super::platform;
    use std::ffi::OsString;
    use std::path::PathBuf;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |k| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(v))
    }

    #[test]
    fn empty_values_are_unset() {
        assert_eq!(platform(env(&[("HOME", ""), ("XDG_CONFIG_HOME", ""), ("APPDATA", "")])), None, "not the working directory");
        assert_eq!(platform(env(&[])), None);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_and_bsd_follow_xdg_config_home() {
        assert_eq!(platform(env(&[("XDG_CONFIG_HOME", "/x"), ("HOME", "/home/u")])), Some(PathBuf::from("/x/iridium_cad")));
        assert_eq!(platform(env(&[("HOME", "/home/u")])), Some(PathBuf::from("/home/u/.config/iridium_cad")));
        // The XDG spec says a relative XDG_CONFIG_HOME is ignored.
        assert_eq!(platform(env(&[("XDG_CONFIG_HOME", "rel"), ("HOME", "/home/u")])), Some(PathBuf::from("/home/u/.config/iridium_cad")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uses_application_support() {
        assert_eq!(platform(env(&[("HOME", "/Users/u")])), Some(PathBuf::from("/Users/u/Library/Application Support/Cad")));
    }

    #[cfg(windows)]
    #[test]
    fn windows_uses_appdata() {
        assert_eq!(
            platform(env(&[("APPDATA", "C:\\Users\\u\\AppData\\Roaming")])),
            Some(PathBuf::from("C:\\Users\\u\\AppData\\Roaming").join("Cad"))
        );
    }
}
