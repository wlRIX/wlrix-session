// SPDX-License-Identifier: GPL-3.0-or-later
//! Telling the rest of the login session that a Wayland desktop is up.
//!
//! `start-wlrix` publishes what it knows before starting anything, but it cannot publish
//! `WAYLAND_DISPLAY`: the compositor does not exist yet, and the socket name is not decided
//! until it does. So the variables that matter most to everything activated *later* are the
//! ones nothing was pushing anywhere.
//!
//! The visible symptom is a portal that never runs. `xdg-desktop-portal-wlr.service` carries
//! `ConditionEnvironment=WAYLAND_DISPLAY`, which systemd checks against the **user manager's**
//! environment -- not the session process's -- so it is skipped, and a screen-capture client
//! is told there are no sources.
//!
//! Two places have to learn it, and they are not the same place:
//!
//! - the **systemd user manager**, for units: their `ConditionEnvironment`, and the
//!   environment a unit's own processes start with.
//! - the **D-Bus activation environment**, for services started on demand by a bus request.
//!   The portal is both at once -- a `Type=dbus` unit -- and a service activated through the
//!   bus does not inherit anything from us.
//!
//! Both are best-effort. A desktop that refused to start because a portal helper was missing
//! would be trading a working session for a working screenshot.

use std::process::{Command, Stdio};

use crate::environment::Environment;

/// The session target, started once the compositor is up.
///
/// It `BindsTo=graphical-session.target` and orders itself before it, so starting this is what
/// pulls in everything the login session hangs off `graphical-session.target`.
const TARGET: &str = "wlrix-session.target";

/// What is published, in order of who needs it.
///
/// `WAYLAND_DISPLAY` and `DISPLAY` are the ones only the compositor can decide. The `XDG_*`
/// three come from `start-wlrix` and are already in this process's environment, but they are
/// republished here because the script only reaches the systemd side: a portal activated
/// through the bus reads `XDG_CURRENT_DESKTOP` to decide which backend to load, and it is not
/// in the activation environment unless somebody puts it there.
///
/// The `XCURSOR_*` pair also comes from the compositor, over the same handshake, and is here for
/// the same reason as the `XDG_*` three: a settings panel or a file dialog that the bus activates
/// is not started by this process and would otherwise load a toolkit default pointer over a
/// desktop drawing the theme from `compositor.toml`.
///
/// `GTK_MODULES` is here for exactly that file dialog. `start-wlrix` exports it so every GTK 3
/// application loads `wlrix-gtk-config` -- the module that reloads the user's gtk.css on a scheme
/// change and takes GtkHeaderBar down to the compositor's 30px -- but the file chooser an
/// application opens through the portal is drawn by `xdg-desktop-portal-gtk`, a bus-activated
/// service that inherits nothing from this process. Found on a live session: every GTK 3
/// application the session started had the module mapped, and the portal did not. Without this
/// every file dialog on the desktop kept a 38px titlebar and the scheme it started with.
///
/// No cleanup at logout is needed for it any more than for the rest: with lingering off -- the
/// default, and this machine's -- the user manager ends with the session and takes its
/// environment with it.
const PUBLISHED: &[&str] = &[
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_TYPE",
    "XDG_SESSION_DESKTOP",
    "XCURSOR_THEME",
    "XCURSOR_SIZE",
    "GTK_MODULES",
];

/// Hand the session's environment to the user manager and the bus.
///
/// `environment` is the compositor's, which carries the handshake variables; anything not in
/// it is taken from this process's own environment. Values are passed explicitly rather than
/// through `systemctl --user import-environment`, which would read *our* environment and so
/// mean calling `set_var` after the handshake reader thread already exists.
pub fn publish(environment: &Environment) {
    let assignments: Vec<String> = PUBLISHED
        .iter()
        .filter_map(|key| {
            let value = match environment.get(key) {
                Some(value) => value.to_string(),
                None => std::env::var(key).ok()?,
            };
            (!value.is_empty()).then(|| format!("{key}={value}"))
        })
        .collect();

    if assignments.is_empty() {
        warn!("nothing to publish to the session bus; portals may not start");
        return;
    }

    // Said out loud, not only on failure. When this does not work the symptom is a portal that
    // quietly never runs, and the first question is always whether the session got this far --
    // which is not a question a log should leave unanswered.
    info!("publishing to the session: {}", assignments.join(" "));

    // The user manager, for units and their conditions.
    run("systemctl", ["--user", "set-environment"], &assignments);
    // The bus, for services activated on demand. `--systemd` makes it do the manager as well,
    // so this alone would usually be enough -- but it comes from the `dbus` package rather
    // than systemd, and a session without it should still get the manager set above.
    run(
        "dbus-update-activation-environment",
        ["--systemd"],
        &assignments,
    );
}

/// Start the session target, so units ordered on `graphical-session.target` run.
///
/// After [`publish`], never before: a unit with a `ConditionEnvironment` is checked as it is
/// started, and one started first would be skipped and stay skipped.
pub fn start_target() {
    info!("starting {TARGET}");
    run("systemctl", ["--user", "start"], &[TARGET.to_string()]);
}

/// Stop the session target on the way out, taking its units with it.
///
/// Without this they linger into the next session, and a unit left running from a compositor
/// that has gone is worse than one that was never started -- it holds a stale
/// `WAYLAND_DISPLAY` and answers requests it cannot serve.
pub fn stop_target() {
    run("systemctl", ["--user", "stop"], &[TARGET.to_string()]);
}

/// Run a command, reporting what it said if it fails. Never fatal.
fn run<const N: usize>(program: &str, flags: [&str; N], arguments: &[String]) {
    let output = Command::new(program)
        .args(flags)
        .args(arguments)
        .stdin(Stdio::null())
        .output();
    match output {
        Ok(output) if output.status.success() => {
            // `dbus-update-activation-environment` reports a failure to reach systemd as a
            // *warning* on stderr and still exits 0, so a clean status is not enough to say
            // it worked.
            let complaint = String::from_utf8_lossy(&output.stderr);
            let complaint = complaint.trim();
            if !complaint.is_empty() {
                warn!("{program}: {complaint}");
            }
        }
        Ok(output) => {
            let reason = String::from_utf8_lossy(&output.stderr);
            warn!("{program} {}: {}", output.status, reason.trim());
        }
        // Not installed is worth one line and no more: a session without systemd or without
        // the D-Bus tools still runs, it just cannot start portals.
        Err(err) => warn!("could not run {program}: {err}"),
    }
}
