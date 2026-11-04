#!/usr/bin/env bash
# Puts the hocket toolchain on PATH without relying on a login shell.
#
#   . scripts/dev-env.sh            # source into the current bash shell
#   scripts/dev-env.sh <cmd> [args] # run one command with the env set
#
# Every entry is only added when the directory exists, so this is a no-op on machines (and CI)
# that lay things out differently. Values already in the environment win over the guesses below.

_hocket_path_prepend() {
  [ -d "$1" ] || return 0
  case ":$PATH:" in *":$1:"*) ;; *) PATH="$1:$PATH" ;; esac
}

# Android SDK: the host keeps it in /opt/android-sdk, deskhand's android-dev template in ~/Android/Sdk.
if [ -z "${ANDROID_HOME:-}" ]; then
  for _d in "${ANDROID_SDK_ROOT:-}" /opt/android-sdk "$HOME/Android/Sdk"; do
    if [ -n "$_d" ] && [ -d "$_d" ]; then ANDROID_HOME="$_d"; break; fi
  done
  unset _d
fi
if [ -n "${ANDROID_HOME:-}" ]; then
  export ANDROID_HOME
  export ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$ANDROID_HOME}"
  _hocket_path_prepend "$ANDROID_HOME/cmdline-tools/latest/bin"
  _hocket_path_prepend "$ANDROID_HOME/emulator"
  _hocket_path_prepend "$ANDROID_HOME/platform-tools"
fi

# JDK unpacked in the home directory (deskhand android-dev); the host uses the system JDK.
if [ -z "${JAVA_HOME:-}" ] && [ -x "$HOME/opt/jdk/bin/java" ]; then
  export JAVA_HOME="$HOME/opt/jdk"
fi
[ -n "${JAVA_HOME:-}" ] && _hocket_path_prepend "$JAVA_HOME/bin"

# Rust (rustup), user-level Node, and ~/.local/bin (the pnpm 10 shim) / ~/bin (deskhand helpers).
_hocket_path_prepend "$HOME/bin"
_hocket_path_prepend "$HOME/.local/bin"
_hocket_path_prepend "$HOME/.local/opt/node/bin"
_hocket_path_prepend "${CARGO_HOME:-$HOME/.cargo}/bin"
export PATH

# ALSA headers extracted at user level (no system libasound2-dev). cpal -> alsa-sys needs them to
# build hocket-core, so every cargo build of the workspace and `pnpm gen` in desktop/ uses this.
_hocket_alsa_pc="$HOME/.local/opt/libasound2-dev/usr/lib/x86_64-linux-gnu/pkgconfig"
if [ -d "$_hocket_alsa_pc" ]; then
  case ":${PKG_CONFIG_PATH:-}:" in
    *":$_hocket_alsa_pc:"*) ;;
    *) export PKG_CONFIG_PATH="$_hocket_alsa_pc${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}" ;;
  esac
fi
unset _hocket_alsa_pc
unset -f _hocket_path_prepend

# Wrapper mode: run the given command. When sourced, `return` succeeds and this line is skipped.
if ! (return 0 2>/dev/null); then
  if [ "$#" -eq 0 ]; then
    echo "usage: . scripts/dev-env.sh   or   scripts/dev-env.sh <cmd> [args...]" >&2
    exit 2
  fi
  exec "$@"
fi
