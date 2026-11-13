#!/usr/bin/env python3
"""Edits Sheprd's public relay list, website/site/relays.json.

Clients take the list only when it is signed (docs/relay-plan.md), so every
change goes through `just relays-add`, `just relays-remove`, or
`just relays-sign`, which call this script and then sign the result.

  relays.py add FILE ENDPOINT --region R --name N [--pin PIN | --no-pin]
  relays.py remove FILE ENDPOINT
  relays.py stamp FILE [--days N]

`add` reads the relay's QUIC pin from its /v1/info unless one is given, and
prints it so it can be compared with the `pin=` the relay logs at start.
Every command stamps the list: a serial no older than the current time and
greater than the last, and an expiry (365 days by default).
"""

import argparse
import json
import sys
import time
import urllib.request

DAY = 24 * 60 * 60
VERSION = 1


def load(path):
    with open(path, encoding="utf-8") as file:
        data = json.load(file)
    if data.get("version") != VERSION:
        sys.exit(f"{path}: version {data.get('version')}, this script writes {VERSION}")
    return data


def save(path, data):
    ordered = {
        "version": data["version"],
        "serial": data["serial"],
        "expires": data["expires"],
        "relays": data["relays"],
    }
    with open(path, "w", encoding="utf-8") as file:
        json.dump(ordered, file, indent=2, ensure_ascii=False)
        file.write("\n")


def stamp(data, days):
    now = int(time.time())
    data["serial"] = max(now, int(data.get("serial", 0)) + 1)
    data["expires"] = now + days * DAY


def normalize(endpoint):
    endpoint = endpoint.strip().rstrip("/")
    if "://" not in endpoint:
        endpoint = "https://" + endpoint
    if not endpoint.startswith("https://"):
        sys.exit(f"{endpoint}: public relays are https only")
    return endpoint


def fetch_pin(endpoint):
    with urllib.request.urlopen(endpoint + "/v1/info", timeout=10) as response:
        info = json.load(response)
    quic = info.get("quic") or {}
    pin = quic.get("pin")
    if not pin:
        sys.exit(f"{endpoint} does not serve QUIC; pass --no-pin to list it anyway")
    return pin


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    add = commands.add_parser("add")
    add.add_argument("file")
    add.add_argument("endpoint")
    add.add_argument("--region", required=True)
    add.add_argument("--name", required=True)
    pin = add.add_mutually_exclusive_group()
    pin.add_argument("--pin")
    pin.add_argument("--no-pin", action="store_true")
    remove = commands.add_parser("remove")
    remove.add_argument("file")
    remove.add_argument("endpoint")
    stamp_only = commands.add_parser("stamp")
    stamp_only.add_argument("file")
    for command in (add, remove, stamp_only):
        command.add_argument("--days", type=int, default=365)
    args = parser.parse_args()

    data = load(args.file)
    relays = data.setdefault("relays", [])
    if args.command == "add":
        endpoint = normalize(args.endpoint)
        relays[:] = [relay for relay in relays if relay.get("endpoint") != endpoint]
        relay = {"endpoint": endpoint, "region": args.region, "name": args.name}
        if not args.no_pin:
            relay["quic_pin"] = args.pin or fetch_pin(endpoint)
            print(f"{endpoint}: QUIC pin {relay['quic_pin']}")
            print("Compare it with the pin= the relay logged at start before signing.")
        relays.append(relay)
    elif args.command == "remove":
        endpoint = normalize(args.endpoint)
        kept = [relay for relay in relays if relay.get("endpoint") != endpoint]
        if len(kept) == len(relays):
            sys.exit(f"{endpoint} is not in {args.file}")
        relays[:] = kept
    stamp(data, args.days)
    save(args.file, data)
    print(f"{args.file}: serial {data['serial']}, {len(relays)} relays, expires in {args.days} days")


if __name__ == "__main__":
    main()
