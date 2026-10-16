#!/usr/bin/env bash
# Install swarm2linux for the current user: stealthctl, the background service stealthd (started with the graphical
# session) and the desktop app stealth-gui with its menu entry. It also installs what the build needs (asking
# first) and the udev rule that lets the logged-in user open the headset.
#
#   ./install.sh               install, or update after `git pull`
#   ./install.sh --uninstall   remove what this script installed
#
# Options: --yes (do not ask before installing packages or Rust), --no-udev (leave the udev rule alone).
set -euo pipefail
cd "$(dirname "$0")"

RUST_MIN=1.92
CONFIG=${XDG_CONFIG_HOME:-$HOME/.config}
DATA=${XDG_DATA_HOME:-$HOME/.local/share}
BIN=$HOME/.local/bin
APP_ID=io.github.swarm2linux
DESKTOP=$DATA/applications/$APP_ID.desktop
ICON=$DATA/icons/hicolor/scalable/apps/$APP_ID.svg
UNIT=$CONFIG/systemd/user/stealthd.service
AUTOSTART=$CONFIG/autostart/$APP_ID.desktop
RULE=/etc/udev/rules.d/70-swarm2linux.rules
PROGRAMS=(stealthctl stealthd stealth-gui)

yes=false
udev=true
uninstall=false
for arg in "$@"; do
    case $arg in
        -y | --yes) yes=true ;;
        --no-udev) udev=false ;;
        --uninstall) uninstall=true ;;
        -h | --help)
            sed -n '2,9s/^# \{0,1\}//p' "$0"
            exit 0
            ;;
        *)
            echo "install.sh: unknown option $arg (see --help)" >&2
            exit 2
            ;;
    esac
done

step() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }
note() { printf '    %s\n' "$*"; }
warn() { printf '\033[33m    warning: %s\033[0m\n' "$*" >&2; }
die() {
    printf '\033[31minstall.sh: %s\033[0m\n' "$*" >&2
    exit 1
}

# ask "question": true for yes; --yes answers yes, and without a terminal the answer is no
ask() {
    $yes && return 0
    [ -t 0 ] || return 1
    local answer
    read -r -p "    $1 [Y/n] " answer
    case $answer in "" | [Yy]*) return 0 ;; *) return 1 ;; esac
}

as_root() {
    if [ "$(id -u)" = 0 ]; then "$@"; else sudo "$@"; fi
}

have() { command -v "$1" >/dev/null 2>&1; }

user_systemd() { have systemctl && systemctl --user show-environment >/dev/null 2>&1; }

# version_at_least 1.99.0 1.92
version_at_least() { [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -n1)" = "$2" ]; }

rust_ok() {
    have cargo && have rustc || return 1
    local v
    v=$(rustc --version | awk '{print $2}')
    version_at_least "${v%%-*}" "$RUST_MIN"
}

# the install command for the build tools on this system, or nothing
package_command() {
    local extra=$1
    if have dnf; then
        echo "dnf install -y gcc pkgconf-pkg-config fontconfig-devel $extra"
    elif have apt-get; then
        echo "apt-get install -y build-essential pkg-config libfontconfig-dev ${extra:+$extra ca-certificates}"
    elif have pacman; then
        echo "pacman -S --needed --noconfirm base-devel fontconfig $extra"
    elif have zypper; then
        echo "zypper --non-interactive install gcc pkg-config fontconfig-devel $extra"
    fi
}

# replace a file in one step, so a running program keeps its old copy
put() {
    local mode=$1 from=$2 to=$3
    mkdir -p "$(dirname "$to")"
    install -m "$mode" "$from" "$to.new"
    mv -f "$to.new" "$to"
}

stop_hand_started_stealthd() {
    if ! systemctl --user is-active --quiet stealthd.service && pgrep -x -u "$(id -u)" stealthd >/dev/null; then
        note "Stopping the stealthd that was started by hand."
        pkill -x -u "$(id -u)" stealthd || true
        for _ in $(seq 50); do
            pgrep -x -u "$(id -u)" stealthd >/dev/null || break
            sleep 0.1
        done
    fi
}

install_all() {
    step "Checking what the build needs"
    # shellcheck source=/dev/null
    [ -f "${CARGO_HOME:-$HOME/.cargo}/env" ] && . "${CARGO_HOME:-$HOME/.cargo}/env"
    local need_rust=false extra="" missing=()
    rust_ok || need_rust=true
    have cc || missing+=("a C compiler")
    have pkg-config || missing+=(pkg-config)
    have pkg-config && pkg-config --exists fontconfig || missing+=("the fontconfig headers")
    if $need_rust && ! have rustup && ! have curl; then
        missing+=(curl)
        extra=curl
    fi
    if [ ${#missing[@]} -gt 0 ]; then
        local cmd
        cmd=$(package_command "$extra")
        [ -n "$cmd" ] || die "missing ${missing[*]}. Install them with your package manager and run this again."
        local list
        list=$(printf '%s, ' "${missing[@]}")
        note "Missing: ${list%, }."
        ask "Install them with: sudo $cmd ?" || die "not installing packages. Run: sudo $cmd"
        if have apt-get; then as_root apt-get update; fi
        # shellcheck disable=SC2086 # the command is split into words on purpose
        as_root $cmd
    else
        note "Compiler, pkg-config and fontconfig: found."
    fi

    if $need_rust; then
        if have rustup; then
            note "Rust $RUST_MIN or newer is needed."
            ask "Update Rust with: rustup update stable ?" || die "Rust is too old. Run: rustup update stable"
            rustup update stable
        else
            note "Rust $RUST_MIN or newer is needed. rustup installs it into ~/.cargo for your user."
            ask "Install Rust with rustup (https://rustup.rs) now?" ||
                die "no Rust. Install it from https://rustup.rs and run this again."
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
            # shellcheck source=/dev/null
            . "${CARGO_HOME:-$HOME/.cargo}/env"
        fi
        rust_ok || die "still no Rust $RUST_MIN or newer (found: $(rustc --version 2>/dev/null || echo none))."
    fi
    note "$(rustc --version)"

    if $udev; then
        step "Headset access (udev rule)"
        if cmp -s packaging/70-swarm2linux.rules "$RULE"; then
            note "Already installed: $RULE"
        else
            note "The rule lets the logged-in user open the headset. Installing it needs root."
            if ! ask "Install $RULE with sudo?"; then
                warn "skipped: without the rule your user may not be able to open the headset."
            else
                as_root install -Dm644 packaging/70-swarm2linux.rules "$RULE"
                if as_root udevadm control --reload &&
                    as_root udevadm trigger --subsystem-match=hidraw --action=change; then
                    note "Installed: $RULE"
                else
                    warn "installed $RULE, but udev did not reload; unplug the transmitter and plug it in again."
                fi
            fi
        fi
    fi

    step "Building (this takes a few minutes the first time)"
    cargo build --release --locked

    step "Installing"
    case $BIN in *[\"\`\$\\]*) die "cannot name $BIN in a desktop entry" ;; esac
    for p in "${PROGRAMS[@]}"; do
        put 755 "target/release/$p" "$BIN/$p"
    done
    # the menu entry names the program by its full path: Plasma's PATH usually lacks ~/.local/bin
    sed "s|^Exec=stealth-gui|Exec=\"$BIN/stealth-gui\"|" "packaging/$APP_ID.desktop" >"target/$APP_ID.desktop"
    put 644 "target/$APP_ID.desktop" "$DESKTOP"
    put 644 "packaging/icons/$APP_ID.svg" "$ICON"
    put 644 packaging/stealthd.service "$UNIT"
    # an autostart entry made by a stealth-gui started from elsewhere (such as the build folder) now starts this one
    if [ -f "$AUTOSTART" ]; then
        sed -i "s|^Exec=.*|Exec=\"$BIN/stealth-gui\" --hidden|" "$AUTOSTART"
    fi
    have update-desktop-database && update-desktop-database -q "$DATA/applications" || true
    have kbuildsycoca6 && kbuildsycoca6 >/dev/null 2>&1 || true
    note "Programs in $BIN, the menu entry \"swarm2linux\", and the service stealthd."

    step "Starting stealthd"
    if user_systemd; then
        systemctl --user daemon-reload
        stop_hand_started_stealthd
        systemctl --user enable --quiet stealthd.service
        systemctl --user restart stealthd.service
        if systemctl --user is-active --quiet stealthd.service; then
            note "Running, and it starts with your graphical session from now on."
        else
            warn "stealthd did not start. See: journalctl --user -u stealthd"
        fi
    else
        warn "no systemd user session found. Start $BIN/stealthd yourself when you log in."
    fi

    step "Done"
    note "Open \"swarm2linux\" from the app menu, or run stealth-gui."
    note "For the tray icon after login, turn on \"Start with the session\" on the app's Device page."
    if pgrep -x -u "$(id -u)" stealth-gui >/dev/null; then
        note "stealth-gui is running already: quit it from its tray menu and start it again to use this version."
    fi
    case ":$PATH:" in
        *":$BIN:"*) ;;
        *) note "Add $BIN to your PATH to run stealthctl and stealth-gui from a terminal." ;;
    esac
}

uninstall_all() {
    step "Removing swarm2linux"
    if user_systemd; then
        systemctl --user disable --now --quiet stealthd.service 2>/dev/null || true
    fi
    pkill -x -u "$(id -u)" stealth-gui 2>/dev/null || true
    pkill -x -u "$(id -u)" stealthd 2>/dev/null || true
    for p in "${PROGRAMS[@]}"; do
        rm -f "$BIN/$p"
    done
    rm -f "$DESKTOP" "$ICON" "$UNIT" "$AUTOSTART"
    if user_systemd; then
        systemctl --user daemon-reload
    fi
    have update-desktop-database && update-desktop-database -q "$DATA/applications" || true
    have kbuildsycoca6 && kbuildsycoca6 >/dev/null 2>&1 || true
    note "Removed the programs, the menu entry, the service and the autostart entry."

    if $udev && [ -e "$RULE" ]; then
        note "Removing the udev rule needs root."
        as_root rm -f "$RULE"
        as_root udevadm control --reload || true
        note "Removed $RULE"
    fi

    step "Done"
    note "Kept: your app preferences in $CONFIG/swarm2linux (delete the folder to remove them), and Rust."
    note "The headset keeps its settings: they are stored on the headset."
}

[ "$(id -u)" != 0 ] || die "run this as your normal user; it asks for root (sudo) when it needs it."
if $uninstall; then uninstall_all; else install_all; fi
