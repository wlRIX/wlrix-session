// SPDX-License-Identifier: GPL-3.0-or-later
//! The environment every process in the session inherits.
//!
//! Two kinds of variables live here. The fixed ones say what desktop this is, and can be
//! set before anything starts. The rest -- `WAYLAND_DISPLAY` and `DISPLAY` -- are only
//! known once the compositor is up, which is why apps are started after it rather than
//! alongside it.

use std::collections::BTreeMap;

/// What the session hands to every process it starts.
#[derive(Debug, Default, Clone)]
pub struct Environment {
    vars: BTreeMap<String, String>,
}

impl Environment {
    /// The variables that do not depend on the compositor.
    ///
    /// These are what portals, toolkits, and `.desktop` handling look at to work out
    /// which desktop they are running under.
    pub fn base() -> Self {
        let mut environment = Self::default();
        environment.set("XDG_SESSION_TYPE", "wayland");
        // Capitalized as the brand is; this is the name that appears in `OnlyShowIn`.
        environment.set("XDG_CURRENT_DESKTOP", "wlRIX");
        environment.set("XDG_SESSION_DESKTOP", "wlrix");
        environment
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.vars.insert(key.to_string(), value.to_string());
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.vars.iter()
    }

    /// Apply to the session's own environment as well.
    ///
    /// Children inherit it anyway; this is for anything that reads the session process
    /// itself, and it keeps `wlrix-session`'s view consistent with what it hands out.
    ///
    /// # Safety
    ///
    /// `set_var` is unsafe in edition 2024 because it is not thread-safe. Call this
    /// during single-threaded startup only.
    pub unsafe fn export(&self) {
        for (key, value) in &self.vars {
            unsafe { std::env::set_var(key, value) };
        }
    }
}
