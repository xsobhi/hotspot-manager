# hotspot-manager-linux

A desktop app for running a **fast 5 GHz Wi-Fi hotspot** on Linux from a second Wi-Fi adapter
(for example a USB dongle), with full control from one window:

- **On/off switch**, and an option to turn it on at startup
- **Name, password, band (5 / 2.4 GHz), channel, channel width (20 / 40 / 80 MHz)**, country,
  hidden network, max devices, and isolating devices from each other
- **Devices**: every device that's connected (or has been), with its name, maker, IP, MAC, signal,
  **live speed it's using right now**, data used and time connected. Search them, sort by any column, and
  give them names you'll recognise
- **Speed limits**: per device, for **all devices** at once, and/or a **total** for the whole hotspot, separately
  for download and upload. Devices are remembered by MAC address, so a limit sticks when they reconnect
- **Network**: hotspot IP (the gateway devices get), subnet mask, DHCP range, lease time and DNS servers, plus a
  **fixed IP per device**. Clashes with your other networks are refused before anything changes
- **Disconnect**, **Block / Unblock** and **Forget** for devices
- **QR code** button: a code your phone camera can scan to join, plus the name and password
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
| `dnsmasq` | gives devices addresses (default `10.42.0.0/24`), fixed IPs, and forwards DNS |
| `iptables` | NAT to whatever connection the laptop is using, plus rules that work with ufw on |
| `tc` (HTB + fq_codel, IFB for upload) | speed limits |
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

## Speed limits

Open **Devices**, select one or more devices, then click **Set speed limit…** (or double-click a device). Choose:

- **Same as all devices**: follow the all-devices limit (the default for every device)
- **No limit**: this device is never limited, even when all devices are
- **Custom**: its own download/upload limit, with quick buttons for 1–50 Mbps

Both the per-device and the all-devices limit can **allow short bursts**. For example, *2 Mbps, bursting to
4 Mbps for 2 seconds* makes pages and small downloads load fast, then holds the device at 2 Mbps. The
burst refills whenever the device uses less than its limit.

**Each device gets** (the bar at the top of the Devices tab) is the fast way to give everyone the same speed,
like MikroTik's PCQ queues. Type e.g. 2 ↓ / 1 ↑ Mbps and press **Apply**, or pick one from **Quick**. Each device
gets its own lane at that speed instead of sharing it, so a device that joins later is limited too.
Devices you gave their own limit keep it, and one click (*make them follow this too*) puts them back on the shared speed.
**Total & bursts…** adds bursts to it, plus an optional **whole-hotspot** cap that everyone shares.

How it works: when a device gets an address, dnsmasq's lease hook records it and rebuilds the `tc` setup.
Download is shaped with an HTB class per limited device on the hotspot adapter, matched by IP. A burst is a
TBF token bucket under that class, with `rate` = limit, `peakrate` = burst speed, and a bucket of
(burst − limit) × seconds. Upload is
redirected to an IFB device (`hm-ifb0`) and shaped the same way. Each class uses fq_codel, so a limited
device still gets low latency. Devices with a manually set (static) IP aren't matched, because limits
follow DHCP leases.

Settings live in `/etc/hotspot-manager/devices.json` (names, limits, last seen) and `limits.json`.
From the command line (speeds in kbit/s, 0 = no limit):

```sh
echo '{"default": {"down": 10000, "up": 5000}}' | sudo hotspotctl set-devices
echo '{"devices": {"aa:bb:cc:dd:ee:ff": {"name": "Living room TV", "limit": {"down": 3000, "up": 1000}}}}' \
  | sudo hotspotctl set-devices
```

## Network settings

Under **Settings → Network**:

| Setting | Default | Notes |
|---|---|---|
| Hotspot IP (gateway) | `10.42.0.1` | Must be private (10.x, 172.16–31.x, 192.168.x). Changing it moves the DHCP range along with it |
| Subnet mask | `255.255.255.0` (/24) | /16 to /29 |
| DHCP range | `10.42.0.10` – `10.42.0.254` | Inside the subnet, with the hotspot IP outside it |
| Lease time | 12 hours | |
| DNS servers | automatic | Empty = devices ask the hotspot, which uses the laptop's DNS. Or up to 3, e.g. `1.1.1.1, 8.8.8.8` |

For a **fixed IP**, select a device and click **Edit…**. The address can be inside or outside the DHCP range.
If the device is online, it gets reconnected so it picks up the new address. Fixed IPs are keyed by MAC, so a
phone using a *random* ("private") MAC per network keeps it only as long as it keeps that MAC.

## Permissions

The polkit rule lets the **logged-in local user** run `hotspotctl` as root without a password. That's how the
app can save settings and flip the switch without prompting, like NetworkManager's own hotspot. Remote and
inactive sessions still need an admin password. `hotspotctl` validates every input: the SSID,
password, channel, country code, adapter name, MAC addresses, device names, speed values and IP settings. If you want a password prompt every time,
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
