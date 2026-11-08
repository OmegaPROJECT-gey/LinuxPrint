# Localization

**English** | [Русский](LANGUAGES.ru.md)

Five interface languages — Russian and English in a compiled dictionary,
Ukrainian, German and French as a gettext overlay on top of it. Overview:
[../README.md](../README.md); release routine: [MAINTENANCE.md](MAINTENANCE.md).

## The dictionary (RU/EN)

`crates/linuxprinterg-core/src/i18n.rs` holds **key → (en, ru)** pairs:

* `ALL_KEYS` — the ordered list of keys; `entry(key)` — the pair;
* `tr(key)` — the string for the current language; an unknown key is
  returned as-is (visible in the UI, never a crash);
* `trf(key, &[("x", value)])` — substitution of `{x}` placeholders, e.g.
  `"crop {x} × {y} mm"`;
* `catalog()` — the (key, en, ru) iterator used to extract `.po` files.

Completeness is guaranteed by tests in the same file:
`all_keys_have_translations` (every `ALL_KEYS` entry has a pair),
`catalog_covers_dictionary`, `translation_switches_by_language`.

## The gettext layer (uk/de/fr)

`msgid` is a pseudo-key of the dictionary (`print.scaling`), the source
English string lives in the `# EN: …` comment, the translation is `msgstr`.
All PO/MO handling is our own code in `crates/linuxprinterg-core/src/po.rs` —
no system gettext at build time or at runtime:

* `po/linuxprinterg.pot` is regenerated from the dictionary on every
  extraction; `po/{uk,de,fr}.po` skeletons are **never overwritten** —
  missing keys are appended to the existing files (a msgmerge-lite), so
  existing translations survive;
* `parse_po` understands comments, multi-line strings, escapes and skips
  `#, fuzzy` entries and empty `msgstr` (they fall back to English);
* `write_mo` / `parse_mo` build and read the binary MO (little- and
  big-endian on read, UTF-8 header).

```bash
make po         # po/linuxprinterg.pot + skeletons po/{uk,de,fr}.po
make po-build   # locale/<ll>/LC_MESSAGES/linuxprinterg.mo from filled .po
```

## Loading at runtime

`set_language()` loads `<base>/<ll>/LC_MESSAGES/linuxprinterg.mo` as an
overlay: a translated key wins, everything else falls back to the English
dictionary line. Search order (first hit wins):

1. `LINUXPRINTERG_LOCALE_DIR` (override for tests and portable runs);
2. `~/.local/share/linuxprinterg/locale`;
3. `XDG_DATA_DIRS`/locale (for deb/rpm this is `/usr/share/locale`);
4. `/app/share/locale` (Flatpak).

Language is chosen in settings — auto / RU / EN / UK / DE / FR; "auto"
resolves via `LC_ALL` → `LC_MESSAGES` → `LANG`. RU/EN are always available
from the dictionary, so a missing or broken `.mo` degrades to English
instead of breaking the UI.

## Adding a language, step by step

1. Add the language to `Language` in `i18n.rs` (with `gettext_code()`),
   `LangChoice` in `storage/settings.rs` and the language row in
   `crates/linuxprinterg/src/pages/settings.rs`;
2. add its code to `LANGS` in `crates/linuxprinterg-core/src/bin/extract-po.rs`;
3. `make po` — creates `po/<ll>.po` with every key and an empty `msgstr`;
4. fill in `msgstr` (keep `{x}` placeholders identical to the `# EN:` line);
5. validate: `msgfmt --check po/<ll>.po` (gettext is used only for this
   external check — never by the build);
6. `make po-build` — build `locale/<ll>/LC_MESSAGES/linuxprinterg.mo`;
7. ship the `.mo`: add `locale/<ll>` to `packaging/nfpm.yml` and the
   `install` line to
   `crates/linuxprinterg/data/org.linuxprinterg.LinuxPrinterG.flatpak.yml`
   (see [PACKAGING.md](PACKAGING.md)).

After a dictionary change: `make po` again — new keys are appended to the
existing `.po` files with their translations untouched.

## Where the catalogs live

| Artifact | Path |
|---|---|
| sources | `po/linuxprinterg.pot`, `po/{uk,de,fr}.po` |
| built catalogs (repo) | `locale/<ll>/LC_MESSAGES/linuxprinterg.mo` |
| .deb / .rpm | `/usr/share/locale/<ll>/LC_MESSAGES/linuxprinterg.mo` |
| Flatpak | `/app/share/locale/<ll>/LC_MESSAGES/linuxprinterg.mo` |

`.deb`/`.rpm` builds fail on purpose if a `.mo` is missing
(`scripts/build-packages.sh` checks all three languages).

See also: [../README.md](../README.md), [MAINTENANCE.md](MAINTENANCE.md),
[../CONTRIBUTING.md](../CONTRIBUTING.md).
