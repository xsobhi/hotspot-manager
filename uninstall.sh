#!/bin/sh
# Remove Hotspot Manager. Run as root: sudo ./uninstall.sh   (add --purge to also delete settings)
set -e
[ "$(id -u)" = 0 ] || { echo "Run as root: sudo $0" >&2; exit 1; }
systemctl disable --now hotspot-manager.service 2>/dev/null || true
rm -f /usr/local/bin/hotspotctl /usr/local/bin/hotspot-manager \
      /usr/local/lib/hotspot-manager/dhcp-event \
      /etc/systemd/system/hotspot-manager.service \
      /usr/share/polkit-1/actions/io.github.xsobhi.hotspot-manager.policy \
      /usr/local/share/applications/hotspot-manager.desktop \
      /usr/local/share/icons/hicolor/scalable/apps/hotspot-manager.svg \
      /usr/local/share/icons/hicolor/scalable/apps/hotspot-manager-qr-symbolic.svg
rmdir /usr/local/lib/hotspot-manager 2>/dev/null || true
[ "$1" = --purge ] && rm -rf /etc/hotspot-manager
systemctl daemon-reload
echo "Removed."
