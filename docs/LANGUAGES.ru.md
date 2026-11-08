# Локализация

[English](LANGUAGES.md) | **Русский**

Пять языков интерфейса — русский и английский в компилируемом словаре,
украинский, немецкий и французский как gettext-overlay поверх него. Обзор:
[../README.ru.md](../README.ru.md); релизный процесс:
[MAINTENANCE.ru.md](MAINTENANCE.ru.md).

## Словарь (RU/EN)

`crates/linuxprinterg-core/src/i18n.rs` хранит пары **ключ → (en, ru)**:

* `ALL_KEYS` — упорядоченный список ключей; `entry(key)` — пара;
* `tr(key)` — строка для текущего языка; неизвестный ключ возвращается как
  есть (виден в интерфейсе, но не роняет приложение);
* `trf(key, &[("x", значение)])` — подстановка плейсхолдеров `{x}`, например
  `"crop {x} × {y} mm"`;
* `catalog()` — итератор (ключ, en, ru), из которого извлекаются `.po`.

Полноту гарантируют тесты в этом же файле: `all_keys_have_translations`
(для каждой пары из `ALL_KEYS` есть перевод), `catalog_covers_dictionary`,
`translation_switches_by_language`.

## gettext-слой (uk/de/fr)

`msgid` — псевдоключ словаря (`print.scaling`), исходная английская строка
лежит в комментарии `# EN: …`, перевод — в `msgstr`. Вся работа с PO/MO —
собственный код в `crates/linuxprinterg-core/src/po.rs`: системный gettext
не нужен ни при сборке, ни в рантайме:

* `po/linuxprinterg.pot` пересоздаётся из словаря при каждой выгрузке;
  скелеты `po/{uk,de,fr}.po` **никогда не перезаписываются** —
  недостающие ключи дописываются в существующие файлы (msgmerge-lite),
  поэтому уже сделанные переводы сохраняются;
* `parse_po` понимает комментарии, многострочные строки, экранирование и
  пропускает записи с `#, fuzzy` и пустым `msgstr` (они откатываются на
  английский);
* `write_mo` / `parse_mo` собирают и читают бинарный MO (little- и
  big-endian при чтении, заголовок UTF-8).

```bash
make po         # po/linuxprinterg.pot + скелеты po/{uk,de,fr}.po
make po-build   # locale/<ll>/LC_MESSAGES/linuxprinterg.mo из заполненных .po
```

## Загрузка в рантайме

`set_language()` подхватывает `<base>/<ll>/LC_MESSAGES/linuxprinterg.mo`
как overlay: переведённый ключ побеждает, всё остальное откатывается на
английскую строку словаря. Порядок поиска (побеждает первый найденный):

1. `LINUXPRINTERG_LOCALE_DIR` (переопределение для тестов и переносных
   запусков);
2. `~/.local/share/linuxprinterg/locale`;
3. каталоги `XDG_DATA_DIRS`/locale (для deb/rpm это `/usr/share/locale`);
4. `/app/share/locale` (Flatpak).

Язык выбирается в настройках — авто / RU / EN / UK / DE / FR; «авто»
определяется по `LC_ALL` → `LC_MESSAGES` → `LANG`. RU/EN всегда есть в
словаре, поэтому отсутствующий или битый `.mo` деградирует до английского,
а не ломает интерфейс.

## Как добавить язык по шагам

1. Добавьте язык в `Language` в `i18n.rs` (с `gettext_code()`),
   `LangChoice` в `storage/settings.rs` и строку языка в
   `crates/linuxprinterg/src/pages/settings.rs`;
2. допишите его код в `LANGS` в
   `crates/linuxprinterg-core/src/bin/extract-po.rs`;
3. `make po` — создаёт `po/<ll>.po` со всеми ключами и пустыми `msgstr`;
4. заполните `msgstr` (плейсхолдеры `{x}` должны совпадать со строкой
   `# EN:`);
5. проверьте: `msgfmt --check po/<ll>.po` (gettext используется только для
   этой внешней проверки — никогда при сборке);
6. `make po-build` — соберите `locale/<ll>/LC_MESSAGES/linuxprinterg.mo`;
7. включите `.mo` в дистрибутив: добавьте `locale/<ll>` в
   `packaging/nfpm.yml` и строку `install` в
   `crates/linuxprinterg/data/org.linuxprinterg.LinuxPrinterG.flatpak.yml`
   (см. [PACKAGING.ru.md](PACKAGING.ru.md)).

После изменения словаря — снова `make po`: новые ключи будут дописаны в
существующие `.po`, переводы не тронутся.

## Где лежат каталоги

| Артефакт | Путь |
|---|---|
| исходники | `po/linuxprinterg.pot`, `po/{uk,de,fr}.po` |
| собранные каталоги в репозитории | `locale/<ll>/LC_MESSAGES/linuxprinterg.mo` |
| .deb / .rpm | `/usr/share/locale/<ll>/LC_MESSAGES/linuxprinterg.mo` |
| Flatpak | `/app/share/locale/<ll>/LC_MESSAGES/linuxprinterg.mo` |

Сборка `.deb`/`.rpm` намеренно падает, если `.mo` нет
(`scripts/build-packages.sh` проверяет все три языка).

См. также: [../README.ru.md](../README.ru.md),
[MAINTENANCE.ru.md](MAINTENANCE.ru.md),
[../CONTRIBUTING.ru.md](../CONTRIBUTING.ru.md).
