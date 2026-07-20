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

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::Deserialize;

/// Where the config lives, relative to a config directory.
const CONFIG_NAME: &str = "wlrix/session.toml";
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
    /// `None` when no config file exists, which is not an error -- it is the ordinary
    /// case on a fresh install, and the built-in defaults apply.
    ///
    /// Also, `None` when a file existed but could not be read, since the defaults are
    /// what ends up being used either way.
    pub path: Option<PathBuf>,
}

/// Read the session config.
///
/// A broken config is reported and then ignored rather than being fatal. A session is
/// often started by a display manager with nothing watching its output, and refusing to
/// start leaves a black screen and no way in to fix the typo; coming up with defaults at
/// least gives a desktop to repair it from.
pub fn load() -> Loaded {
    let Some(path) = find() else {
        return Loaded {
            config: Config::default(),
            path: None,
        };
    };

    match std::fs::read_to_string(&path) {
        Ok(text) => match toml::from_str::<Config>(&text) {
            Ok(config) => Loaded {
                config,
                path: Some(path),
            },
            Err(err) => {
                eprintln!("wlrix-session: {} is not valid: {err}", path.display());
                eprintln!("wlrix-session: carrying on with the default session");
                Loaded {
                    config: Config::default(),
                    path: None,
                }
            }
        },
        Err(err) => {
            eprintln!("wlrix-session: could not read {}: {err}", path.display());
            Loaded {
                config: Config::default(),
                path: None,
            }
        }
    }
}

/// The first config file that exists: the user's, then the system's.
fn find() -> Option<PathBuf> {
    config_dirs()
        .into_iter()
        .map(|dir| dir.join(CONFIG_NAME))
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
