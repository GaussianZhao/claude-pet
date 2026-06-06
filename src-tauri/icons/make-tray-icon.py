#!/usr/bin/env python3
"""Generates `tray.png` — a monochrome macOS *template* menu-bar icon.

A template image is pure black (RGB 0) with a meaningful alpha mask; macOS
renders it light or dark to match the menu bar automatically. So we only shape
the alpha channel: an opaque robot-head silhouette with the eyes and mouth cut
out. Stdlib only (no PIL). Output is supersampled for smooth edges.

Run this, then rebuild the app — `lib.rs` embeds `tray.png` via include_bytes!.
"""
import os
import struct
import zlib

S = 256          # output size (px)
SS = 4           # supersample factor per axis
M = S * SS       # internal render size


def rounded_inside(px, py, x, y, w, h, rad):
    """Is (px,py) inside a rounded rect [x,y,w,h] with corner radius rad?"""
    if not (x <= px <= x + w and y <= py <= y + h):
        return False
    cx = min(max(px, x + rad), x + w - rad)
    cy = min(max(py, y + rad), y + h - rad)
    if (x + rad <= px <= x + w - rad) or (y + rad <= py <= y + h - rad):
        return True
    return (px - cx) ** 2 + (py - cy) ** 2 <= rad * rad


def in_circle(px, py, cx, cy, r):
    return (px - cx) ** 2 + (py - cy) ** 2 <= r * r


def covered(px, py):
    """True where the silhouette is opaque (body minus the cut-out features)."""
    # Head body.
    body = rounded_inside(px, py, 0.20 * M, 0.30 * M, 0.60 * M, 0.56 * M, 0.17 * M)
    # Antenna: stalk + tip ball.
    stalk = (0.475 * M <= px <= 0.525 * M) and (0.16 * M <= py <= 0.32 * M)
    ball = in_circle(px, py, 0.50 * M, 0.15 * M, 0.055 * M)
    if not (body or stalk or ball):
        return False
    # Cut-outs (eyes + smile) carve holes into the silhouette.
    if in_circle(px, py, 0.395 * M, 0.55 * M, 0.085 * M):
        return False
    if in_circle(px, py, 0.605 * M, 0.55 * M, 0.085 * M):
        return False
    # Smile: a thin downward arc = big circle minus slightly-smaller circle,
    # kept to the lower band only.
    smx, smy, sr, t = 0.50 * M, 0.52 * M, 0.20 * M, 0.035 * M
    d2 = (px - smx) ** 2 + (py - smy) ** 2
    if (sr - t) ** 2 <= d2 <= sr ** 2 and py > smy + 0.10 * M:
        return False
    return True


# Supersample: alpha = fraction of covered sub-pixels.
buf = bytearray(S * S * 4)
inv = 1.0 / (SS * SS)
for oy in range(S):
    for ox in range(S):
        hits = 0
        for sy in range(SS):
            py = oy * SS + sy + 0.5
            for sx in range(SS):
                if covered(ox * SS + sx + 0.5, py):
                    hits += 1
        a = int(hits * inv * 255 + 0.5)
        i = (oy * S + ox) * 4
        buf[i] = 0          # R
        buf[i + 1] = 0      # G
        buf[i + 2] = 0      # B
        buf[i + 3] = a      # alpha mask


def chunk(tag, data):
    return (
        struct.pack(">I", len(data))
        + tag
        + data
        + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    )


raw = bytearray()
for y in range(S):
    raw.append(0)
    raw.extend(buf[y * S * 4:(y + 1) * S * 4])

png = b"\x89PNG\r\n\x1a\n"
png += chunk(b"IHDR", struct.pack(">IIBBBBB", S, S, 8, 6, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
png += chunk(b"IEND", b"")

out = os.path.join(os.path.dirname(__file__), "tray.png")
with open(out, "wb") as f:
    f.write(png)
print("wrote", out, f"({S}x{S} template)")
