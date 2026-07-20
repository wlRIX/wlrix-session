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

## How the environment reaches apps

`WAYLAND_DISPLAY` and `DISPLAY` are not known until the compositor and XWayland are
running, and a process cannot be handed them after it has started. So the compositor
reports both on stdout as `KEY=VALUE` lines (with `WLRIX_SESSION_HANDSHAKE=1` set, which
the session does), and apps are started only once they have arrived. This is what makes
X11 apps work on a machine whose login environment has no `DISPLAY` at all.

Waiting for XWayland is not fatal — a session without X11 still starts, with a warning.
