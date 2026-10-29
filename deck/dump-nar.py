#!/usr/bin/env python3
"""Write a Nix archive (.nar) of a directory tree — same bytes as nix-store --dump."""

from __future__ import annotations

import os
import sys
from pathlib import Path


def st(b: bytes | str) -> bytes:
    if isinstance(b, str):
        b = b.encode()
    return len(b).to_bytes(8, "little") + b + (-len(b) % 8) * b"\0"


def serialise1(path: Path) -> bytes:
    return st(b"(") + serialise2(path) + st(b")")


def serialise2(path: Path) -> bytes:
    if path.is_symlink():
        return (
            st(b"type")
            + st(b"symlink")
            + st(b"target")
            + st(os.fsencode(os.readlink(path)))
        )
    if path.is_file():
        out = st(b"type") + st(b"regular")
        if path.stat().st_mode & 0o100:
            out += st(b"executable") + st(b"")
        return out + st(b"contents") + st(path.read_bytes())
    if path.is_dir():
        out = st(b"type") + st(b"directory")
        for child in sorted(path.iterdir(), key=lambda p: p.name):
            out += (
                st(b"entry")
                + st(b"(")
                + st(b"name")
                + st(os.fsencode(child.name))
                + st(b"node")
                + serialise1(child)
                + st(b")")
            )
        return out
    raise SystemExit(f"unsupported path type: {path}")


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit(f"usage: {sys.argv[0]} <directory> <output.nar>")
    root = Path(sys.argv[1])
    out = Path(sys.argv[2])
    if not root.is_dir():
        raise SystemExit(f"not a directory: {root}")
    out.write_bytes(st(b"nix-archive-1") + serialise1(root))


if __name__ == "__main__":
    main()
