#!/bin/sh
# Install Hotspot Manager. Run as root: sudo ./install.sh
set -e
[ "$(id -u)" = 0 ] || { echo "Run as root: sudo $0" >&2; exit 1; }
cd "$(dirname "$0")"

if command -v apt-get >/dev/null; then
    apt-get install -y hostapd iw dnsmasq-base iptables qrencode python3-gi gir1.2-gtk-3.0
else
    echo "Not a Debian/Ubuntu system: install hostapd, iw, dnsmasq, iptables, qrencode and PyGObject (GTK 3) yourself." >&2
fi
# The distro's own hostapd unit is not used (and fails without its config file).
systemctl disable --now hostapd.service 2>/dev/null || true

install -m 755 bin/hotspotctl /usr/local/bin/hotspotctl
install -m 755 bin/hotspot-manager /usr/local/bin/hotspot-manager
install -m 644 systemd/hotspot-manager.service /etc/systemd/system/hotspot-manager.service
install -m 644 polkit/io.github.xsobhi.hotspot-manager.policy /usr/share/polkit-1/actions/
install -D -m 644 desktop/hotspot-manager.desktop /usr/local/share/applications/hotspot-manager.desktop
install -d -m 755 /etc/hotspot-manager
systemctl daemon-reload

echo "Installed. Open \"Hotspot Manager\" from the menu, set a password in Settings and flip the switch."
