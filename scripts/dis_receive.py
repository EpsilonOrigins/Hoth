#!/usr/bin/env python3
"""Decode selected fields of DIS 7 Entity State PDUs from local UDP."""

import argparse
import json
import socket
import struct


def decode(packet):
    version, exercise, pdu_type, family, stamp, length = struct.unpack_from(
        "!BBBBIH", packet
    )
    if (version, pdu_type, family, length, len(packet)) != (7, 1, 1, 144, 144):
        raise ValueError("expected a 144-byte DIS 7 Entity State PDU")
    return {
        "exercise": exercise,
        "time_in_hour_seconds": (stamp >> 1) * 3600 / (1 << 31),
        "entity": struct.unpack_from("!HHH", packet, 12),
        "norad_id": int(packet[129:140].rstrip(b"\0")),
        "velocity_ecef_m_per_s": struct.unpack_from("!fff", packet, 36),
        "position_ecef_m": struct.unpack_from("!ddd", packet, 48),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=3000)
    parser.add_argument("--count", type=int, default=100)
    parser.add_argument("--timeout", type=float, default=5)
    args = parser.parse_args()
    if args.count <= 0 or args.timeout <= 0:
        parser.error("count and timeout must be positive")
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as receiver:
        receiver.bind(("127.0.0.1", args.port))
        receiver.settimeout(args.timeout)
        for _ in range(args.count):
            print(json.dumps(decode(receiver.recv(2048))))


if __name__ == "__main__":
    main()
