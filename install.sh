#!/bin/sh
# runnir — installer / updater / uninstaller.
#
# Single source of truth for getting runnir onto a machine and off it again.
# POSIX sh only (runs under dash and macOS /bin/sh) — no bashisms, no arrays.
#
# Usage:
#   install.sh [install]     build from source and install (default)
#   install.sh update        fetch latest, rebuild, reinstall
#   install.sh uninstall     remove everything the installer added
#   install.sh --help        show help
#
# One-liner install:
#   curl -fsSL https://raw.githubusercontent.com/drheavymetal/Runnar/main/install.sh | sh
#
# Environment overrides:
#   PREFIX      install prefix for the binary (default: $HOME/.local)
#               the binary lands in $PREFIX/bin/runnir
#   XDG_DATA_HOME, XDG_CONFIG_HOME honoured as usual.

set -eu

# --- constants ---------------------------------------------------------------

REPO_URL="https://github.com/drheavymetal/Runnar.git"
RUSTUP_URL="https://sh.rustup.rs"

DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"

DATA_DIR="$DATA_HOME/runnir"          # our private data dir (cache + self copy)
SRC_DIR="$DATA_DIR/src"               # the git checkout we build from
SELF_COPY="$DATA_DIR/install.sh"      # copy of this script the helpers re-exec

# Record whether the caller set PREFIX *before* we default it, so the root
# guard can tell an explicit system install from an accidental one.
if [ -n "${PREFIX+x}" ]; then
	PREFIX_SET="1"
fi
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"
BIN="$BIN_DIR/runnir"
HELPER_UPDATE="$BIN_DIR/runnir-update"
HELPER_UNINSTALL="$BIN_DIR/runnir-uninstall"

APPS_DIR="$DATA_HOME/applications"
ICONS_DIR="$DATA_HOME/icons/hicolor/256x256/apps"
DESKTOP_FILE="$APPS_DIR/runnir.desktop"
ICON_FILE="$ICONS_DIR/runnir.png"

# --- pretty output -----------------------------------------------------------

if [ -t 1 ]; then
	C_BOLD=$(printf '\033[1m')
	C_GREEN=$(printf '\033[32m')
	C_YELLOW=$(printf '\033[33m')
	C_RED=$(printf '\033[31m')
	C_DIM=$(printf '\033[2m')
	C_OFF=$(printf '\033[0m')
else
	C_BOLD='' C_GREEN='' C_YELLOW='' C_RED='' C_DIM='' C_OFF=''
fi

say()  { printf '%s\n' "$*"; }
info() { printf '%s==>%s %s\n' "$C_GREEN" "$C_OFF" "$*"; }
step() { printf '%s  -%s %s\n' "$C_DIM" "$C_OFF" "$*"; }
warn() { printf '%swarning:%s %s\n' "$C_YELLOW" "$C_OFF" "$*" >&2; }
err()  { printf '%serror:%s %s\n' "$C_RED" "$C_OFF" "$*" >&2; }
die()  { err "$*"; exit 1; }

# Is there a human at the keyboard we can ask questions? When curl|sh pipes the
# script into stdin, stdin is the script, not a tty — so this is false and we
# never block waiting on a read that can't be answered.
is_interactive() {
	[ -t 0 ] && [ -t 1 ]
}

# ask <prompt> — returns 0 for yes, 1 for no. Non-interactive => no.
ask() {
	if ! is_interactive; then
		return 1
	fi
	printf '%s [y/N] ' "$1"
	read -r _ans || return 1
	case "$_ans" in
		y | Y | yes | YES | Yes) return 0 ;;
		*) return 1 ;;
	esac
}

# --- environment checks ------------------------------------------------------

detect_platform() {
	OS=$(uname -s)
	ARCH=$(uname -m)
	case "$OS" in
		Linux)  PLATFORM="linux" ;;
		Darwin) PLATFORM="macos" ;;
		*) die "unsupported OS '$OS' — runnir installs on Linux and macOS only." ;;
	esac
	step "platform: $PLATFORM ($ARCH)"
}

# Guard against accidental system-wide installs: refuse root unless the caller
# clearly meant it by setting PREFIX explicitly.
check_not_root() {
	_uid=$(id -u 2>/dev/null || echo 1000)
	if [ "$_uid" -eq 0 ] && [ -z "${PREFIX_SET:-}" ]; then
		die "refusing to run as root without an explicit PREFIX.
  Running as your normal user installs to \$HOME/.local (no sudo needed).
  If a system-wide install is really intended, re-run with e.g. PREFIX=/usr/local."
	fi
}

need_cmd() {
	command -v "$1" >/dev/null 2>&1
}

ensure_git() {
	need_cmd git || die "git is required but was not found. Install git and re-run."
}

ensure_cargo() {
	if need_cmd cargo; then
		step "cargo: $(command -v cargo)"
		return 0
	fi
	# rustup may be installed but not yet on PATH in this shell.
	if [ -f "$HOME/.cargo/env" ]; then
		# shellcheck disable=SC1091
		. "$HOME/.cargo/env"
		if need_cmd cargo; then
			step "cargo: $(command -v cargo) (from ~/.cargo/env)"
			return 0
		fi
	fi

	warn "no Rust toolchain found (cargo is missing)."
	if is_interactive; then
		say "runnir builds from source and needs the Rust toolchain (rustup + cargo)."
		if ask "Install rustup now via the official installer ($RUSTUP_URL)?"; then
			need_cmd curl || die "curl is required to install rustup. Install curl (or install rustup manually) and re-run."
			info "installing rustup (official installer)…"
			curl --proto '=https' --tlsv1.2 -fsSL "$RUSTUP_URL" | sh -s -- -y
			# shellcheck disable=SC1091
			. "$HOME/.cargo/env"
			need_cmd cargo || die "rustup installed but cargo still not on PATH. Open a new shell and re-run."
			step "cargo: $(command -v cargo)"
			return 0
		fi
	fi

	die "Rust toolchain required. Install it with:
    curl --proto '=https' --tlsv1.2 -fsSL $RUSTUP_URL | sh
  then open a new shell (or run '. \"\$HOME/.cargo/env\"') and re-run this installer."
}

# --- source management -------------------------------------------------------

fetch_source() {
	mkdir -p "$DATA_DIR"
	if [ -d "$SRC_DIR/.git" ]; then
		info "updating source in $SRC_DIR"
		git -C "$SRC_DIR" fetch --depth 1 origin
		# Default branch of origin (main, master, …).
		_branch=$(git -C "$SRC_DIR" remote show origin 2>/dev/null \
			| sed -n 's/.*HEAD branch: //p')
		[ -n "$_branch" ] || _branch="main"
		git -C "$SRC_DIR" checkout -q "$_branch" 2>/dev/null || true
		git -C "$SRC_DIR" reset --hard "origin/$_branch"
	else
		info "cloning $REPO_URL"
		# Clean out a stale non-git dir if one is somehow there.
		if [ -e "$SRC_DIR" ] && [ ! -d "$SRC_DIR/.git" ]; then
			rm -rf "$SRC_DIR"
		fi
		git clone --depth 1 "$REPO_URL" "$SRC_DIR"
	fi
}

current_commit() {
	git -C "$SRC_DIR" rev-parse --short HEAD 2>/dev/null || echo "unknown"
}

# --- build & install ---------------------------------------------------------

build_release() {
	info "building runnir (cargo build --release) — this can take a few minutes"
	( cd "$SRC_DIR" && cargo build --release )
	[ -f "$SRC_DIR/target/release/runnir" ] \
		|| die "build finished but $SRC_DIR/target/release/runnir is missing."
}

install_binary() {
	mkdir -p "$BIN_DIR"
	install -m 0755 "$SRC_DIR/target/release/runnir" "$BIN" 2>/dev/null \
		|| { cp "$SRC_DIR/target/release/runnir" "$BIN" && chmod 0755 "$BIN"; }
	step "binary -> $BIN"
}

install_self_copy() {
	mkdir -p "$DATA_DIR"
	# The freshly cloned/updated checkout always carries install.sh at its root.
	if [ -f "$SRC_DIR/install.sh" ]; then
		cp "$SRC_DIR/install.sh" "$SELF_COPY"
	elif [ -f "$0" ] && [ "$0" != "sh" ]; then
		cp "$0" "$SELF_COPY" 2>/dev/null || true
	fi
	[ -f "$SELF_COPY" ] && chmod 0644 "$SELF_COPY"
}

install_helpers() {
	mkdir -p "$BIN_DIR"

	cat > "$HELPER_UPDATE" <<EOF
#!/bin/sh
# runnir updater — regenerated by install.sh; edits will be overwritten.
exec sh "$SELF_COPY" update "\$@"
EOF
	chmod 0755 "$HELPER_UPDATE"
	step "helper -> $HELPER_UPDATE"

	cat > "$HELPER_UNINSTALL" <<EOF
#!/bin/sh
# runnir uninstaller — regenerated by install.sh; edits will be overwritten.
exec sh "$SELF_COPY" uninstall "\$@"
EOF
	chmod 0755 "$HELPER_UNINSTALL"
	step "helper -> $HELPER_UNINSTALL"
}

install_desktop() {
	if [ "$PLATFORM" != "linux" ]; then
		step "desktop entry: skipped (not Linux)"
		return 0
	fi
	mkdir -p "$APPS_DIR" "$ICONS_DIR"

	if [ -f "$SRC_DIR/assets/icon.png" ]; then
		cp "$SRC_DIR/assets/icon.png" "$ICON_FILE"
	elif [ -f "$SRC_DIR/assets/logo.png" ]; then
		cp "$SRC_DIR/assets/logo.png" "$ICON_FILE"
	fi

	# Point Exec at the absolute binary so launchers work regardless of PATH,
	# and Icon at our installed file.
	cat > "$DESKTOP_FILE" <<EOF
[Desktop Entry]
Type=Application
Name=runnir
GenericName=Terminal
Comment=A GPU-accelerated terminal emulator
Exec=$BIN
Icon=$ICON_FILE
Terminal=false
Categories=System;TerminalEmulator;
Keywords=terminal;shell;runnir;
StartupWMClass=runnir
EOF
	step "desktop -> $DESKTOP_FILE"
	step "icon    -> $ICON_FILE"
	if need_cmd update-desktop-database; then
		update-desktop-database "$APPS_DIR" >/dev/null 2>&1 || true
	fi
}

# --- PATH advice -------------------------------------------------------------

warn_path() {
	case ":$PATH:" in
		*":$BIN_DIR:"*) return 0 ;;
	esac
	warn "$BIN_DIR is not on your PATH."
	say  "  Add it so you can run 'runnir':"
	say  "    fish:  fish_add_path $BIN_DIR"
	say  "    bash/zsh/sh:  export PATH=\"$BIN_DIR:\$PATH\"   (add to your shell rc)"
}

# --- flows -------------------------------------------------------------------

do_install() {
	info "installing runnir"
	detect_platform
	check_not_root
	ensure_git
	ensure_cargo
	fetch_source
	_commit=$(current_commit)
	build_release
	install_binary
	install_self_copy
	install_helpers
	install_desktop
	say ""
	info "${C_BOLD}runnir installed${C_OFF} (commit $_commit)"
	say  "  binary:     $BIN"
	say  "  update:     runnir-update"
	say  "  uninstall:  runnir-uninstall"
	warn_path
	say ""
	say  "Run ${C_BOLD}runnir${C_OFF} to start."
}

do_update() {
	info "updating runnir"
	detect_platform
	check_not_root
	ensure_git
	ensure_cargo
	if [ ! -d "$SRC_DIR/.git" ]; then
		warn "no existing source checkout at $SRC_DIR — doing a fresh install instead."
		do_install
		return
	fi
	_old=$(current_commit)
	_old_ver=$("$BIN" --version 2>/dev/null || echo "runnir (unknown)")
	fetch_source
	_new=$(current_commit)
	if [ "$_old" = "$_new" ]; then
		info "already up to date (commit $_new)"
	fi
	build_release
	install_binary
	install_self_copy
	install_helpers
	install_desktop
	_new_ver=$("$BIN" --version 2>/dev/null || echo "runnir (unknown)")
	say ""
	info "${C_BOLD}runnir updated${C_OFF}"
	say  "  commit:  $_old -> $_new"
	say  "  version: $_old_ver -> $_new_ver"
	say  "  config left untouched: $CONFIG_HOME/runnir"
}

# rm helper that refuses to act on an empty path — belt and braces on top of set -u.
safe_rm() {
	# $1 = path, $2 = human label
	_p="${1:-}"
	if [ -z "$_p" ]; then
		warn "internal: refusing to remove an empty path (${2:-?})"
		return 0
	fi
	if [ -e "$_p" ] || [ -L "$_p" ]; then
		rm -f "$_p"
		step "removed $_p"
	fi
}

do_uninstall() {
	info "uninstalling runnir"
	detect_platform

	_purge=""
	# Accept --purge in either position after the subcommand.
	for _a in "$@"; do
		[ "$_a" = "--purge" ] && _purge="1"
	done

	safe_rm "$BIN" "binary"
	safe_rm "$HELPER_UPDATE" "update helper"
	safe_rm "$HELPER_UNINSTALL" "uninstall helper"
	if [ "$PLATFORM" = "linux" ]; then
		safe_rm "$DESKTOP_FILE" "desktop entry"
		safe_rm "$ICON_FILE" "icon"
		if need_cmd update-desktop-database; then
			update-desktop-database "$APPS_DIR" >/dev/null 2>&1 || true
		fi
	fi

	# Cache / source dir: ask before removing (it can be large; also holds the
	# self copy this very script may be running from).
	if [ -d "$DATA_DIR" ]; then
		_do_data=""
		if [ -n "$_purge" ]; then
			_do_data="1"
		elif ask "Remove the cached source and build dir at $DATA_DIR?"; then
			_do_data="1"
		fi
		if [ -n "$_do_data" ]; then
			# DATA_DIR is a fixed, non-empty path we constructed; guarded above.
			rm -rf "$DATA_DIR"
			step "removed $DATA_DIR"
		else
			step "kept $DATA_DIR"
		fi
	fi

	# User config: keep by default, remove only with --purge (or explicit yes).
	_cfg="$CONFIG_HOME/runnir"
	if [ -d "$_cfg" ]; then
		_do_cfg=""
		if [ -n "$_purge" ]; then
			_do_cfg="1"
		elif ask "Also remove your runnir config at $_cfg? (kept by default)"; then
			_do_cfg="1"
		fi
		if [ -n "$_do_cfg" ]; then
			rm -rf "$_cfg"
			step "removed $_cfg"
		else
			step "kept config $_cfg"
		fi
	fi

	say ""
	info "runnir uninstalled."
}

usage() {
	cat <<EOF
${C_BOLD}runnir installer${C_OFF}

Usage:
  install.sh [install]     build from source and install (default)
  install.sh update        fetch latest, rebuild, reinstall
  install.sh uninstall     remove what the installer added (config kept)
  install.sh uninstall --purge   also remove cache/source and config
  install.sh --help        show this help

One-liner install:
  curl -fsSL https://raw.githubusercontent.com/drheavymetal/Runnar/main/install.sh | sh

Environment:
  PREFIX   install prefix (default: \$HOME/.local); binary at \$PREFIX/bin/runnir

Paths (with current environment):
  binary:     $BIN
  source:     $SRC_DIR
  helpers:    $HELPER_UPDATE
              $HELPER_UNINSTALL
  desktop:    $DESKTOP_FILE   (Linux only)
  config:     $CONFIG_HOME/runnir   (never touched unless --purge)
EOF
}

# --- entrypoint --------------------------------------------------------------

CMD="${1:-install}"
case "$CMD" in
	install) shift 2>/dev/null || true; do_install ;;
	update | up) shift 2>/dev/null || true; do_update ;;
	uninstall | remove) shift 2>/dev/null || true; do_uninstall "$@" ;;
	-h | --help | help) usage ;;
	-v | --version) say "runnir install.sh"; ;;
	*) err "unknown command: $CMD"; usage; exit 2 ;;
esac
