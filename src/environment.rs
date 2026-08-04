// SPDX-License-Identifier: GPL-3.0-or-later
//! The environment every process in the session inherits, on top of the session's own.
//!
//! What is here is only what the session *learns*: `WAYLAND_DISPLAY` and `DISPLAY`, which are
//! not known until the compositor and XWayland are up, plus anything the config's `[env]`
//! adds. That is why apps start after the compositor rather than alongside it.
//!
//! What says which desktop this is -- `XDG_CURRENT_DESKTOP`, `XDG_SESSION_DESKTOP`,
//! `XDG_SESSION_TYPE` -- is **not** set here. `start-wlrix` exports it before the session
//! manager starts, along with the toolkit backends, the keyring and the systemd user
//! environment, and it is inherited from there. Setting it in both places meant two answers to
//! the same question, and they had already drifted: the script said `XDG_SESSION_DESKTOP=wlRIX`
//! and this said `wlrix`.
//!
//! Children get the process environment plus these, not instead of it -- nothing calls
//! `env_clear` -- so the script's exports reach them either way.

use std::collections::BTreeMap;

/// What the session hands to every process it starts.
#[derive(Debug, Default, Clone)]
pub struct Environment {
    vars: BTreeMap<String, String>,
}

impl Environment {
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
