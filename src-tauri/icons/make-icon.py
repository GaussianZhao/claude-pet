#!/usr/bin/env python3
"""Generates a 1024x1024 RGBA source icon for Claude Pet using only stdlib.

Run `cargo tauri icon icons/source.png` afterwards to produce the full set.
"""
import math
import struct
import zlib
import os

N = 1024
buf = bytearray(N * N * 4)


def put(x, y, r, g, b, a=255):
    if 0 <= x < N and 0 <= y < N:
        i = (y * N + x) * 4
        # alpha-over composite
        ia = a / 255
        buf[i] = int(buf[i] * (1 - ia) + r * ia)
        buf[i + 1] = int(buf[i + 1] * (1 - ia) + g * ia)
        buf[i + 2] = int(buf[i + 2] * (1 - ia) + b * ia)
        buf[i + 3] = max(buf[i + 3], a)


def rounded(px, py, x, y, w, h, rad):
    """Signed-ish test: is (px,py) inside a rounded rect?"""
    cx = min(max(px, x + rad), x + w - rad)
    cy = min(max(py, y + rad), y + h - rad)
    if x + rad <= px <= x + w - rad or y + rad <= py <= y + h - rad:
        return x <= px <= x + w and y <= py <= y + h
    return (px - cx) ** 2 + (py - cy) ** 2 <= rad * rad


for y in range(N):
    for x in range(N):
        # Background rounded square with vertical purple gradient.
        if rounded(x, y, 64, 64, 896, 896, 220):
            t = y / N
            r = int(0x8b + (0x6d - 0x8b) * t)
            g = int(0x5c + (0x28 - 0x5c) * t)
            b = int(0xf6 + (0xd9 - 0xf6) * t)
            put(x, y, r, g, b, 255)

# Body blob.
for y in range(N):
    for x in range(N):
        if rounded(x, y, 312, 300, 400, 440, 150):
            put(x, y, 0xed, 0xe9, 0xfe, 255)

# Antenna.
for y in range(250, 305):
    for x in range(505, 519):
        put(x, y, 0xed, 0xe9, 0xfe, 255)
for y in range(N):
    for x in range(N):
        if (x - 512) ** 2 + (y - 245) ** 2 <= 26 ** 2:
            put(x, y, 0x8b, 0x5c, 0xf6, 255)

# Eyes + pupils.
for (ex) in (430, 594):
    for y in range(N):
        for x in range(N):
            d = (x - ex) ** 2 + (y - 470) ** 2
            if d <= 52 ** 2:
                put(x, y, 0xff, 0xff, 0xff, 255)
            if d <= 24 ** 2:
                put(x, y, 0x31, 0x2e, 0x81, 255)

# Smile.
for a in range(200):
    ang = math.pi * (0.15 + 0.7 * a / 200)
    cx = 512 + math.cos(ang) * 70
    cy = 560 + math.sin(ang) * 50
    for dx in range(-7, 8):
        for dy in range(-7, 8):
            put(int(cx) + dx, int(cy) + dy, 0x31, 0x2e, 0x81, 255)

# Encode PNG (filter byte 0 per scanline).
raw = bytearray()
for y in range(N):
    raw.append(0)
    raw.extend(buf[y * N * 4:(y + 1) * N * 4])


def chunk(tag, data):
    c = struct.pack(">I", len(data)) + tag + data
    return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)


png = b"\x89PNG\r\n\x1a\n"
png += chunk(b"IHDR", struct.pack(">IIBBBBB", N, N, 8, 6, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
png += chunk(b"IEND", b"")

out = os.path.join(os.path.dirname(__file__), "source.png")
with open(out, "wb") as f:
    f.write(png)
print("wrote", out)
