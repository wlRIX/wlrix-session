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
  for f in '{{bin-dst}}' '{{start-dst}}' '{{target-dst}}' '{{session-dst}}'; do
      echo "installed $f"
  done
  echo
  echo "The session entry runs start-wlrix, which must be on PATH, as must"
  echo "wlrix-compositor and the desktop's apps -- the session starts them by name."

[doc("Remove what install put down")]
uninstall:
  #!/usr/bin/env bash
  set -euo pipefail
  rm -f '{{bin-dst}}' '{{start-dst}}' '{{target-dst}}' '{{session-dst}}'
  echo "removed the session, its launcher and the session entry"

clean:
  cargo clean
