# wlrix-session

The wlRIX session manager. Sets up the environment (Wayland/D-Bus/XDG vars),
launches `wlrix-compositor`, then the core C# desktop apps (toolchest, desks),
and supervises the running session.

- **Language:** Rust
- **License:** GPL-3.0-or-later
- **Reference:** [cosmic-session](https://github.com/pop-os/cosmic-session)

## Build

```sh
cargo build
cargo run
```
