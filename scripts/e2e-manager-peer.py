#!/usr/bin/env python3
"""Read Linux SO_PEERCRED from a connected private systemd UNIX socket.

The request is intentionally empty: connection credentials are kernel-provided
and are checked BEFORE systemctl sends any service-management instruction.
This script is test infrastructure, not part of the shipped product.
"""
import json
import socket
import struct
import sys


def main():
    if len(sys.argv) != 2:
        raise SystemExit("one private socket path required")
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(2)
        connection.connect(sys.argv[1])
        raw = connection.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12)
        pid, uid, gid = struct.unpack("3i", raw)
    print(json.dumps({"pid": pid, "uid": uid, "gid": gid}))


if __name__ == "__main__":
    main()
