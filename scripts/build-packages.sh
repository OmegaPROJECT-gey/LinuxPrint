#!/bin/sh
# Сборка установщиков LinuxPrinterG: .deb и .rpm через nfpm.
#
# Использование:
#   scripts/build-packages.sh [deb|rpm|all]   (по умолчанию all)
#
# Требует release-сборки (cargo build --release) — Makefile делает это
# зависимостью. Результат: dist/linuxprinterg_<ver>_<arch>.deb
#                           dist/linuxprinterg-<ver>-1.<arch>.rpm
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PACKAGER="${1:-all}"
case "$PACKAGER" in
    deb|rpm|all) ;;
    *)
        echo "неизвестный пакет: $PACKAGER (deb|rpm|all)" >&2
        exit 1
        ;;
esac

# Версия из workspace Cargo.toml (первая строка version = ...).
VERSION="$(grep -m1 '^version = ' Cargo.toml | sed 's/^version = "\(.*\)"/\1/')"
if [ -z "$VERSION" ]; then
    echo "не удалось определить версию из Cargo.toml" >&2
    exit 1
fi
echo "версия: $VERSION"

# Release-бинарник обязателен.
BIN="target/release/linuxprinterg"
if [ ! -x "$BIN" ]; then
    echo "нет $BIN — выполните: cargo build --release" >&2
    exit 1
fi

# Иконки должны быть в дереве (рендер: python3 scripts/render-icons.py).
if [ ! -f crates/linuxprinterg/data/icons/hicolor/scalable/apps/org.linuxprinterg.LinuxPrinterG.svg ]; then
    echo "нет иконок в hicolor — выполните: python3 scripts/render-icons.py" >&2
    exit 1
fi

# Переводы: собрать .mo из po/*.po (собственный писатель MO, без gettext).
cargo run -q -p linuxprinterg-core --bin extract-po -- --build
for L in uk de fr; do
    if [ ! -f "locale/$L/LC_MESSAGES/linuxprinterg.mo" ]; then
        echo "нет locale/$L/…mo — заполните po/$L.po и выполните: make po-build" >&2
        exit 1
    fi
done

# nfpm: получить при необходимости.
NFPM="$ROOT/target/tools/nfpm"
if [ ! -x "$NFPM" ]; then
    "$ROOT/scripts/get-nfpm.sh"
fi

# Сгенерировать конфиг с подставленной версией.
mkdir -p target/package dist
sed "s/__VERSION__/${VERSION}/" packaging/nfpm.yml > target/package/nfpm.yml

ARCH_GO="$(uname -m)"
case "$ARCH_GO" in
    x86_64)  ARCH_DEB="amd64";   ARCH_RPM="x86_64" ;;
    aarch64) ARCH_DEB="arm64";   ARCH_RPM="aarch64" ;;
    *)       ARCH_DEB="$ARCH_GO"; ARCH_RPM="$ARCH_GO" ;;
esac

build_one() {
    fmt="$1"
    case "$fmt" in
        deb) target="dist/linuxprinterg_${VERSION}_${ARCH_DEB}.deb" ;;
        rpm) target="dist/linuxprinterg-${VERSION}-1.${ARCH_RPM}.rpm" ;;
    esac
    rm -f "$target"
    "$NFPM" package \
        --config target/package/nfpm.yml \
        --packager "$fmt" \
        --target "$target"
    echo "готово: $target"
}

if [ "$PACKAGER" = "deb" ] || [ "$PACKAGER" = "all" ]; then
    build_one deb
fi
if [ "$PACKAGER" = "rpm" ] || [ "$PACKAGER" = "all" ]; then
    build_one rpm
fi

ls -la dist/
