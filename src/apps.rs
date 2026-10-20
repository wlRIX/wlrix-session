// SPDX-License-Identifier: GPL-3.0-or-later
//! The desktop's own apps, started once the compositor is up.
//!
//! These are ordinary Wayland clients -- the toolchest and desks are normal windows in
//! IRIX, not shell components -- so there is nothing special about starting them beyond
//! doing it after the compositor exists and with its environment. `wlrix-desktop` is the
//! exception in kind, being a layer-shell background surface rather than a window, but it
//! is started exactly the same way.
//!
//! An app that is missing or fails to start is reported and skipped. A desktop with no
//! toolchest is poor, but better than no desktop at all, and the compositor is still
//! there to launch things from.

use std::process::{Child, Command};

use crate::{config::AppSpec, environment::Environment};

/// What the session starts when there is no config file to say otherwise.
///
/// The wallpaper comes first and the desktop second, so each is up before anything can open
/// over it -- and in that order, because `wlrix-desktop`'s icons are drawn on the layer *above*
/// `wlrix-bg`'s wallpaper and are transparent between them. Nothing depends on the ordering --
/// a layer surface goes below windows whenever it arrives, and the two are on different layers
/// so they cannot race for stacking -- but starting the backdrop last would show a bare desktop
/// for a moment.
///
/// **`wlrix-tray` after `wlrix-desktop` is not free the way those two are.** Both are on the
/// wlr-layer-shell *bottom* layer, and the compositor's `layer_under` walks a layer's surfaces in
/// map order, newest first -- so the one started later takes the pointer where they overlap.
/// Started before the desktop, the tray would be drawn on screen and unclickable, since the
/// desktop's surface covers the whole output.
///
/// `wlrix-network`, `wlrix-power` and `wlrix-bluetooth` follow the tray, though nothing depends on
/// them: they publish the Wi-Fi, battery and Bluetooth icons the tray draws, and register again
/// whenever a tray appears, so they may start in either order.
///
/// `wlrix-idle` and `wlrix-clipboard` draw nothing at all, so where they sit is free; they go
/// last because a session that comes up without them is merely one that never blanks or forgets
/// what was copied, while a session that comes up without a toolchest has no way to start
/// anything.
///
/// `wlrix-sound` draws nothing either, but goes **first**: it is the one the others ask for
/// sounds, and the session's own login sound follows straight after this list is started. It
/// is not a Wayland client, so starting it before anything is on screen costs nothing.
///
/// `wlrix-osd` follows it, and early is the point. It is the notification server, and that is
/// decided by whoever owns `org.freedesktop.Notifications` first: mako and dunst are D-Bus
/// activated, so the first program to send a notification before `wlrix-osd` has taken the name
/// starts one of them instead, and it keeps the name for the rest of the session. It takes the name
/// before it connects to the compositor, so being early in this list is enough.
///
/// `wlrix-polkit-agent` follows that: it draws nothing until somebody needs to type a password, and
/// it should be registered with polkitd before anything in the session can ask for one. Without
/// it, pkexec has nowhere to ask but the VT the session was started on, hidden under the
/// compositor.
pub const DEFAULT_APPS: &[&str] = &[
    "wlrix-sound",
    "wlrix-osd",
    "wlrix-polkit-agent",
    "wlrix-bg",
    "wlrix-desktop",
    "wlrix-tray",
    "wlrix-network",
    "wlrix-power",
    "wlrix-bluetooth",
    "wlrix-toolchest",
    "wlrix-desks",
    "wlrix-idle",
    "wlrix-clipboard",
];

pub struct App {
    pub name: String,
    pub child: Child,
}

/// Start each app, skipping any that will not run.
pub fn start_all(apps: &[AppSpec], environment: &Environment) -> Vec<App> {
    let mut started = Vec::new();
    for app in apps {
        match Command::new(&app.command)
            .args(&app.args)
            .envs(environment.iter())
            // Into the session log: an app that fails on startup says why on its own
            // stderr, and on a TTY that would go to the console the compositor covers.
            .stdout(crate::log::child_output())
            .stderr(crate::log::child_output())
            .spawn()
        {
            Ok(child) => {
                info!("started {} (pid {})", app.command, child.id());
                started.push(App {
                    name: app.command.clone(),
                    child,
                });
            }
            Err(err) => {
                warn!("could not start {}: {err}", app.command);
            }
        }
    }
    started
}

/// Collect the apps that have exited on their own, dropping them from the list.
///
/// Closing one is an ordinary thing to do -- the toolchest and desks are windows with a close
/// button, not shell components -- and the desktop carries on without them. But a child nobody
/// waits for stays on the process table as a zombie, and the session used to wait for its apps
/// only at shutdown, so closing the Desks window left `[wlrix-desks] <defunct>` sitting there
/// for the rest of the login.
///
/// Called whenever [`crate::signals::wait_for_child_exit`] reports something happened, so an app
/// is reaped about as soon as it goes.
pub fn reap(apps: &mut Vec<App>) {
    apps.retain_mut(|app| match app.child.try_wait() {
        Ok(Some(status)) => {
            info!("{} exited ({status})", app.name);
            false
        }
        Ok(None) => true,
        // Dropped rather than kept: the only real answer here is that the child is already
        // gone, and keeping it would have `stop_all` signal a pid the system is free to have
        // given to something else by then.
        Err(err) => {
            warn!("could not check {}: {err}", app.name);
            false
        }
    });
}

/// Ask every app to exit, then wait for it.
///
/// Called when the compositor has gone: the apps' windows died with it, and leaving
/// them running would strand processes with no display to draw on.
pub fn stop_all(apps: Vec<App>) {
    for mut app in apps {
        // Already gone is the common case -- their Wayland connection just dropped.
        match app.child.try_wait() {
            Ok(Some(_)) => continue,
            Ok(None) => {}
            Err(err) => {
                warn!("could not check {}: {err}", app.name);
                continue;
            }
        }
        if let Err(err) = app.child.kill() {
            warn!("could not stop {}: {err}", app.name);
            continue;
        }
        let _ = app.child.wait();
        info!("stopped {}", app.name);
    }
}
