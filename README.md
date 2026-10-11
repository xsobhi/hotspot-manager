# hotspot-manager-linux

A desktop app for running a **fast 5 GHz Wi-Fi hotspot** on Linux from a second Wi-Fi adapter
(for example a USB dongle), with full control from one window:

- **On/off switch**, and an option to turn it on at startup
- **Name, password, band (5 / 2.4 GHz), channel, channel width (20 / 40 / 80 MHz)**, country,
  hidden network, max devices, and isolating devices from each other
- **Connected devices**: name, IP, MAC, signal, link speed, time connected and data used, with
  **Disconnect** and **Block / Unblock** buttons
- **Share** button with the name, password and a QR code your phone camera can scan to join
- **Log** tab, so you can see why it failed to start

## Why not NetworkManager's built-in hotspot?

NetworkManager (≤ 1.50) always starts its hotspot at **20 MHz** width. On 5 GHz that caps a phone's
link at about 78–87 Mbit/s, which is roughly 30–50 Mbit/s in a speed test. This app runs
[hostapd](https://w1.fi/hostapd/) directly at **80 MHz** instead, which allows up to 433 Mbit/s per
spatial stream.

It's a thin layer over standard tools:

| Part | Does |
|---|---|
| `hostapd` | runs the access point (WPA2, 802.11n/ac) |
| `dnsmasq` | gives devices an address in `10.42.0.0/24` and forwards DNS |
| `iptables` | NAT to whatever connection the laptop is using, plus rules that work with ufw on |
| `hotspotctl` | small root helper that ties it together, run as the `hotspot-manager` systemd service |
| `hotspot-manager` | the GTK 3 window |

While the hotspot is on, NetworkManager is told to leave that one adapter alone. Your normal Wi-Fi
or Ethernet keeps working, and the adapter goes back to NetworkManager when you switch the hotspot off.

Tested on an MSI Katana 15 B13VFK with a **TP-Link Archer T4U v3** (RTL8812BU, in-kernel `rtw88_8822bu`
driver) as the hotspot, the built-in Intel AX201 as the internet connection, Linux Mint 22.3 (Cinnamon),
kernel 7.0.

## Requirements

- A Wi-Fi adapter that supports AP mode (the adapter list in Settings shows this), **separate from the one
  that carries your internet**
- Debian/Ubuntu/Mint (the installer uses apt). On other distros, install `hostapd iw dnsmasq iptables
  qrencode` and PyGObject with GTK 3, then run the installer.

## Install

```sh
git clone https://github.com/xsobhi/hotspot-manager-linux
cd hotspot-manager-linux
sudo ./install.sh
```

Open **Hotspot Manager** from the menu, set a name and password under **Settings**, pick your USB adapter,
**Save and apply**, then flip the switch in the title bar.

Command line, if you prefer it:

```sh
hotspotctl status                     # JSON: state, settings, connected devices
sudo hotspotctl start | stop | restart
echo '{"password": "newpassword"}' | sudo hotspotctl set-config
sudo hotspotctl block aa:bb:cc:dd:ee:ff
```

Uninstall with `sudo ./uninstall.sh`, or `sudo ./uninstall.sh --purge` to also delete the settings in
`/etc/hotspot-manager`.

## Permissions

The polkit rule lets the **logged-in local user** run `hotspotctl` as root without a password. That's how the
app can save settings and flip the switch without prompting, like NetworkManager's own hotspot. Remote and
inactive sessions still need an admin password. `hotspotctl` validates every input: the SSID,
password, channel, country code, adapter name and MAC addresses. If you want a password prompt every time,
change `allow_active` to `auth_admin_keep` in `/usr/share/polkit-1/actions/io.github.xsobhi.hotspot-manager.policy`.

The Wi-Fi password is stored in `/etc/hotspot-manager/password` (root-only, mode 600).

## Notes

- **Only non-DFS channels are offered** (5 GHz: 36–48 and 149–165). DFS channels need radar detection,
  which USB adapters generally can't do in AP mode. Channels 149–165 aren't allowed for hotspots in some
  countries (e.g. most of Europe). If one fails to start, the Log tab says why. Use 36–48.
- **Set your country** in Settings. It decides which channels and transmit power are legal, and with the
  default world setting many adapters refuse to host on 5 GHz.
- **RTL8812BU / RTL8822BU (`rtw88_8822bu`) on 5 GHz:** with the driver's beamforming support on,
  devices could see the network but never finished connecting. The log showed
  `failed to get tx report from firmware`. Turning it off fixed it:
  ```sh
  echo 'options rtw88_core support_bf=N' | sudo tee /etc/modprobe.d/rtw88-hotspot.conf
  sudo modprobe -r rtw88_8822bu rtw88_usb rtw88_8822b rtw88_core && sudo modprobe rtw88_8822bu
  ```
- The hotspot hands out `10.42.0.0/24`, the same range NetworkManager's hotspot uses, so don't run both
  at once.

## License

MIT
