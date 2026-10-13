// SPDX-License-Identifier: GPL-3.0-or-later
//! wlRIX session manager.
//!
//! Starts the compositor, waits to learn the environment only it can decide, starts the
//! desktop's apps with that environment, and stays until the compositor exits.
//!
//! The ordering is the whole point. `WAYLAND_DISPLAY` and `DISPLAY` are not known until
//! the compositor and XWayland are running, and a process cannot be given them after it
//! has started -- so apps wait, rather than starting alongside and finding an empty
//! `DISPLAY`.
//!
//! The compositor is the session: when it exits, the session tears down and exits with
//! its status.

#[macro_use]
mod log;

mod apps;
mod compositor;
mod config;
mod environment;
mod signals;
mod sound;
mod systemd;

use std::time::Duration;

use config::AppSpec;
use environment::Environment;

/// The compositor to run, unless `--compositor` says otherwise.
const DEFAULT_COMPOSITOR: &str = "wlrix-compositor";

/// What the command line asked for. Everything is optional: anything not given here is
/// taken from the config file, and anything absent there falls back to a built-in
/// default. The command line wins because it is the more deliberate act.
#[derive(Default)]
struct Args {
    compositor: Option<String>,
    apps: Option<Vec<AppSpec>>,
}

fn parse_args() -> Result<Args, String> {
    let mut compositor = None;
    let mut apps: Option<Vec<AppSpec>> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--compositor" => {
                compositor = Some(
                    args.next()
                        .ok_or_else(|| "--compositor needs a command".to_string())?,
                );
            }
            // Repeatable. Giving any replaces the configured list rather than adding
            // to it, so `--app foo` means "just foo".
            "--app" => {
                let app = args
                    .next()
                    .ok_or_else(|| "--app needs a command".to_string())?;
                apps.get_or_insert_default().push(AppSpec::command(app));
            }
            "--no-apps" => {
                apps = Some(Vec::new());
            }
            "--help" | "-h" => {
                println!(
                    "wlrix-session {}\n\n\
                     Usage: wlrix-session [options]\n\n\
                     Options:\n  \
                       --compositor <command>  compositor to run (default: {DEFAULT_COMPOSITOR})\n  \
                       --app <command>         app to start; repeatable, replaces the configured list\n  \
                       --no-apps               start the compositor only\n  \
                       -h, --help              this message\n\n\
                     Config: $XDG_CONFIG_HOME/wlrix/session.toml, else /etc/wlrix/session.toml\n\
                     Autostart: the same directories' autostart.toml, added to the default\n\
                     \x20          session only -- not when a config file or --app/--no-apps\n\
                     \x20          has already said what the session is\n\
                     Default apps when unconfigured: {}",
                    env!("CARGO_PKG_VERSION"),
                    apps::DEFAULT_APPS.join(", ")
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(Args { compositor, apps })
}

fn main() -> std::process::ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(err) => {
            warn!("{err}");
            return std::process::ExitCode::from(2);
        }
    };

    // Before anything is reported, so nothing is lost from the record.
    let log_path = log::init();
    if let Some(path) = &log_path {
        info!("logging to {}", path.display());
    }

    let mut loaded = config::load();
    match &loaded.source {
        config::Source::File(path) => info!("config {}", path.display()),
        config::Source::Rejected(path) => {
            info!("ignoring {}; using the default session", path.display())
        }
        config::Source::None => info!("no config file; using defaults"),
    }

    // Starts empty: what says which desktop this is comes from `start-wlrix` and is already in
    // this process's environment, which children inherit. See `environment`.
    let mut environment = Environment::default();
    for (key, value) in &loaded.config.env {
        environment.set(key, value);
    }
    // SAFETY: startup, before any thread is spawned.
    unsafe { environment.export() };

    // Taken rather than moved out: `loaded` is still needed below to decide the app list.
    let compositor_command = args
        .compositor
        .or(loaded.config.compositor.take())
        .unwrap_or_else(|| DEFAULT_COMPOSITOR.to_string());

    // An empty list in a config file means "no apps", so the defaults only apply when
    // nothing said anything at all.
    //
    // `autostart.toml` adds to the *default* session and nothing else. Anything that
    // replaced the app list -- a `session.toml`, or `--app`/`--no-apps` -- has said what the
    // session is, and quietly appending to it would make `--no-apps` untrue and a
    // three-app `session.toml` start four. See `config`'s module docs.
    let (mut app_list, takes_autostart) =
        config::resolve_apps(args.apps, loaded, apps::DEFAULT_APPS);
    if takes_autostart {
        let (extra, from) = config::load_autostart();
        if let Some(path) = from {
            info!("autostart: {} app(s) from {}", extra.len(), path.display());
        }
        app_list.extend(extra);
    } else if let Some(path) = config::autostart_path() {
        // Say so rather than leaving the user to wonder why their autostart never runs.
        info!(
            "not reading {}: the app list was set explicitly",
            path.display()
        );
    }

    info!("starting {compositor_command}");
    let mut compositor = match compositor::start(&compositor_command, &environment) {
        Ok(compositor) => compositor,
        Err(err) => {
            warn!("{err}");
            return std::process::ExitCode::FAILURE;
        }
    };

    // From here on a shutdown request has somewhere to go. Installed before the apps
    // start, so a signal arriving mid-startup still brings the compositor down rather
    // than orphaning it.
    signals::forward_to(compositor.child.id());

    for key in [
        "WAYLAND_DISPLAY",
        "DISPLAY",
        "XCURSOR_THEME",
        "XCURSOR_SIZE",
    ] {
        match compositor.environment.get(key) {
            Some(value) => info!("{key}={value}"),
            None => info!("{key} unset"),
        }
    }

    // Now that there is a display to name, tell the user manager and the bus about it, and
    // bring the session target up. Before the apps, so anything they ask a portal for finds
    // one running. See `systemd`.
    systemd::publish(&compositor.environment);
    systemd::start_target();

    // Before the apps, so no exit can happen in the gap between starting one and being able
    // to hear about it.
    signals::watch_children();
    let mut started = apps::start_all(&app_list, &compositor.environment);

    // IRIX's boot chime, once the desktop is coming up. The sound daemon was started a moment
    // ago with the rest and may not be listening yet, so it is given a second -- but only when
    // this session started it: a list without it is a session that wanted no sounds, and should
    // not pay a second at every login for asking.
    if app_list.iter().any(|app| app.command == "wlrix-sound") {
        sound::wait_for_daemon(Duration::from_secs(1));
    }
    sound::play("desktop-login");

    // The compositor is the session: when it goes, so does everything. Nothing *else* is worth
    // ending the session for -- an app that dies leaves a desktop that carries on without it --
    // but every app still has to be waited for as it exits, or it stays behind as a zombie.
    //
    // Hence a loop rather than a plain `wait` on the compositor: the session sleeps until a
    // child of any kind has gone, then works out which. The compositor is asked first, so its
    // exit is never delayed by a round of reaping, and the apps are reaped before the sleep so
    // that anything which exited before this loop was reached is collected on the first pass.
    let status = loop {
        match compositor.child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Err(err) => break Err(err),
            Ok(None) => {}
        }
        apps::reap(&mut started);
        signals::wait_for_child_exit();
    };

    // The logout sound, heard out before the sound daemon goes down with the other apps. Only
    // for a session that is ending on purpose -- a logout, or a shutdown passed on to the
    // compositor -- and not for a crash, which is not a farewell. Bounded, so a sound server
    // that has already gone cannot hold the logout up.
    if matches!(&status, Ok(status) if status.success()) || signals::shutdown_requested() {
        sound::play_and_wait("desktop-logout", Duration::from_secs(3));
    }
    apps::stop_all(started);
    systemd::stop_target();

    match status {
        Ok(status) => {
            info!("compositor exited ({status})");
            // A compositor that died because we passed a shutdown signal on to it did
            // what was asked, even though it exited by signal rather than cleanly.
            if status.success() || signals::shutdown_requested() {
                std::process::ExitCode::SUCCESS
            } else {
                std::process::ExitCode::FAILURE
            }
        }
        Err(err) => {
            warn!("could not wait for the compositor: {err}");
            std::process::ExitCode::FAILURE
        }
    }
}
