#!/bin/sh
# Install stealthctl and stealthd for the current user and start stealthd with the graphical session.
# The udev rule (packaging/70-swarm2linux.rules) is only needed if /dev/hidraw* of the headset is not
# accessible to your user; installing it needs root.
set -eu
cd "$(dirname "$0")/.."
cargo build --release
install -Dm755 target/release/stealthctl "$HOME/.local/bin/stealthctl"
install -Dm755 target/release/stealthd "$HOME/.local/bin/stealthd"
install -Dm644 packaging/stealthd.service "$HOME/.config/systemd/user/stealthd.service"
systemctl --user daemon-reload
systemctl --user enable --now stealthd.service
systemctl --user --no-pager status stealthd.service | head -5
