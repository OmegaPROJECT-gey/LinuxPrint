#!/bin/sh
# Загрузка статического бинарника nfpm (упаковщик .deb/.rpm без
# rpmbuild/dpkg-deb). Бинарник кладётся в target/tools/nfpm и не коммитится.
# Используется scripts/build-packages.sh автоматически.
set -eu

NFPM_VERSION="${NFPM_VERSION:-2.48.0}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TOOLS="$ROOT/target/tools"
NFPM="$TOOLS/nfpm"

if [ -x "$NFPM" ] && "$NFPM" --version 2>/dev/null | grep -q "GitVersion:.*${NFPM_VERSION}"; then
    echo "nfpm $NFPM_VERSION уже установлен: $NFPM"
    exit 0
fi

ARCH="$(uname -m)"
case "$ARCH" in
    x86_64)  GOARCH="x86_64" ;;
    aarch64) GOARCH="arm64" ;;
    *)
        echo "неподдерживаемая архитектура: $ARCH (скачайте nfpm вручную)" >&2
        exit 1
        ;;
esac

mkdir -p "$TOOLS"
URL="https://github.com/goreleaser/nfpm/releases/download/v${NFPM_VERSION}/nfpm_${NFPM_VERSION}_Linux_${GOARCH}.tar.gz"
echo "загрузка nfpm ${NFPM_VERSION} ($GOARCH)..."
TMP="$TOOLS/nfpm.tar.gz.part"
curl -fsSL --max-time 180 -o "$TMP" "$URL"
tar -xzf "$TMP" -C "$TOOLS" nfpm
rm -f "$TMP"
chmod 0755 "$NFPM"
"$NFPM" --version | head -2
