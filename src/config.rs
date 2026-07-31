// SPDX-License-Identifier: GPL-3.0-or-later
//! The session config file.
//!
//! ```toml
//! # ~/.config/wlrix/session.toml
//! compositor = "wlrix-compositor"
//!
//! [[app]]
//! command = "wlrix-toolchest"
//!
//! [[app]]
//! command = "alacritty"
//! args = ["-e", "fish"]
//!
//! [env]
//! QT_QPA_PLATFORM = "wayland"
//! ```
//!
//! Read from the user's config directory first, then `/etc/wlrix`; the first file found
//! wins outright rather than merging, so what a user sees in their own file is the whole
//! of what they get.
//!
//! Unknown keys are an error. A silently ignored typo in a config file is a bad
//! afternoon, and the cost of being strict is a clear message instead.
//!
//! ## Autostart
//!
//! `session.toml` replaces the app list; `autostart.toml` **adds** to it:
//!
//! ```toml
//! # ~/.config/wlrix/autostart.toml
//! [[app]]
//! command = "nm-applet"
//!
//! [[app]]
//! command = "syncthing"
//! args = ["--no-browser"]
//! ```
//!
//! It is read **only when nothing has replaced the app list** -- no `session.toml`, and no
//! `--app`/`--no-apps` on the command line. The two files answer different questions: "what
//! is my session?" and "what else should run alongside the usual one?". Honoring both at
//! once would mean a `session.toml` listing three apps quietly starting four, and
//! `--no-apps` -- which exists to get a bare compositor -- would stop being true.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::Deserialize;

/// Where the config lives, relative to a config directory.
const CONFIG_NAME: &str = "wlrix/session.toml";
/// The extra apps to start alongside the default session, relative to a config directory.
const AUTOSTART_NAME: &str = "wlrix/autostart.toml";
/// Consulted when the user has no config of their own.
const SYSTEM_CONFIG_DIR: &str = "/etc";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The compositor to run. `None` leaves the built-in default.
    pub compositor: Option<String>,
    /// Apps to start once the compositor is up, in order.
    #[serde(default, rename = "app")]
    pub apps: Vec<AppSpec>,
    /// Extra environment for the compositor and every app.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// One app to start.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppSpec {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

impl AppSpec {
    /// A command with no arguments, as `--app` gives.
    pub fn command(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
        }
    }
}

/// A config file, and where it came from.
pub struct Loaded {
    pub config: Config,
    pub source: Source,
}

/// Where the session's settings came from.
///
/// "No file" and "a file we could not use" both end in the defaults, but they are not
/// the same thing to whoever is reading the log: one is the ordinary case on a fresh
/// install, the other means a file they wrote is being ignored.
pub enum Source {
    /// No config file anywhere.
    None,
    /// Read and used.
    File(PathBuf),
    /// Found, but unusable. The reason has already been reported.
    Rejected(PathBuf),
}

impl Source {
    /// Whether a config file actually had a say.
    ///
    /// This is what decides if the built-in defaults apply: a rejected file must not
    /// be read as "the user asked for no apps".
    pub fn is_configured(&self) -> bool {
        matches!(self, Self::File(_))
    }
}

/// Read the session config.
///
/// A broken config is reported and then ignored rather than being fatal. A session is
/// often started by a display manager with nothing watching its output, and refusing to
/// start leaves a black screen and no way in to fix the typo; coming up with defaults at
/// least gives a desktop to repair it from.
pub fn load() -> Loaded {
    let Some(path) = find(CONFIG_NAME) else {
        return Loaded {
            config: Config::default(),
            source: Source::None,
        };
    };

    let rejected = |path: PathBuf| Loaded {
        config: Config::default(),
        source: Source::Rejected(path),
    };

    match std::fs::read_to_string(&path) {
        Ok(text) => match toml::from_str::<Config>(&text) {
            Ok(config) => Loaded {
                config,
                source: Source::File(path),
            },
            Err(err) => {
                warn!("{} is not valid: {err}", path.display());
                rejected(path)
            }
        },
        Err(err) => {
            warn!("could not read {}: {err}", path.display());
            rejected(path)
        }
    }
}

/// The extra apps to start after the default session.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Autostart {
    /// Apps to start once the default ones are up, in order.
    ///
    /// No `compositor` or `env` here on purpose: both are settled before the compositor
    /// starts, and autostart runs long after. Accepting them would be accepting something
    /// that could not take effect, and `deny_unknown_fields` says so instead.
    #[serde(default, rename = "app")]
    pub apps: Vec<AppSpec>,
}

/// Read `autostart.toml`, if there is one.
///
/// Only called when nothing has replaced the app list -- see the module docs. A broken file
/// is reported and ignored, like the session config: a typo in an autostart entry should cost
/// the entry, not the desktop.
pub fn load_autostart() -> (Vec<AppSpec>, Option<PathBuf>) {
    let Some(path) = find(AUTOSTART_NAME) else {
        return (Vec::new(), None);
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => match toml::from_str::<Autostart>(&text) {
            Ok(autostart) => (autostart.apps, Some(path)),
            Err(err) => {
                warn!("{} is not valid: {err}", path.display());
                (Vec::new(), None)
            }
        },
        Err(err) => {
            warn!("could not read {}: {err}", path.display());
            (Vec::new(), None)
        }
    }
}

/// Whether an `autostart.toml` exists at all, for reporting that it is being skipped.
///
/// Without this a user whose `session.toml` shadows their autostart gets no hint at all --
/// they would just see it never run.
pub fn autostart_path() -> Option<PathBuf> {
    find(AUTOSTART_NAME)
}

/// The session's app list before autostart, and whether autostart adds to it.
///
/// An empty list in a config file means "no apps", so the built-in defaults only apply when
/// nothing said anything at all -- and a *rejected* file must not read as "the user asked for
/// no apps", which is what [`Source::is_configured`] is for.
///
/// Split out from `main` because this precedence is the whole of the autostart feature, and
/// getting it wrong is invisible until someone's session starts the wrong things.
pub fn resolve_apps(
    from_args: Option<Vec<AppSpec>>,
    loaded: Loaded,
    defaults: &[&str],
) -> (Vec<AppSpec>, bool) {
    match (from_args, loaded.source.is_configured()) {
        // `--app` / `--no-apps`: the most deliberate act there is.
        (Some(apps), _) => (apps, false),
        // A `session.toml` that parsed. It is the session.
        (None, true) => (loaded.config.apps, false),
        // Nothing replaced the list, so the defaults stand and autostart may add to them.
        (None, false) => (
            defaults.iter().map(|app| AppSpec::command(*app)).collect(),
            true,
        ),
    }
}

/// The first file of this name that exists: the user's, then the system's.
fn find(name: &str) -> Option<PathBuf> {
    config_dirs()
        .into_iter()
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

/// Directories to look in, most specific first.
fn config_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = user_config_dir() {
        dirs.push(dir);
    }
    dirs.push(Path::new(SYSTEM_CONFIG_DIR).to_path_buf());
    dirs
}

/// `$XDG_CONFIG_HOME`, or `~/.config` as the spec says to assume.
fn user_config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME")
        && !dir.is_empty()
    {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULTS: &[&str] = &["wlrix-desktop", "wlrix-toolchest"];

    fn loaded(source: Source, apps: Vec<&str>) -> Loaded {
        Loaded {
            config: Config {
                apps: apps.into_iter().map(AppSpec::command).collect(),
                ..Config::default()
            },
            source,
        }
    }

    fn names(apps: &[AppSpec]) -> Vec<&str> {
        apps.iter().map(|app| app.command.as_str()).collect()
    }

    #[test]
    fn with_nothing_configured_the_defaults_run_and_autostart_applies() {
        let (apps, autostart) = resolve_apps(None, loaded(Source::None, vec![]), DEFAULTS);
        assert_eq!(names(&apps), DEFAULTS);
        assert!(autostart, "the default session is what autostart adds to");
    }

    #[test]
    fn a_session_file_replaces_the_list_and_shuts_autostart_out() {
        // The whole rule: `session.toml` says what the session *is*. Appending to it would
        // mean a file listing one app quietly starting several.
        let (apps, autostart) = resolve_apps(
            None,
            loaded(Source::File("/etc/wlrix/session.toml".into()), vec!["foo"]),
            DEFAULTS,
        );
        assert_eq!(names(&apps), ["foo"]);
        assert!(!autostart);
    }

    #[test]
    fn an_empty_session_file_means_no_apps_at_all() {
        let (apps, autostart) = resolve_apps(
            None,
            loaded(Source::File("/etc/wlrix/session.toml".into()), vec![]),
            DEFAULTS,
        );
        assert!(apps.is_empty(), "an empty list is a choice, not an absence");
        assert!(!autostart, "autostart must not refill an emptied session");
    }

    #[test]
    fn command_line_apps_replace_the_list_and_shut_autostart_out() {
        let (apps, autostart) = resolve_apps(
            Some(vec![AppSpec::command("only-this")]),
            loaded(Source::None, vec![]),
            DEFAULTS,
        );
        assert_eq!(names(&apps), ["only-this"]);
        assert!(!autostart);
    }

    #[test]
    fn no_apps_stays_true() {
        // `--no-apps` exists to get a bare compositor. Autostart running anyway would make
        // the flag a lie.
        let (apps, autostart) =
            resolve_apps(Some(Vec::new()), loaded(Source::None, vec![]), DEFAULTS);
        assert!(apps.is_empty());
        assert!(!autostart);
    }

    #[test]
    fn the_command_line_beats_a_session_file() {
        let (apps, autostart) = resolve_apps(
            Some(vec![AppSpec::command("only-this")]),
            loaded(Source::File("/etc/wlrix/session.toml".into()), vec!["foo"]),
            DEFAULTS,
        );
        assert_eq!(names(&apps), ["only-this"]);
        assert!(!autostart);
    }

    #[test]
    fn a_rejected_session_file_falls_back_to_the_defaults_with_autostart() {
        // A file that would not parse must not read as "the user asked for no apps" -- and
        // since the defaults are what ends up running, autostart still applies to them.
        let (apps, autostart) = resolve_apps(
            None,
            loaded(Source::Rejected("/etc/wlrix/session.toml".into()), vec![]),
            DEFAULTS,
        );
        assert_eq!(names(&apps), DEFAULTS);
        assert!(autostart);
    }

    #[test]
    fn an_autostart_file_parses_the_same_shape_as_the_session_one() {
        let autostart: Autostart = toml::from_str(
            "[[app]]\n\
             command = \"nm-applet\"\n\
             \n\
             [[app]]\n\
             command = \"syncthing\"\n\
             args = [\"--no-browser\"]\n",
        )
        .expect("should parse");
        assert_eq!(names(&autostart.apps), ["nm-applet", "syncthing"]);
        assert_eq!(autostart.apps[1].args, ["--no-browser"]);
    }

    #[test]
    fn an_empty_autostart_file_is_valid_and_adds_nothing() {
        let autostart: Autostart = toml::from_str("").expect("should parse");
        assert!(autostart.apps.is_empty());
    }

    #[test]
    fn autostart_refuses_keys_it_could_not_honour() {
        // `compositor` and `env` are both settled before the compositor starts, long before
        // autostart runs. Accepting them silently would be accepting a setting that does
        // nothing; `deny_unknown_fields` turns that into a message.
        assert!(toml::from_str::<Autostart>("compositor = \"other\"\n").is_err());
        assert!(toml::from_str::<Autostart>("[env]\nFOO = \"bar\"\n").is_err());
        // And a plain typo, which is the case it is really there for.
        assert!(toml::from_str::<Autostart>("[[apps]]\ncommand = \"x\"\n").is_err());
    }
}
