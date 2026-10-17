# LinuxPrinterG — сборка, тесты, упаковка.
# Требования для сборки UI: gtk4-devel >= 4.14, libadwaita-devel >= 1.5.
# Требования для работы: cups-client (lp/lpstat), sane-backends (scanimage),
# poppler-utils (pdftoppm, предпросмотр PDF), tesseract (OCR, опционально),
# libreoffice (конвертация DOCX/ODT, опционально).

APP        := linuxprinterg
CORE       := linuxprinterg-core
CARGO      := cargo
DIST       := dist

.PHONY: all build release test test-int fmt fmt-check clippy run clean install flatpak deb rpm appimage

all: build

build:
	$(CARGO) build --workspace

release:
	$(CARGO) build --release --workspace

test:
	$(CARGO) test --workspace

# Интеграционные тесты, вызывающие внешние утилиты (lpstat, scanimage, pdftoppm).
test-int:
	$(CARGO) test -p $(CORE) -- --ignored --test-threads=1

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

clippy:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

run:
	$(CARGO) run --release --workspace

clean:
	$(CARGO) clean
	rm -rf $(DIST)

install: release
	install -Dm755 target/release/$(APP) "$(DESTDIR)/usr/bin/$(APP)"
	install -Dm644 crates/$(APP)/data/org.linuxprinterg.LinuxPrinterG.desktop \
		"$(DESTDIR)/usr/share/applications/org.linuxprinterg.LinuxPrinterG.desktop"
	install -Dm644 crates/$(APP)/data/org.linuxprinterg.LinuxPrinterG.metainfo.xml \
		"$(DESTDIR)/usr/share/metainfo/org.linuxprinterg.LinuxPrinterG.metainfo.xml"

# --- Flatpak -------------------------------------------------------------
# Песочница ограничивает доступ к USB (SANE) и CUPS; manifest фиксирует это явно.
# См. README, раздел «Ограничения Flatpak».
flatpak:
	flatpak-builder --force-clean --user --install build-flatpak \
		crates/$(APP)/data/org.linuxprinterg.LinuxPrinterG.flatpak.yml

# --- Дистрибутивы --------------------------------------------------------
deb: release
	$(CARGO) deb -p $(APP) --no-strip --output $(DIST)

rpm: release
	cargo rpm init -p $(APP) 2>/dev/null || true
	cargo rpm build

# AppImage: используйте linuxdeploy с GTK-плагином.
appimage: release
	@echo "Используйте linuxdeploy + linuxdeploy-plugin-gtk:"
	@echo "  linuxdeploy --appdir AppDir -e target/release/$(APP) \\\\"
	@echo "    --plugin gtk --output appimage"
