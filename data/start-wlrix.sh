#!/bin/bash
set -e

if command -v systemctl > /dev/null; then
  # Robustness: if the previous graphical session left some failed units,
  # reset them so that they don't break this startup.
  for unit in $(systemctl --user --no-legend --state=failed --plain list-units | cut -f1 -d' '); do
    partof="$(systemctl --user show -p PartOf --value "$unit")"
    for target in wlrix-session.target graphical-session.target; do
      if [ "$partof" = "$target" ]; then
        systemctl --user reset-failed "$unit"
        break
      fi
    done
  done
fi

# These two keep "only if it is not already set". A display manager sets XDG_CURRENT_DESKTOP
# itself, from the DesktopNames in wlrix.desktop, and overriding either by hand is a legitimate
# thing to want for a toolkit or portal quirk.
export XDG_CURRENT_DESKTOP="${XDG_CURRENT_DESKTOP:=wlRIX}"
export XDG_SESSION_DESKTOP="${XDG_SESSION_DESKTOP:=wlrix}"

# This one is forced, and that is the difference that matters.
#
# It says what the session *is*, and pam_systemd sets it from how the login arrived: a console
# login gets `tty`, and so does a greetd login, which is a console login as far as logind is
# concerned. `${XDG_SESSION_TYPE:=wayland}` does nothing about that -- the parameter is set, just
# set to the wrong thing -- so the session ran with `tty` here, on a TTY and through the greeter
# alike.
#
# It matters because it is the flag toolkits auto-detect on. Chromium and Edge resolve
# `--ozone-platform-hint=auto`, their default, by reading exactly this, and fall back to X11 when
# it does not say `wayland` -- which is how this was found: Edge under XWayland here and native
# Wayland on another desktop, from the same profile with no flags set either way.
#
# Whatever it said before, once the compositor is up this is a Wayland session.
export XDG_SESSION_TYPE=wayland
export _JAVA_AWT_WM_NONREPARENTING=1
export GDK_BACKEND=wayland,x11
# GTK 3 reads its window-button order -- and everything else in the settings portal -- only when
# this is set. Measured: with it, `gtk-decoration-layout` comes back as the
# `menu:minimize,maximize` xdg-desktop-portal-wlrix reports, which is what IRIX's frame has;
# without it GTK 3 keeps its own `menu:close` and ignores settings.ini for that key as well, so
# there is no other way to give a GTK 3 window the right buttons.
#
# GTK 4 needs no such flag -- it reads the portal either way -- and this changes nothing for it.
#
# The rest of what it turns on is portals wlRIX does not implement, which `wlrix-portals.conf`
# sends to the `gtk` backend: the same file chooser and print dialog GTK would have opened
# itself, reached one indirection later.
export GTK_USE_PORTAL=1
# The wlRIX GTK 3 module (wlrix-gtk-config), which does the two things the stylesheets cannot:
# reloads ~/.config/gtk-3.0/gtk.css when the scheme changes -- GTK 3 reads it once at startup and
# never again -- and clears GtkHeaderBar's `has-subtitle`, whose reserved second line makes a
# GTK 3 titlebar 38px against the compositor's 30.
#
# In the environment rather than as `gtk-modules` in settings.ini, which would be the other
# place: GTK merges the two, but that key is a list, and the daemon writing it would have to
# merge with whatever the user already had there. Nothing else in the session sets GTK_MODULES,
# and GTK 2 -- the one other reader of this variable -- is not installed as a dependency of
# anything here. GTK 4 removed module support and ignores it.
export GTK_MODULES=wlrix-gtk-config
export MOZ_ENABLE_WAYLAND=1
export QT_QPA_PLATFORM="wayland;xcb"
export QT_AUTO_SCREEN_SCALE_FACTOR=1
export QT_ENABLE_HIGHDPI_SCALING=1
# Use GTK themes with QT apps, provided by qt5-base or qt6-base
export QT_QPA_PLATFORMTHEME=gtk3

# Start gnome keyring components if the daemon is active
# -> check if /run/user/$UID/keyring exists
if [ -d "/run/user/$(id -u)/keyring" ]; then
  # Use PATH lookup instead of hardcoding /usr/bin
  if command -v gnome-keyring-daemon > /dev/null 2>&1; then
    eval "$(gnome-keyring-daemon --start --components=pkcs11,secrets,ssh > /dev/null 2>&1)"
  else
    echo "gnome-keyring-daemon not found in PATH" >&2
  fi

  # Only set SSH_AUTH_SOCK if the socket actually exists. Either
  # set the correct one, or don't set one at all. Don't set the
  # wrong value.
  if [ -S "/run/user/$(id -u)/gcr/ssh" ]; then
    export SSH_AUTH_SOCK="/run/user/$(id -u)/gcr/ssh"
  elif [ -S "/run/user/$(id -u)/keyring/ssh" ]; then
    export SSH_AUTH_SOCK="/run/user/$(id -u)/keyring/ssh"
  fi
fi

if command -v systemctl > /dev/null; then
  # Import some variables that we explicitly want to have available
  # in the user session.
  systemctl --user import-environment XDG_SESSION_TYPE XDG_CURRENT_DESKTOP SSH_AUTH_SOCK

  # For environment variables already imported into the user's
  # session, if the value imported differs from teh value in this
  # environment, update it.
  mapfile -t existing_env_vars < <(systemctl --user show-environment)
  for env_var in "${existing_env_vars[@]}"; do
    env_var_name="${env_var%%=*}"
    env_var_value=${!env_var_name:-}

    # Skip current iteration if the environment variable's value
    # in the current environment is unset.
    if [[ -z "${env_var_value}" ]]; then
      continue;
    fi

    env_var_val_str_to_compare="${env_var_name}=${env_var_value}"
    env_var_val_str_to_compare_ansi_c_quoted="${env_var_name}=\$'$(printf '%q' "${env_var_value}")'"
    if [[ "${env_var}" == "${env_var_val_str_to_compare}" ]]; then
      continue
    elif [[ "${env_var}" == "${env_var_val_str_to_compare_ansi_c_quoted}" ]]; then
      continue
    fi
    systemctl --user import-environment "${env_var_name}" ||:
  done
fi

# Run wlrix-session.
#
# Prefer the user bus that is already there. On a systemd system the user manager runs one at
# $XDG_RUNTIME_DIR/bus whether or not DBUS_SESSION_BUS_ADDRESS happens to be exported into this
# environment, and starting a *private* bus alongside it cuts the session off from the one
# everything else uses: `dbus-update-activation-environment` then updates a bus no portal will
# ever be activated on, and D-Bus services the session starts cannot be reached from outside it.
#
# dbus-run-session stays as the fallback for a machine with no user bus at all.
if [[ -z "${DBUS_SESSION_BUS_ADDRESS:-}" && -S "${XDG_RUNTIME_DIR:-}/bus" ]]; then
  export DBUS_SESSION_BUS_ADDRESS="unix:path=${XDG_RUNTIME_DIR}/bus"
fi

if [[ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]]; then
  exec dbus-run-session -- wlrix-session
else
  exec wlrix-session
fi
