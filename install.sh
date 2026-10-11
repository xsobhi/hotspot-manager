#!/bin/sh
# Install Hotspot Manager. Run as root: sudo ./install.sh
set -e
[ "$(id -u)" = 0 ] || { echo "Run as root: sudo $0" >&2; exit 1; }
cd "$(dirname "$0")"

if command -v apt-get >/dev/null; then
    apt-get install -y hostapd iw dnsmasq-base iptables iproute2 ieee-data qrencode python3-gi gir1.2-gtk-3.0
else
    echo "Not a Debian/Ubuntu system: install hostapd, iw, dnsmasq, iptables, iproute2 (tc), qrencode and PyGObject (GTK 3) yourself." >&2
fi
# The distro's own hostapd unit is not used (and fails without its config file).
systemctl disable --now hostapd.service 2>/dev/null || true

install -m 755 bin/hotspotctl /usr/local/bin/hotspotctl
install -m 755 bin/hotspot-manager /usr/local/bin/hotspot-manager
install -D -m 755 lib/dhcp-event /usr/local/lib/hotspot-manager/dhcp-event
install -m 644 systemd/hotspot-manager.service /etc/systemd/system/hotspot-manager.service
install -m 644 polkit/io.github.xsobhi.hotspot-manager.policy /usr/share/polkit-1/actions/
install -D -m 644 icons/hotspot-manager.svg /usr/local/share/icons/hicolor/scalable/apps/hotspot-manager.svg
install -D -m 644 icons/hotspot-manager-qr-symbolic.svg /usr/local/share/icons/hicolor/scalable/apps/hotspot-manager-qr-symbolic.svg
install -D -m 644 desktop/hotspot-manager.desktop /usr/local/share/applications/hotspot-manager.desktop
install -d -m 755 /etc/hotspot-manager
systemctl daemon-reload
update-desktop-database -q /usr/local/share/applications 2>/dev/null || true

# Desktop shortcut for the user who ran the installer (sudo or pkexec).
user=${SUDO_USER:-}
[ -z "$user" ] && [ -n "${PKEXEC_UID:-}" ] && user=$(id -nu "$PKEXEC_UID")
if [ -n "$user" ] && [ "$user" != root ]; then
    desk=$(runuser -u "$user" -- xdg-user-dir DESKTOP 2>/dev/null || echo "/home/$user/Desktop")
    if [ -d "$desk" ]; then
        install -m 755 -o "$user" -g "$(id -gn "$user")" desktop/hotspot-manager.desktop "$desk/hotspot-manager.desktop"
        runuser -u "$user" -- gio set "$desk/hotspot-manager.desktop" metadata::trusted true 2>/dev/null || true
    fi
fi

echo "Installed. Open \"Hotspot Manager\" from the menu, set a password in Settings and flip the switch."
