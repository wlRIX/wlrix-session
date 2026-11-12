#!/usr/bin/env just --justfile
name := 'wlrix-session'

rootdir := ''
prefix := '/usr'

usrdir := absolute_path(clean(rootdir / prefix))
bindir := usrdir / 'bin'
systemddir := usrdir / 'lib' / 'systemd' / 'user'
sessiondir := usrdir / 'share' / 'wayland-sessions'

bin-src := 'target' / 'release' / name
bin-dst := bindir / name

# The script a display manager actually runs. It sets the session's environment up and then
# execs the binary above; see data/start-wlrix.sh.
start-src := 'data' / 'start-wlrix.sh'
start-dst := bindir / 'start-wlrix'

target-src := 'data' / name + '.target'
target-dst := systemddir / name + '.target'

session-src := 'data' / 'wlrix.desktop'
session-dst := sessiondir / 'wlrix.desktop'

# Defaults for other programs' config files, read through XDG_CONFIG_DIRS, which start-wlrix
# points here first. Under share/ rather than /etc/xdg so they apply to the wlRIX session alone.
xdgdir := usrdir / 'share' / 'wlrix' / 'xdg'
fcitx-xcb-src := 'data' / 'xdg' / 'fcitx5' / 'conf' / 'xcb.conf'
fcitx-xcb-dst := xdgdir / 'fcitx5' / 'conf' / 'xcb.conf'

# List available recipes.
default:
  @just --list

release:
  cargo build --release

lint:
  cargo clippy

test:
  cargo test

# Install the session manager, its launcher and the entry a display manager offers.
#
# Deliberately does not build: this is normally run as root, and building as root leaves a
# target directory nobody can write to afterwards.
#
#     just release && sudo just install
[doc("Install the session and its data files (build first; run as root)")]
install:
  #!/usr/bin/env bash
  set -euo pipefail
  if [ ! -x '{{bin-src}}' ]; then
      echo "no release build -- run 'just release' first" >&2
      exit 1
  fi
  install -Dm0755 '{{bin-src}}' '{{bin-dst}}'
  install -Dm0755 '{{start-src}}' '{{start-dst}}'
  install -Dm0644 '{{target-src}}' '{{target-dst}}'
  install -Dm0644 '{{session-src}}' '{{session-dst}}'
  install -Dm0644 '{{fcitx-xcb-src}}' '{{fcitx-xcb-dst}}'
  for f in '{{bin-dst}}' '{{start-dst}}' '{{target-dst}}' '{{session-dst}}' '{{fcitx-xcb-dst}}'; do
      echo "installed $f"
  done
  echo
  echo "The session entry runs start-wlrix, which must be on PATH, as must"
  echo "wlrix-compositor and the desktop's apps -- the session starts them by name."

[doc("Remove what install put down")]
uninstall:
  #!/usr/bin/env bash
  set -euo pipefail
  rm -f '{{bin-dst}}' '{{start-dst}}' '{{target-dst}}' '{{session-dst}}' '{{fcitx-xcb-dst}}'
  rmdir -p --ignore-fail-on-non-empty "$(dirname '{{fcitx-xcb-dst}}')" 2>/dev/null ||:
  echo "removed the session, its launcher, the session entry and its config defaults"

clean:
  cargo clean
