## What's new in 1.1.1

- **Internet on networks that block sharing.** Some networks send packets with a TTL of 1, so that what a PC shares dies on the way (laptops on the hotspot had no internet). Packets arriving with TTL 1 or 2 are now raised before Windows forwards them, as MyPublicWiFi does.
- **DNS that works for laptops**: devices get this PC's own DNS servers instead of Windows' sharing relay, which refused some of their questions.

## What's new in 1.1.0

- **Upload speed limits work.** Windows' sharing sent uploads on by itself; the limiter now holds them where they really leave, so blocking and "Share internet" off stop uploads too.
- **Low latency under a speed limit**: short queues, and small packets (pings, games, DNS) go ahead of downloads. A speed test at the limit went from 3 seconds of latency to about 30 ms over idle.
- **Smoother speed tests**: TCP slows down early instead of stalling when a long queue overflows.
- **The adapter you choose hosts**: Windows gives Wi-Fi Direct to one Wi-Fi card at a time; it's moved to the chosen one (a USB adapter plugged in again, say).
- **Internet sharing that doesn't get stuck** after an adapter comes or goes, and the hotspot no longer turns itself off after a few seconds.
- **Time connected** in the device list; the hotspot IP is checked (private, and not a network this PC is on).

## Hotspot Manager for Windows

The Windows version of Hotspot Manager: a Wi-Fi hotspot from this PC's internet, on its own Wi-Fi or a USB Wi-Fi adapter, with the Linux app's controls.

- **Turn it on and off**, with a QR code to join, and at sign-in if you like.
- **See who's connected**: names, makers, how fast they're using it now and how much they used.
- **Speed limits**: for everyone, for each device, with short bursts, and a cap for the whole hotspot.
- **Block or disconnect** a device, keep devices from seeing each other, set the most devices allowed.
- **Your own hotspot address** (any, not only Windows' 192.168.137.1), DHCP range, lease time and DNS servers.
- **Band and channel width**: 5 GHz on adapters whose drivers only host on 2.4 GHz otherwise (through the driver's Wi-Fi mode), and the width up to 80 MHz.
- **No internet to share?** It still starts, for the devices to reach each other and this PC.
- **Share internet** can be turned off for a hotspot without it.

### Install

Download **HotspotManager-…-setup.exe** and run it (one administrator prompt). It installs to Program Files with its helper service, which counts, limits and blocks the devices' traffic and gives out their addresses.

Speed limits, data use and blocking work through [WinDivert](https://reqrypt.org/windivert.html), a packet driver that Microsoft Defender flags as potentially unwanted (as it does most of them, since malware uses them too). The setup program downloads it, and offers to let Defender leave Hotspot Manager's folder alone; without that, Defender may remove it, and everything else still works.

The portable **.zip** has the app and `install.ps1` for installing by hand (`powershell -ExecutionPolicy Bypass -File install.ps1`).

Windows 10 or 11, 64-bit.
