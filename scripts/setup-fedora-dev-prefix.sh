#!/usr/bin/env bash
# Распаковка -devel RPM в пользовательский префикс для сборки GTK4/libadwaita
# приложения БЕЗ root (песочницы, CI-окружения без dev-пакетов).
#
# Идемпотентен: повторный запуск пересоздаёт префикс из уже скачанных RPM.
# В конце печатает переменные окружения, которые нужно выставить перед сборкой:
#
#   source scripts/setup-fedora-dev-prefix.sh   # либо выполнить и export самому
#
set -euo pipefail

PREFIX="${PREFIX:-/tmp/gtkprefix}"
RPMDIR="${RPMDIR:-/tmp/linuxprinterg-rpms}"
PKG_CONFIG_PATH_EXPORT="$PREFIX/usr/lib64/pkgconfig:$PREFIX/usr/share/pkgconfig"

PKGS=(
    gtk4-devel
    libadwaita-devel
    glib2-devel
    gdk-pixbuf2-devel
    pango-devel
    cairo-devel
    cairo-gobject-devel
    graphene-devel
    vulkan-loader-devel
    appstream-devel
)

command -v dnf >/dev/null || {
    echo "нужен dnf (Fedora/RHEL-совместимая система)" >&2
    exit 1
}
command -v rpm2cpio >/dev/null || {
    echo "нужен rpm2cpio (пакет cpio)" >&2
    exit 1
}

mkdir -p "$PREFIX" "$RPMDIR"

# 1. Скачивание (повторное скачивание пропускается, если RPM уже есть).
for p in "${PKGS[@]}"; do
    if compgen -G "$RPMDIR/${p}-*.rpm" >/dev/null; then
        continue
    fi
    echo "скачивание: $p"
    dnf download --destdir "$RPMDIR" "$p" >/dev/null
done

# 2. Распаковка в префикс.
for rpm in "$RPMDIR"/*.rpm; do
    (cd "$PREFIX" && rpm2cpio "$rpm" | cpio -idmu --quiet)
done

# 3. devel-RPM содержат симлинки libX.so -> libX.so.N, а версионные файлы
#    живут в системном runtime. Если цепочка в префиксе висит — перенаправляем
#    симлинк на установленную в системе библиотеку.
fixed=0
for link in "$PREFIX"/usr/lib64/*.so "$PREFIX"/usr/lib64/*.so.*; do
    [ -L "$link" ] || continue
    if [ ! -e "$link" ]; then
        target="$(readlink "$link")"
        case "$target" in
            /*) ;;
            *) target="/usr/lib64/$target" ;;
        esac
        if [ -e "$target" ]; then
            ln -sfn "$target" "$link"
            fixed=$((fixed + 1))
        fi
    fi
done
echo "исправлено висячих симлинков: $fixed"

echo "готово: $PREFIX"
echo "export PKG_CONFIG_PATH=\"$PKG_CONFIG_PATH_EXPORT\""
echo "export RUSTFLAGS=\"-L $PREFIX/usr/lib64\""
