#!/bin/sh
# Пост-установочный скрипт (deb: postinst, rpm: %post).
# Обновляет кэши рабочего стола и иконок, если утилиты установлены.
set -u

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q /usr/share/applications >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t /usr/share/icons/hicolor >/dev/null 2>&1 || true
fi
if command -v touch >/dev/null 2>&1; then
    touch -r /usr/share/icons/hicolor /usr/share/icons/hicolor/index.theme \
        >/dev/null 2>&1 || true
fi

exit 0
