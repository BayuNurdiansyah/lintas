#!/usr/bin/env bash
# One-time setup: allow the current user to read /dev/input and write /dev/uinput without root.
set -e
sudo install -m644 "$(dirname "$0")/99-lintas.rules" /etc/udev/rules.d/99-lintas.rules
echo uinput | sudo tee /etc/modules-load.d/lintas.conf >/dev/null
sudo modprobe uinput
sudo usermod -aG input "$USER"
sudo udevadm control --reload-rules && sudo udevadm trigger
echo "Done. Log out and back in (or reboot) so the 'input' group takes effect."

# lintas needs TCP 4242 (the serve<->host connection) and UDP 5353 (mDNS
# discovery) reachable. Only touches the firewall if one is actually active,
# and only adds these two specific rules, tagged so they're easy to spot or
# remove later (e.g. `sudo ufw status numbered`, then `sudo ufw delete <n>`).
if command -v ufw >/dev/null && sudo ufw status | grep -q "Status: active"; then
    echo "Detected an active ufw firewall: opening TCP 4242 and UDP 5353 for lintas."
    sudo ufw allow 4242/tcp comment lintas
    sudo ufw allow 5353/udp comment lintas
elif command -v firewall-cmd >/dev/null && sudo firewall-cmd --state >/dev/null 2>&1; then
    echo "Detected an active firewalld: opening TCP 4242 and UDP 5353 for lintas."
    sudo firewall-cmd --permanent --add-port=4242/tcp
    sudo firewall-cmd --permanent --add-port=5353/udp
    sudo firewall-cmd --reload
else
    echo "No active ufw/firewalld detected: skipping firewall changes."
    echo "If you do use a firewall, open TCP 4242 (and UDP 5353 for mDNS discovery) manually."
fi
