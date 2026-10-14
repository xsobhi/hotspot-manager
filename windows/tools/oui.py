#!/usr/bin/env python3
"""The makers of phones, tablets, laptops and consoles by MAC prefix, for naming devices that
don't give a name (the Linux app reads the whole IEEE list from /usr/share/ieee-data instead).
Reads IEEE's registry (https://standards-oui.ieee.org/oui/oui.csv) and writes data/oui.txt:

    python3 windows/tools/oui.py oui.csv
"""
import csv
import re
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "data/oui.txt"

# (pattern in the registered name, the short name shown)
BRANDS = [
    (r"^Apple", "Apple"), (r"^Samsung Electro", "Samsung"), (r"Xiaomi", "Xiaomi"), (r"^Huawei", "Huawei"),
    (r"^Honor Device", "Honor"), (r"^Google", "Google"), (r"OnePlus", "OnePlus"), (r"^GUANGDONG OPPO|^OPPO", "OPPO"),
    (r"vivo Mobile", "vivo"), (r"^Realme", "realme"), (r"^Motorola Mobility", "Motorola"), (r"^LG Electronics", "LG"),
    (r"^Sony", "Sony"), (r"^Nokia", "Nokia"), (r"^HMD Global", "Nokia"), (r"^HTC", "HTC"), (r"^Lenovo", "Lenovo"),
    (r"^Dell", "Dell"), (r"^Hewlett Packard|^HP Inc", "HP"), (r"^ASUSTek", "ASUS"), (r"^Acer", "Acer"),
    (r"^Microsoft", "Microsoft"), (r"^Intel Corporate", "Intel"), (r"^Amazon Technologies", "Amazon"),
    (r"^Nintendo", "Nintendo"), (r"^Infinix|^TECNO|^Transsion|^ITEL", "Transsion (Tecno/Infinix/itel)"),
    (r"^ZTE", "ZTE"), (r"^TCL", "TCL"), (r"^Realtek", "Realtek"), (r"^MediaTek", "MediaTek"),
    (r"^Qualcomm|^QUALCOMM", "Qualcomm"), (r"^Murata", "Murata"), (r"^AzureWave", "AzureWave"),
    (r"^LITE-ON|^Liteon", "Lite-On"), (r"^Hon Hai|^HON HAI", "Foxconn"), (r"^Chicony", "Chicony"),
    (r"^Espressif", "Espressif"), (r"^Raspberry Pi", "Raspberry Pi"), (r"^Valve", "Valve"),
    (r"^Meta Platforms|^Oculus", "Meta"), (r"^Fitbit", "Fitbit"), (r"^Garmin", "Garmin"), (r"^Roku", "Roku"),
    (r"^TP-LINK|^TP-Link", "TP-Link"), (r"^Infinix", "Infinix"), (r"^Nothing Technology", "Nothing"),
    (r"^Fairphone", "Fairphone"), (r"^Micro-Star|^MSI", "MSI"), (r"^GIGA-BYTE", "Gigabyte"), (r"^Razer", "Razer"),
]


def main(path):
    lines = set()
    with open(path, encoding="utf-8", errors="replace") as f:
        for row in csv.DictReader(f):
            name = row.get("Organization Name", "").strip()
            prefix = row.get("Assignment", "").strip().lower()
            for pattern, brand in BRANDS:
                if re.search(pattern, name, re.I) and len(prefix) == 6:
                    lines.add(f"{prefix} {brand}")
                    break
    OUT.write_text("\n".join(sorted(lines)) + "\n")
    print(f"{len(lines)} prefixes written to {OUT}")


if __name__ == "__main__":
    main(sys.argv[1])
