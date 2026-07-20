# wlrix-session

The wlRIX session manager. Starts `wlrix-compositor`, waits to learn the environment
only the compositor can decide, starts the desktop's apps with it, and stays until the
compositor exits.

- **Language:** Rust
- **License:** GPL-3.0-or-later
- **Reference:** [cosmic-session](https://github.com/pop-os/cosmic-session)

## Build

```sh
cargo build
cargo run
```

## Installing

The session is launched by a display manager (or by `wlrix-greeter`) through a session
entry:

```sh
install -Dm644 share/wayland-sessions/wlrix.desktop \
    /usr/share/wayland-sessions/wlrix.desktop
```

`wlrix-session`, `wlrix-compositor` and the desktop's apps need to be on `PATH`, or
named by absolute path in the config.

Note that `desktop-file-validate` objects to `DesktopNames`, which is how a session
tells the display manager what to set `XDG_CURRENT_DESKTOP` to. It is a session-file
convention rather than part of the desktop-entry spec, so the validator does not know
it — the shipped `plasma.desktop` fails the same check. It is correct; leave it.

## Configuration

Read from `$XDG_CONFIG_HOME/wlrix/session.toml` (or `~/.config/wlrix/session.toml`),
falling back to `/etc/wlrix/session.toml`. The first file found is used whole rather
than merged, so your own file is all of what you get. Without a config file the
built-in defaults apply.

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

An `[[app]]` list that is present but empty means no apps. Unknown keys are an error,
so a typo is reported rather than quietly ignored. A config that cannot be parsed is
reported and then ignored: a session is usually started by a display manager with
nothing watching its output, and refusing to start would leave a black screen and no
way in to fix it.

### Options

Anything given on the command line wins over the config file.

| Option                   | Effect                                                 |
|--------------------------|--------------------------------------------------------|
| `--compositor <command>` | compositor to run                                      |
| `--app <command>`        | app to start; repeatable, replaces the configured list |
| `--no-apps`              | start the compositor only                              |
| `-h`, `--help`           | usage                                                  |

## Logs

Both the session and the compositor write to the temp directory as well as the
terminal, truncating at startup so each file is a record of the current session:

| File                           | Contents                                                        |
|--------------------------------|-----------------------------------------------------------------|
| `$TMPDIR/wlrix-session.log`    | the session's own messages, **and the apps' stdout and stderr** |
| `$TMPDIR/wlrix-compositor.log` | the compositor's log                                            |

This matters most on a TTY, where the console the session was writing to is covered by
the compositor the moment it starts: the text is there, but none of it can be read. If
an app does not appear, its own error is in the session log.

## How the environment reaches apps

`WAYLAND_DISPLAY` and `DISPLAY` are not known until the compositor and XWayland are
running, and a process cannot be handed them after it has started. So the compositor
reports both on stdout as `KEY=VALUE` lines (with `WLRIX_SESSION_HANDSHAKE=1` set, which
the session does), and apps are started only once they have arrived. This is what makes
X11 apps work on a machine whose login environment has no `DISPLAY` at all.

Waiting for XWayland is not fatal — a session without X11 still starts, with a warning.
