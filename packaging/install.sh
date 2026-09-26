#!/usr/bin/env bash
# One-time setup: allow the current user to read /dev/input and write /dev/uinput without root.
set -e
sudo install -m644 "$(dirname "$0")/99-lintas.rules" /etc/udev/rules.d/99-lintas.rules
echo uinput | sudo tee /etc/modules-load.d/lintas.conf >/dev/null
sudo modprobe uinput
sudo usermod -aG input "$USER"
sudo udevadm control --reload-rules && sudo udevadm trigger
echo "Done. Log out and back in (or reboot) so the 'input' group takes effect."
