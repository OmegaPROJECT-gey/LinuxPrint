#!/usr/bin/env python3
"""Растеризация иконки LinuxPrinterG в дерево hicolor.

Источник: crates/linuxprinterg/data/icons/org.linuxprinterg.LinuxPrinterG.svg
Результат: crates/linuxprinterg/data/icons/hicolor/{scalable,NNxNN}/apps/...

Рендер напрямую в целевой размер через librsvg + cairo
(GObject-типелайб Rsvg-2.0, python3-gi + python3-cairo) — аналог rsvg-convert,
которого в системе может не быть.

Запуск: python3 scripts/render-icons.py
"""
from __future__ import annotations

import os
import sys

import cairo
import gi

gi.require_version("Rsvg", "2.0")
from gi.repository import Rsvg  # noqa: E402

APP_ID = "org.linuxprinterg.LinuxPrinterG"
SIZES = [16, 24, 32, 48, 64, 128, 256]


def render(handle: Rsvg.Handle, size: int, out_path: str) -> None:
    surface = cairo.ImageSurface(cairo.FORMAT_ARGB32, size, size)
    ctx = cairo.Context(surface)
    scale = size / 512.0
    ctx.scale(scale, scale)
    handle.render_cairo(ctx)
    surface.write_to_png(out_path)


def main() -> int:
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    src = os.path.join(root, "crates", "linuxprinterg", "data", "icons", f"{APP_ID}.svg")
    out_root = os.path.join(root, "crates", "linuxprinterg", "data", "icons", "hicolor")

    if not os.path.isfile(src):
        print(f"нет источника: {src}", file=sys.stderr)
        return 1

    handle = Rsvg.Handle.new_from_file(src)

    # scalable (SVG копируется как есть)
    scalable_dir = os.path.join(out_root, "scalable", "apps")
    os.makedirs(scalable_dir, exist_ok=True)
    with open(src, "rb") as f:
        data = f.read()
    with open(os.path.join(scalable_dir, f"{APP_ID}.svg"), "wb") as f:
        f.write(data)

    # растровые размеры
    for size in SIZES:
        d = os.path.join(out_root, f"{size}x{size}", "apps")
        os.makedirs(d, exist_ok=True)
        out = os.path.join(d, f"{APP_ID}.png")
        render(handle, size, out)
        print(f"{size}x{size} -> {out}")

    print("OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
