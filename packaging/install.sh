#!/usr/bin/env bash
# Setup sekali jalan: akses /dev/input dan /dev/uinput tanpa root
set -e
sudo install -m644 "$(dirname "$0")/99-lintas.rules" /etc/udev/rules.d/99-lintas.rules
echo uinput | sudo tee /etc/modules-load.d/lintas.conf >/dev/null
sudo modprobe uinput
sudo usermod -aG input "$USER"
sudo udevadm control --reload-rules && sudo udevadm trigger
echo "Selesai. Logout lalu login lagi supaya grup 'input' aktif."
