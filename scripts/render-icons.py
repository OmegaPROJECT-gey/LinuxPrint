#!/usr/bin/env python3
"""Растеризация иконки LinuxPrinterG в дерево hicolor.

Источник: Icon.png (корень репозитория, 1254x1254, прозрачный фон).
Результат:
  * crates/linuxprinterg/data/icons/hicolor/{16..256}x{NN}/apps/<id>.png
  * crates/linuxprinterg/data/icons/hicolor/scalable/apps/<id>.svg
    (SVG встраивает растровое ядро 512x512 в base64, чтобы векторная
    и растровая ветки hicolor показывали одну и ту же иконку)

Уменьшение — пошаговое (halving), а не одним шагом: cairo при коэффициенте
×78 иначе даёт артефакы выборки. Зависимости: python3-cairo (без внешних
утилит вроде ImageMagick/rsvg-convert).

Запуск: python3 scripts/render-icons.py
"""
from __future__ import annotations

import base64
import io
import os
import sys

import cairo

APP_ID = "org.linuxprinterg.LinuxPrinterG"
SIZES = [16, 24, 32, 48, 64, 128, 256]
SCALABLE = 512


def _halve(surface: cairo.ImageSurface) -> cairo.ImageSurface:
    """Уменьшить поверхность ровно вдвое (box-среднее через масштаб)."""
    w, h = surface.get_width(), surface.get_height()
    out = cairo.ImageSurface(cairo.FORMAT_ARGB32, w // 2, h // 2)
    ctx = cairo.Context(out)
    ctx.scale((w // 2) / w, (h // 2) / h)
    ctx.set_source_surface(surface, 0, 0)
    ctx.paint()
    return out


def shrink_to(surface: cairo.ImageSurface, target: int) -> cairo.ImageSurface:
    """Пошагово уменьшить до квадрата `target`x`target`."""
    while surface.get_width() > target * 2 and surface.get_height() > target * 2:
        surface = _halve(surface)
    w, h = surface.get_width(), surface.get_height()
    if w == target and h == target:
        return surface
    out = cairo.ImageSurface(cairo.FORMAT_ARGB32, target, target)
    ctx = cairo.Context(out)
    ctx.scale(target / w, target / h)
    ctx.set_source_surface(surface, 0, 0)
    ctx.paint()
    return out


def png_bytes(surface: cairo.ImageSurface) -> bytes:
    buf = io.BytesIO()
    surface.write_to_png(buf)
    return buf.getvalue()


def main() -> int:
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    src = os.path.join(root, "Icon.png")
    out_root = os.path.join(root, "crates", "linuxprinterg", "data", "icons", "hicolor")

    if not os.path.isfile(src):
        print(f"нет источника: {src}", file=sys.stderr)
        return 1

    source = cairo.ImageSurface.create_from_png(src)

    # --- растровые размеры -------------------------------------------------
    for size in SIZES:
        d = os.path.join(out_root, f"{size}x{size}", "apps")
        os.makedirs(d, exist_ok=True)
        out = os.path.join(d, f"{APP_ID}.png")
        shrink_to(source, size).write_to_png(out)
        print(f"{size}x{size} -> {out}")

    # --- scalable: SVG с встроенным растровым ядром ------------------------
    core = png_bytes(shrink_to(source, SCALABLE))
    b64 = base64.b64encode(core).decode("ascii")
    svg = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        "<!-- Иконка LinuxPrinterG: Tux с принтером и листом «G».\n"
        "     Источник — Icon.png (корень репозитория); scalable-версия\n"
        "     встраивает растровое ядро 512x512 (base64), чтобы векторная\n"
        "     и растровая ветки hicolor не расходились.\n"
        "     Перегенерация: python3 scripts/render-icons.py -->\n"
        '<svg xmlns="http://www.w3.org/2000/svg" '
        'xmlns:xlink="http://www.w3.org/1999/xlink"\n'
        f'     width="{SCALABLE}" height="{SCALABLE}" viewBox="0 0 {SCALABLE} {SCALABLE}">\n'
        f'  <image width="{SCALABLE}" height="{SCALABLE}" '
        f'xlink:href="data:image/png;base64,{b64}"/>\n'
        "</svg>\n"
    )
    svg_dir = os.path.join(out_root, "scalable", "apps")
    os.makedirs(svg_dir, exist_ok=True)
    svg_path = os.path.join(svg_dir, f"{APP_ID}.svg")
    with open(svg_path, "w", encoding="utf-8") as f:
        f.write(svg)
    print(f"scalable -> {svg_path}")

    print("OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
