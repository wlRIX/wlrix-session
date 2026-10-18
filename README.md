# wlrix-session

The wlRIX session manager. Starts `wlrix-compositor`, waits to learn the environment only the compositor can decide,
starts the desktop's apps with it, and stays until the compositor exits.

- **Language:** Rust
- **License:** GPL-3.0-or-later
- **Reference:** [cosmic-session](https://github.com/pop-os/cosmic-session)

## Build

```sh
cargo build
cargo run
```

## Installing

```sh
just release
sudo just install
```

`install` deliberately does not build: it is normally run as root, and building as root leaves a target directory nobody
can write to afterwards. `prefix` (default `/usr`) and `rootdir` (a staging root, as a package build wants) override
where things land.

What lands:

| Path                                            |                                         |
|-------------------------------------------------|-----------------------------------------|
| `$prefix/bin/wlrix-session`                     | the session manager                     |
| `$prefix/bin/start-wlrix`                       | what a display manager actually runs    |
| `$prefix/lib/systemd/user/wlrix-session.target` | the user target other units can bind to |
| `$prefix/share/wayland-sessions/wlrix.desktop`  | the entry a display manager offers      |

`start-wlrix`, `wlrix-compositor` and the desktop's apps need to be on `PATH`, or named by absolute path in the config.

### start-wlrix

The session entry runs the **script**, not the binary. `start-wlrix` is where the session's environment is set up, and
`wlrix-session` deliberately does not repeat any of it:

- `XDG_CURRENT_DESKTOP`, `XDG_SESSION_DESKTOP` — set only if they are not already; a display manager fills the first in
  from `DesktopNames`
- `XDG_SESSION_TYPE=wayland` — **forced**, unlike the two above. `pam_systemd` sets it from how the login arrived, so a
  console login gets `tty` and so does a greetd login, which logind sees as a console login. Once the compositor is up
  this is a Wayland session whatever it was started from, and the variable is what toolkits auto-detect on: Chromium and
  Edge resolve `--ozone-platform-hint=auto` by reading it and fall back to X11 when it does not say `wayland`
- the toolkit backends — `GDK_BACKEND`, `MOZ_ENABLE_WAYLAND`, `QT_QPA_PLATFORM` and friends
- gnome-keyring, and `SSH_AUTH_SOCK` if a socket actually turned up
- importing all of that into the systemd user environment, and clearing failed units left by a previous session
- a D-Bus session bus, if there is not one already

That is why `wlrix-greeter --session` defaults to `start-wlrix` too. Starting `wlrix-session` directly — by hand, or
from a hand-written session entry — skips the lot, and the first symptom is usually a toolkit picking X11 or portals not
knowing which desktop this is.

Note that `desktop-file-validate` objects to `DesktopNames`, which is how a session tells the display manager what to
set `XDG_CURRENT_DESKTOP` to. It is a session-file convention rather than part of the desktop-entry spec, so the
validator does not know it — the shipped `plasma.desktop` fails the same check. It is correct; leave it.

## Configuration

Read from `$XDG_CONFIG_HOME/wlrix/session.toml` (or `~/.config/wlrix/session.toml`), falling back to
`/etc/wlrix/session.toml`. The first file found is used whole rather than merged, so your own file is all of what you
get. Without a config file the built-in defaults apply.

```toml
# The compositor to run.
compositor = "wlrix-compositor"

# Apps to start once the compositor is up, in order.
[[app]]
command = "wlrix-toolchest"

[[app]]
command = "alacritty"
args = ["-e", "fish"]

# Extra environment for the compositor and every app.
[env]
QT_QPA_PLATFORM = "wayland"
```

An `[[app]]` list that is present but empty means no apps. Unknown keys are an error, so a typo is reported rather than
quietly ignored. A config that cannot be parsed is reported and then ignored: a session is usually started by a display
manager with nothing watching its output, and refusing to start would leave a black screen and no way in to fix it.

Note that a `session.toml` replaces the default list outright, and some of what is on it is not decoration. In
particular `wlrix-idle` is what blanks the screen on a timeout and locks the session before the machine suspends — a
`session.toml` that does not list it gets a session that does neither, and one that does not list `wlrix-clipboard` has
no clipboard history (and `Super+V` opens a popup that says so). One that does not list `wlrix-sound` is silent: every
component asks for its sounds, and with no daemon to answer nothing plays. One that does not list `wlrix-polkit-agent`
cannot ask for an administrator's password: pkexec then has nowhere to prompt but the VT the session started on, hidden
under the compositor, so Login Settings and the Software Manager report that no agent is running. One that does not list
`wlrix-network` has no Wi-Fi icon in the tray. One that does not list
`wlrix-osd` gets whichever notification daemon D-Bus activates first — mako or dunst if one is installed, otherwise
notifications go nowhere. `wlrix-session --help` prints the current default list.

`wlrix-sound` goes first, before anything that might ask for a sound. `wlrix-osd` goes straight after it, so it owns
`org.freedesktop.Notifications` before anything can send a notification and D-Bus-activate another daemon instead. Once the apps are started the session plays the
login sound, waiting up to a second for the daemon to be listening if it was on the list. On a logout or shutdown it
plays the logout sound and waits up to three seconds for it to finish before stopping the apps. A crash plays nothing.

**Order matters for two of them.** `wlrix-tray` has to come after `wlrix-desktop`: both are wlr-layer-shell *bottom*
surfaces, the compositor gives the pointer to whichever mapped later where they overlap, and the desktop's surface
covers the whole output — so a tray started first is drawn but unclickable. `wlrix-bg` before `wlrix-desktop` is only
cosmetic by comparison; they are on different layers and cannot race.

### Autostart

`session.toml` says what the session *is*; `autostart.toml`, in the same directories and found the same way, says what
else should run **alongside the usual one**. Its apps start after the default ones, in order.

```toml
# ~/.config/wlrix/autostart.toml
[[app]]
command = "nm-applet"

[[app]]
command = "syncthing"
args = ["--no-browser"]
```

Both of those put a status icon in `wlrix-tray` rather than a window on the desktop, which is what an autostart entry
usually wants to do.

Only `[[app]]` is accepted here. There is no `compositor` or `[env]`, because both are settled before the compositor
starts and autostart runs long after — accepting them would be accepting a setting that could not take effect.

It is read **only when nothing has replaced the app list**: no `session.toml`, and no `--app`/`--no-apps` on the command
line. Honoring both at once would mean a `session.toml` listing three apps quietly starting four, and `--no-apps` —
which exists to get a bare compositor — would stop being true. When an `autostart.toml` is being skipped for this
reason, the session log says so, rather than leaving you to wonder why it never ran.

### Options

Anything given on the command line wins over the config file.

| Option                   | Effect                                                 |
|--------------------------|--------------------------------------------------------|
| `--compositor <command>` | compositor to run                                      |
| `--app <command>`        | app to start; repeatable, replaces the configured list |
| `--no-apps`              | start the compositor only                              |
| `-h`, `--help`           | usage                                                  |

## Logs

Both the session and the compositor write to the temp directory as well as the terminal, truncating at startup so each
file is a record of the current session (in the per-user runtime dir, so the greeter's logs and the session's never
collide):

| File                                    | Contents                                                        |
|-----------------------------------------|-----------------------------------------------------------------|
| `$XDG_RUNTIME_DIR/wlrix-session.log`    | the session's own messages, **and the apps' stdout and stderr** |
| `$XDG_RUNTIME_DIR/wlrix-compositor.log` | the compositor's log                                            |

This matters most on a TTY, where the console the session was writing to is covered by the compositor the moment it
starts: the text is there, but none of it can be read. If an app does not appear, its own error is in the session log.

## Portals and the systemd user session

Once the compositor is up, the session hands `WAYLAND_DISPLAY`, `DISPLAY`, the `XDG_*` three and the `XCURSOR_*` pair to
**two** places and then starts `wlrix-session.target`:

| Where                            | Who reads it                                                             |
|----------------------------------|--------------------------------------------------------------------------|
| the systemd user manager         | units: their `ConditionEnvironment`, and what their processes start with |
| the D-Bus activation environment | services started on demand by a bus request                              |

`start-wlrix` cannot do this part. It runs before the compositor exists, so there is no socket name to publish yet —
which is why it imports only what it already knows.

Getting it wrong is quiet. `xdg-desktop-portal-wlr.service` carries
`ConditionEnvironment=WAYLAND_DISPLAY`, checked against the *user manager's* environment rather than the session
process's, so without this it is skipped with `unmet condition check` and a screen-capture client is simply told there
are no sources. The portal is a `Type=dbus` unit, so it needs the activation environment too: a service the bus starts
inherits nothing from us, and reads `XDG_CURRENT_DESKTOP` to decide which backend to load.

`wlrix-session.target` is started after the variables are published, never before — a unit with a `ConditionEnvironment`
is checked as it starts, and one started too early is skipped and stays skipped. It is stopped again when the compositor
exits, so its units do not linger into the next session holding a stale `WAYLAND_DISPLAY`.

All of it is best-effort: a desktop that refused to start because a portal helper was missing would be trading a working
session for a working screenshot.

## How the environment reaches apps

`WAYLAND_DISPLAY` and `DISPLAY` are not known until the compositor and XWayland are running, and a process cannot be
handed them after it has started. So the compositor reports both on stdout as `KEY=VALUE` lines (with
`WLRIX_SESSION_HANDSHAKE=1` set, which the session does), and apps are started only once they have arrived. This is what
makes X11 apps work on a machine whose login environment has no `DISPLAY` at all.

Waiting for XWayland is not fatal — a session without X11 still starts, with a warning.

`XCURSOR_THEME` and `XCURSOR_SIZE` travel the same way, for a different reason: they are not *discovered*, they are
decided, in the compositor's `[cursor]` config. A toolkit loads its own pointer from those two variables, so an app
started without them draws Adwaita's cursor over a desktop drawing the theme wlRIX ships. Sending them through the
handshake keeps that decision in one file instead of splitting it between `compositor.toml` and `start-wlrix`, which is
how `XDG_SESSION_DESKTOP` came to have two different values before it was moved to the script alone.

Only these four keys are accepted from the compositor's stdout; anything else on it is passed through to the log as
output rather than trusted as configuration.
