//! Страница «Печать»: выбор файла (кнопка/перетаскивание), предпросмотр,
//! параметры, отправка в CUPS, очередь с отменой заданий.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk::gdk;
use gtk::gio;
use gtk::pango;

use linuxprinterg_core::Error;
use linuxprinterg_core::domain::{
    JobState, Orientation, PrintColorMode, PrintQuality, PrintRequest, Printer, Sides,
};
use linuxprinterg_core::i18n;
use linuxprinterg_core::services::{convert, cups, ipp, naming, pdf, preview};
use linuxprinterg_core::storage::journal::Category;

use crate::ctx::Ctx;
use crate::window::{PageHandle, confirm, make_handle};

const MEDIA_FALLBACK: &[&str] = &[
    "A4",
    "A5",
    "A3",
    "A6",
    "Letter",
    "Legal",
    "Executive",
    "4x6",
];
const QUALITY_LABELS: &[&str] = &[
    "print.quality.draft",
    "print.quality.normal",
    "print.quality.high",
    "print.quality.photo",
];
const SIDES_LABELS: &[&str] = &["print.sides.off", "print.sides.long", "print.sides.short"];
const COLOR_LABELS: &[&str] = &["print.color.color", "print.color.gray", "print.color.auto"];
const ORIENTATION_LABELS: &[&str] = &[
    "print.orientation.portrait",
    "print.orientation.landscape",
    "print.orientation.reverse_portrait",
    "print.orientation.reverse_landscape",
];
/// Отложенная печать: подписи — ключи i18n, значения ниже в `submit`
/// (CUPS-значения `job-hold-until`).
const HOLD_LABELS: &[&str] = &[
    "print.hold_now",
    "print.hold_day",
    "print.hold_evening",
    "print.hold_night",
    "print.hold_weekend",
];

struct PrintState {
    ctx: Ctx,
    window: adw::ApplicationWindow,
    file: RefCell<Option<PathBuf>>,
    printer_dd: gtk::DropDown,
    copies_spin: gtk::SpinButton,
    page_range: gtk::Entry,
    title: gtk::Entry,
    media_dd: gtk::DropDown,
    tray_dd: gtk::DropDown,
    media_type_dd: gtk::DropDown,
    /// Подгонка под лист: индекс0 = «по умолчанию» (опция не отправляется),
    /// далее — реальные choices принтера; параллельно со списком подписей
    /// хранятся сырые значения CUPS (`print-scaling`).
    scaling_dd: gtk::DropDown,
    scaling_choices: RefCell<Vec<String>>,
    sides_dd: gtk::DropDown,
    color_dd: gtk::DropDown,
    quality_dd: gtk::DropDown,
    orientation_dd: gtk::DropDown,
    hold_dd: gtk::DropDown,
    file_name: gtk::Label,
    file_meta: gtk::Label,
    picture: gtk::Picture,
    spinner: gtk::Spinner,
    print_btn: gtk::Button,
    queue_list: gtk::ListBox,
    queue_stack: gtk::Stack,
    /// Подряд идущие неудачи опроса CUPS (автообновление отключается).
    queue_failures: Cell<u32>,
    /// Токены заданий, находящихся под наблюдением: когда такое задание
    /// завершится, пришлётся системное уведомление (см. detect_completions).
    watched_jobs: RefCell<HashSet<String>>,
}

pub fn build(ctx: &Ctx, window: &adw::ApplicationWindow) -> (gtk::Widget, PageHandle) {
    // --- Заголовок страницы ----------------------------------------------
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&gtk::Label::new(Some(&i18n::tr("nav.print")))));
    let refresh_btn = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text(i18n::tr("common.refresh"))
        .css_classes(["flat"])
        .build();
    header.pack_end(&refresh_btn);
    toolbar.add_top_bar(&header);

    // --- Область файла ----------------------------------------------------
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Contain)
        .can_shrink(true)
        .width_request(170)
        .height_request(230)
        .css_classes(["card"])
        .build();
    let spinner = gtk::Spinner::new();
    spinner.set_visible(false);
    let preview_stack = gtk::Stack::builder().build();
    preview_stack.add_named(&picture, Some("picture"));
    preview_stack.add_named(&spinner, Some("spinner"));

    let file_name = gtk::Label::builder()
        .label(i18n::tr("print.drop_hint"))
        .xalign(0.0)
        .wrap(true)
        .ellipsize(pango::EllipsizeMode::Middle)
        .hexpand(true)
        .build();
    let file_meta = gtk::Label::builder()
        .label("")
        .xalign(0.0)
        .wrap(true)
        .css_classes(["dim-label"])
        .build();
    let open_btn = gtk::Button::builder()
        .label(i18n::tr("print.select_file"))
        .build();
    let clear_btn = gtk::Button::builder()
        .icon_name("edit-clear-symbolic")
        .tooltip_text(i18n::tr("common.clear"))
        .css_classes(["flat"])
        .build();

    let info_col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .build();
    info_col.append(&file_name);
    info_col.append(&file_meta);
    let btn_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    btn_row.append(&open_btn);
    btn_row.append(&clear_btn);
    info_col.append(&btn_row);

    let file_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(16)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(18)
        .margin_end(18)
        .build();
    file_row.append(&preview_stack);
    file_row.append(&info_col);

    // --- Принтер и кнопка печати -----------------------------------------
    let printer_dd = gtk::DropDown::from_strings(&[]);
    printer_dd.set_hexpand(true);
    let print_btn = gtk::Button::builder()
        .label(i18n::tr("print.print_now"))
        .icon_name("printer-symbolic")
        .css_classes(["suggested-action"])
        .build();
    let printer_section = section_label(&i18n::tr("print.printer"));
    let printer_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_start(18)
        .margin_end(18)
        .margin_bottom(6)
        .build();
    printer_row.append(&printer_section);
    printer_row.append(&printer_dd);
    printer_row.append(&print_btn);

    // --- Параметры печати -------------------------------------------------
    let grid = gtk::Grid::builder()
        .row_spacing(10)
        .column_spacing(18)
        .margin_start(18)
        .margin_end(18)
        .margin_bottom(12)
        .build();

    let copies_spin = gtk::SpinButton::with_range(1.0, 999.0, 1.0);
    copies_spin.set_value(ctx.settings.borrow().print.copies as f64);
    let page_range = gtk::Entry::builder()
        .placeholder_text(i18n::tr("print.page_range_hint"))
        .build();
    let title = gtk::Entry::builder()
        .placeholder_text(i18n::tr("print.title"))
        .build();
    let media_dd = gtk::DropDown::from_strings(MEDIA_FALLBACK);
    let tray_dd = gtk::DropDown::from_strings(&[]);
    let media_type_dd = gtk::DropDown::from_strings(&[]);
    let scaling_dd = gtk::DropDown::from_strings(&[&i18n::tr("print.scaling.default")]);
    let sides_dd = translated_dd(SIDES_LABELS);
    let color_dd = translated_dd(COLOR_LABELS);
    let quality_dd = translated_dd(QUALITY_LABELS);
    let orientation_dd = translated_dd(ORIENTATION_LABELS);
    let hold_dd = translated_dd(HOLD_LABELS);

    sides_dd.set_selected(match ctx.settings.borrow().print.sides {
        Sides::OneSided => 0,
        Sides::TwoSidedLongEdge => 1,
        Sides::TwoSidedShortEdge => 2,
    });
    color_dd.set_selected(match ctx.settings.borrow().print.color_mode {
        PrintColorMode::Color => 0,
        PrintColorMode::Grayscale => 1,
        PrintColorMode::Auto => 2,
    });
    quality_dd.set_selected(match ctx.settings.borrow().print.quality {
        PrintQuality::Draft => 0,
        PrintQuality::Normal => 1,
        PrintQuality::High => 2,
        PrintQuality::Photo => 3,
    });
    orientation_dd.set_selected(match ctx.settings.borrow().print.orientation {
        Orientation::Portrait => 0,
        Orientation::Landscape => 1,
        Orientation::ReversePortrait => 2,
        Orientation::ReverseLandscape => 3,
    });

    let mut row = 0;
    add_grid_row(&grid, &mut row, &i18n::tr("print.copies"), &copies_spin);
    add_grid_row(&grid, &mut row, &i18n::tr("print.page_range"), &page_range);
    add_grid_row(&grid, &mut row, &i18n::tr("print.title"), &title);
    add_grid_row(&grid, &mut row, &i18n::tr("print.media"), &media_dd);
    add_grid_row(&grid, &mut row, &i18n::tr("print.tray"), &tray_dd);
    add_grid_row(
        &grid,
        &mut row,
        &i18n::tr("print.media_type"),
        &media_type_dd,
    );
    add_grid_row(&grid, &mut row, &i18n::tr("print.scaling"), &scaling_dd);
    add_grid_row(&grid, &mut row, &i18n::tr("print.sides"), &sides_dd);
    add_grid_row(&grid, &mut row, &i18n::tr("print.color"), &color_dd);
    add_grid_row(&grid, &mut row, &i18n::tr("print.quality"), &quality_dd);
    add_grid_row(
        &grid,
        &mut row,
        &i18n::tr("print.orientation"),
        &orientation_dd,
    );
    add_grid_row(&grid, &mut row, &i18n::tr("print.hold_label"), &hold_dd);

    // Быстрый фотопрофиль: качество «Фото», цвет, односторонняя печать и
    // фотобумага (если принтер сообщает тип носителя).
    let photo_btn = gtk::Button::builder()
        .label(i18n::tr("print.photo_profile"))
        .icon_name("camera-photo-symbolic")
        .tooltip_text(i18n::tr("print.photo_profile"))
        .halign(gtk::Align::Start)
        .build();
    // Пресет фотоформата 4×6/10×15: тот же фотопрофиль + размер листа
    // (подбирается по списку поддерживаемых медиа принтера).
    let photo_size_btn = gtk::Button::builder()
        .label(i18n::tr("print.photo_size"))
        .tooltip_text(i18n::tr("print.photo_size"))
        .halign(gtk::Align::Start)
        .build();
    let profiles_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    profiles_row.append(&photo_btn);
    profiles_row.append(&photo_size_btn);
    add_grid_row(
        &grid,
        &mut row,
        &i18n::tr("print.quick_profiles"),
        &profiles_row,
    );

    // --- Очередь ----------------------------------------------------------
    let queue_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let queue_empty = gtk::Label::builder()
        .label(i18n::tr("print.queue_empty"))
        .css_classes(["dim-label"])
        .margin_top(12)
        .margin_bottom(12)
        .build();
    let queue_stack = gtk::Stack::new();
    queue_stack.add_named(&queue_list, Some("list"));
    queue_stack.add_named(&queue_empty, Some("empty"));
    queue_stack.set_visible_child_name("empty");

    let queue_refresh = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text(i18n::tr("common.refresh"))
        .css_classes(["flat"])
        .build();
    let queue_header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_start(18)
        .margin_end(18)
        .margin_top(6)
        .build();
    queue_header.append(&section_label(&i18n::tr("print.queue")));
    queue_header.append(&queue_refresh);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    content.append(&file_row);
    content.append(&printer_row);
    content.append(&grid);
    content.append(&queue_header);
    content.append(&queue_stack);

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&content)
        .vexpand(true)
        .build();
    toolbar.set_content(Some(&scrolled));

    let state = Rc::new(PrintState {
        ctx: ctx.clone(),
        window: window.clone(),
        file: RefCell::new(None),
        printer_dd: printer_dd.clone(),
        copies_spin,
        page_range,
        title,
        media_dd,
        tray_dd,
        media_type_dd,
        scaling_dd,
        scaling_choices: RefCell::new(vec![String::new()]),
        sides_dd,
        color_dd,
        quality_dd,
        orientation_dd,
        hold_dd,
        file_name,
        file_meta,
        picture,
        spinner,
        print_btn: print_btn.clone(),
        queue_list,
        queue_stack,
        queue_failures: Cell::new(0),
        watched_jobs: RefCell::new(HashSet::new()),
    });

    // --- Сигналы ----------------------------------------------------------
    {
        let s = state.clone();
        open_btn.connect_clicked(move |_| s.choose_file());
    }
    {
        let s = state.clone();
        clear_btn.connect_clicked(move |_| s.clear_file());
    }
    {
        let s = state.clone();
        print_btn.connect_clicked(move |_| s.submit());
    }
    {
        let s = state.clone();
        printer_dd.connect_selected_notify(move |_| s.load_printer_options());
    }
    {
        let s = state.clone();
        photo_btn.connect_clicked(move |_| s.apply_photo_profile());
    }
    {
        let s = state.clone();
        photo_size_btn.connect_clicked(move |_| s.apply_photo_size_preset());
    }
    {
        let s = state.clone();
        // Ручной сброс счётчика неудач: автообновление снова заработает.
        queue_refresh.connect_clicked(move |_| {
            s.queue_failures.set(0);
            s.refresh_queue();
        });
    }
    // Перетаскивание файлов на область файла.
    {
        let s = state.clone();
        let dnd = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
        dnd.connect_drop(move |_, value, _, _| {
            if let Ok(list) = value.get::<gdk::FileList>() {
                if let Some(file) = list.files().into_iter().next() {
                    if let Some(path) = file.path() {
                        s.set_file(path);
                        return true;
                    }
                }
            }
            false
        });
        file_row.add_controller(dnd);
    }

    // --- Первоначальная загрузка ------------------------------------------
    let refresh_state = state.clone();
    let refresh = move || refresh_state.refresh();
    let handle = make_handle(refresh.clone());
    refresh();
    state.refresh_queue();

    // Автообновление очереди каждые3 секунды (см. auto_refresh_queue:
    // после неудач опрос прекращается до ручного «Обновить»).
    {
        let s = state.clone();
        glib::timeout_add_seconds_local(3, move || {
            s.auto_refresh_queue();
            glib::ControlFlow::Continue
        });
    }

    (toolbar.upcast::<gtk::Widget>(), handle)
}

fn translated(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|k| i18n::tr(k)).collect()
}

/// `DropDown` с локализованными подписями из словаря ключей.
fn translated_dd(keys: &[&str]) -> gtk::DropDown {
    let strings = translated(keys);
    let refs: Vec<&str> = strings.iter().map(String::as_str).collect();
    gtk::DropDown::from_strings(&refs)
}

fn section_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .css_classes(["heading"])
        .build()
}

fn add_grid_row(grid: &gtk::Grid, row: &mut i32, label: &str, widget: &impl IsA<gtk::Widget>) {
    let l = gtk::Label::builder()
        .label(label)
        .xalign(0.0)
        .css_classes(["dim-label"])
        .build();
    grid.attach(&l, 0, *row, 1, 1);
    widget.set_hexpand(true);
    grid.attach(widget, 1, *row, 1, 1);
    *row += 1;
}

fn human_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["Б", "КБ", "МБ", "ГБ"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

impl PrintState {
    /// Обновить список принтеров и очередь.
    fn refresh(self: &Rc<Self>) {
        let s = self.clone();
        self.ctx.refresh_devices(Some(Box::new(move || {
            s.fill_printers();
        })));
        self.refresh_queue();
    }

    fn fill_printers(self: &Rc<Self>) {
        let printers = self.ctx.printers.borrow().clone();
        let names: Vec<String> = printers
            .iter()
            .map(|p| format!("{} — {}", p.name, state_label(&p.state)))
            .collect();
        if names.is_empty() {
            set_items(&self.printer_dd, &[i18n::tr("print.no_printer")]);
            self.print_btn.set_sensitive(false);
            return;
        }
        self.print_btn.set_sensitive(true);
        set_items(&self.printer_dd, &names);
        // Выбор по умолчанию.
        let default_name = self.ctx.effective_default_printer();
        let idx = default_name
            .and_then(|name| printers.iter().position(|p| p.name == name))
            .unwrap_or(0) as u32;
        self.printer_dd.set_selected(idx);
        self.load_printer_options();
    }

    fn selected_printer(&self) -> Option<Printer> {
        let printers = self.ctx.printers.borrow();
        let idx = self.printer_dd.selected() as usize;
        printers.get(idx).cloned()
    }

    /// Загрузить реальные опции принтера (кассеты, носители, типы бумаги).
    fn load_printer_options(self: &Rc<Self>) {
        let Some(printer) = self.selected_printer() else {
            return;
        };
        // Применяем медиа по умолчанию из настроек, если оно есть в списке.
        let default_media = self.ctx.settings.borrow().print.media.clone();
        let s = self.clone();
        crate::tasks::run_async(
            move || cups::printer_options(&printer.name),
            move |result| {
                let options = result.ok();
                let media: Vec<String> = options
                    .as_ref()
                    .and_then(|o| o.get("PageSize"))
                    .cloned()
                    .unwrap_or_else(|| MEDIA_FALLBACK.iter().map(|s| s.to_string()).collect());
                let trays: Vec<String> = options
                    .as_ref()
                    .and_then(|o| o.get("InputSlot"))
                    .cloned()
                    .unwrap_or_else(|| vec![i18n::tr("common.auto")]);
                let media_types: Vec<String> = options
                    .as_ref()
                    .and_then(|o| o.get("MediaType"))
                    .cloned()
                    .unwrap_or_default();

                if let Some(idx) = media.iter().position(|m| m == &default_media) {
                    s.media_dd.set_selected(idx as u32);
                } else if !media.is_empty() {
                    s.media_dd.set_selected(0);
                }
                set_items(&s.media_dd, &media);
                set_items(&s.tray_dd, &trays);
                if media_types.is_empty() {
                    set_items(&s.media_type_dd, &[i18n::tr("common.none")]);
                } else {
                    set_items(&s.media_type_dd, &media_types);
                }
                // Подгонка под лист: реальные choices принтера; если опция
                // не заявлена — остаётся только «по умолчанию» (ничего не
                // отправляем).
                let scalings: Vec<String> = options
                    .as_ref()
                    .and_then(|o| o.get("print-scaling"))
                    .cloned()
                    .unwrap_or_default();
                let mut labels = vec![i18n::tr("print.scaling.default")];
                let mut choices = vec![String::new()];
                for c in scalings {
                    labels.push(match scaling_label_key(&c) {
                        Some(k) => i18n::tr(k),
                        None => c.clone(),
                    });
                    choices.push(c);
                }
                set_items(&s.scaling_dd, &labels);
                s.scaling_dd.set_selected(0);
                *s.scaling_choices.borrow_mut() = choices;
            },
        );
    }

    fn choose_file(self: &Rc<Self>) {
        let dialog = gtk::FileDialog::builder()
            .title(i18n::tr("print.select_file"))
            .build();
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("PDF / images / documents"));
        for mime in [
            "application/pdf",
            "image/png",
            "image/jpeg",
            "image/tiff",
            "image/webp",
            "image/bmp",
            "text/plain",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "application/vnd.oasis.opendocument.text",
        ] {
            filter.add_mime_type(mime);
        }
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        dialog.set_filters(Some(&filters));
        dialog.set_default_filter(Some(&filter));

        let s = self.clone();
        dialog.open(
            Some(&self.window),
            gio::Cancellable::NONE,
            move |result| match result {
                Ok(file) => {
                    if let Some(path) = file.path() {
                        s.set_file(path);
                    }
                }
                Err(e) if e.matches(gtk::DialogError::Dismissed) => {}
                Err(e) => s.ctx.toast_error(&Error::msg(e.to_string())),
            },
        );
    }

    fn set_file(self: &Rc<Self>, path: PathBuf) {
        if let Err(e) = cups::check_printable_file(&path) {
            self.ctx.toast_error(&e);
            return;
        }
        let meta = std::fs::metadata(&path).ok();
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let ext = naming::extension_of(&path);
        self.file_name.set_label(&path.to_string_lossy());
        self.file_meta
            .set_label(&format!(".{ext} · {}", human_size(size)));

        // Предупреждение о формате, требующем конвертации.
        if cups::is_office_document(&path) && !convert::available() {
            self.file_meta
                .set_label(&format!(".{ext} · {}", i18n::tr("print.convert_missing")));
        }

        if self.title.text().is_empty() {
            self.title.set_text(&naming::stem_of(&path));
        }
        *self.file.borrow_mut() = Some(path.clone());
        self.show_preview(path);
    }

    fn clear_file(self: &Rc<Self>) {
        *self.file.borrow_mut() = None;
        self.file_name.set_label(&i18n::tr("print.drop_hint"));
        self.file_meta.set_label("");
        self.picture.set_paintable(None::<&gdk::Texture>);
        self.title.set_text("");
    }

    /// Показать предпросмотр: изображения — напрямую, PDF — через pdftoppm.
    fn show_preview(self: &Rc<Self>, path: PathBuf) {
        let is_pdf = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("pdf"))
            .unwrap_or(false);

        if !is_pdf {
            match gdk::Texture::from_filename(&path) {
                Ok(texture) => {
                    self.picture.set_paintable(Some(&texture));
                    self.spinner.set_visible(false);
                    self.picture.set_visible(true);
                }
                Err(e) => {
                    self.ctx.toast(&i18n::trf(
                        "print.preview_failed",
                        &[("msg", &e.to_string())],
                    ));
                }
            }
            return;
        }

        self.spinner.set_visible(true);
        self.spinner.start();
        self.picture.set_visible(false);
        let s = self.clone();
        let cache = linuxprinterg_core::storage::cache_dir().join("preview");
        crate::tasks::run_async(
            move || preview::pdf_first_page(&path, &cache, 96),
            move |result| {
                s.spinner.stop();
                s.spinner.set_visible(false);
                s.picture.set_visible(true);
                match result {
                    Ok(png) => match gdk::Texture::from_filename(&png) {
                        Ok(texture) => s.picture.set_paintable(Some(&texture)),
                        Err(e) => s.ctx.toast(&i18n::trf(
                            "print.preview_failed",
                            &[("msg", &e.to_string())],
                        )),
                    },
                    Err(e) if e.is_cancelled() => {}
                    Err(e) => {
                        s.ctx.toast(&i18n::trf(
                            "print.preview_failed",
                            &[("msg", &e.ui_message())],
                        ));
                    }
                }
            },
        );
    }

    /// Собрать запрос печати и отправить в CUPS (в фоне).
    fn submit(self: &Rc<Self>) {
        let Some(file) = self.file.borrow().clone() else {
            self.ctx.toast(&i18n::tr("print.drop_hint"));
            return;
        };
        let Some(printer) = self.selected_printer() else {
            self.ctx.toast(&i18n::tr("print.no_printer"));
            return;
        };

        let media = selected_text(&self.media_dd);
        let tray = selected_text(&self.tray_dd);
        let media_type =
            selected_text(&self.media_type_dd).filter(|t| t != &i18n::tr("common.none"));
        let sides = match self.sides_dd.selected() {
            1 => Sides::TwoSidedLongEdge,
            2 => Sides::TwoSidedShortEdge,
            _ => Sides::OneSided,
        };
        let color_mode = match self.color_dd.selected() {
            1 => PrintColorMode::Grayscale,
            2 => PrintColorMode::Auto,
            _ => PrintColorMode::Color,
        };
        let quality = match self.quality_dd.selected() {
            0 => PrintQuality::Draft,
            2 => PrintQuality::High,
            3 => PrintQuality::Photo,
            _ => PrintQuality::Normal,
        };
        let orientation = match self.orientation_dd.selected() {
            1 => Orientation::Landscape,
            2 => Orientation::ReversePortrait,
            3 => Orientation::ReverseLandscape,
            _ => Orientation::Portrait,
        };
        let page_range = self.page_range.text().to_string();
        let page_range = if page_range.trim().is_empty() {
            None
        } else {
            Some(page_range)
        };
        let title = {
            let t = self.title.text().to_string();
            if t.trim().is_empty() {
                naming::stem_of(&file)
            } else {
                t
            }
        };
        let copies = self.copies_spin.value().max(1.0) as u32;
        // Отложенная печать: значения CUPS `job-hold-until`
        // (индексы соответствуют HOLD_LABELS).
        let hold_until = match self.hold_dd.selected() {
            1 => Some("day-time".to_string()),
            2 => Some("evening".to_string()),
            3 => Some("night".to_string()),
            4 => Some("weekend".to_string()),
            _ => None,
        };
        // Подгонка под лист: индекс0 — «по умолчанию» (пусто = не отправлять).
        let print_scaling = self
            .scaling_choices
            .borrow()
            .get(self.scaling_dd.selected() as usize)
            .cloned()
            .filter(|s| !s.is_empty());

        let mut req = PrintRequest {
            printer: printer.name.clone(),
            file: file.clone(),
            title,
            copies,
            page_range,
            media: media.clone(),
            sides,
            color_mode,
            quality,
            orientation,
            tray,
            media_type,
            hold_until,
            print_scaling,
        };

        self.print_btn.set_sensitive(false);
        let s = self.clone();
        crate::tasks::run_async(
            move || {
                // Офисный документ: конвертация через LibreOffice headless.
                let effective = if cups::is_office_document(&req.file) {
                    if !convert::available() {
                        return Err(Error::msg(i18n::tr("print.convert_missing")));
                    }
                    let out_dir = linuxprinterg_core::storage::cache_dir().join("converted");
                    convert::to_pdf(&req.file, &out_dir)?
                } else if naming::extension_of(&req.file) == "webp" {
                    // WebP понимают не все CUPS-фильтры: собираем PDF
                    // через image-энкодер и печатаем уже его.
                    let out_dir = linuxprinterg_core::storage::cache_dir().join("converted");
                    let out = out_dir.join(format!("{}.pdf", naming::stem_of(&req.file)));
                    pdf::image_file_to_pdf(&req.file, &out)?;
                    out
                } else {
                    req.file.clone()
                };
                req.file = effective;
                let job = cups::submit(&req)?;
                Ok((job, req.file.clone(), req.printer.clone()))
            },
            move |result| {
                s.print_btn.set_sensitive(true);
                match result {
                    Ok((job, effective, printer)) => {
                        let msg = if job == 0 {
                            i18n::trf("journal.entry.print_submitted", &[("device", &printer)])
                        } else {
                            i18n::trf("print.job_submitted", &[("id", &job.to_string())])
                        };
                        s.ctx.toast(&msg);
                        s.ctx.journal.info(
                            Category::Print,
                            i18n::trf("journal.entry.print_submitted", &[("device", &printer)]),
                        );
                        if effective != file {
                            s.ctx.journal.info(
                                Category::Print,
                                i18n::trf(
                                    "journal.entry.convert_done",
                                    &[("path", &effective.to_string_lossy())],
                                ),
                            );
                        }
                        s.refresh_queue();
                    }
                    Err(e) if e.is_cancelled() => {}
                    Err(e) => s.ctx.toast_error(&e),
                }
            },
        );
    }

    /// Обновить очередь печати.
    fn refresh_queue(self: &Rc<Self>) {
        let s = self.clone();
        crate::tasks::run_async(cups::queue, move |result| match result {
            Ok(jobs) => {
                s.queue_failures.set(0);
                s.detect_completions(&jobs);
                s.fill_queue(&jobs);
            }
            Err(e) => {
                s.queue_failures
                    .set(s.queue_failures.get().saturating_add(1));
                // Пустая очередь при недоступном CUPS не должна громить UI.
                tracing::warn!("очередь недоступна: {e}");
                s.fill_queue(&[]);
            }
        });
    }

    /// Автообновление очереди: опрашивает CUPS каждые несколько секунд,
    /// но прекращает после двух неудач подряд (CUPS недоступен/остановлен).
    /// Снова включается после ручного «Обновить» или успешного опроса.
    fn auto_refresh_queue(self: &Rc<Self>) {
        if self.queue_failures.get() >= 2 {
            return;
        }
        self.refresh_queue();
    }

    /// Отслеживание завершений заданий (вызывается только при успешном
    /// опросе): активные задания становятся «наблюдаемыми», а перешедшие в
    /// терминальное состояние — порождают системное уведомление
    /// (freedesktop Notifications, D-Bus через `gio`) и запись в журнал.
    ///
    /// Опрос ошибся → вызов пропускается: пустой список из-за сбоя CUPS
    /// не должен разбудить уведомления обо всех заданиях сразу.
    fn detect_completions(&self, jobs: &[linuxprinterg_core::domain::Job]) {
        // Всё под одним borrow вычисляется здесь: ниже нужен borrow_mut.
        let (finished, gone, next) = {
            let watched = self.watched_jobs.borrow();
            let finished: Vec<(&str, Option<String>)> = jobs
                .iter()
                .filter(|job| job.state.is_terminal() && watched.contains(&job.token))
                .map(|job| {
                    (
                        job.token.as_str(),
                        job_state_label(job).map(|(text, _)| text),
                    )
                })
                .collect();
            // Были под наблюдением, но из списка исчезли (очищена история
            // или задание удалено извне) — сообщаем без статуса.
            let gone: Vec<String> = watched
                .iter()
                .filter(|t| !jobs.iter().any(|j| &j.token == *t))
                .cloned()
                .collect();
            let next: HashSet<String> = jobs
                .iter()
                .filter(|j| !j.state.is_terminal())
                .map(|j| j.token.clone())
                .collect();
            (finished, gone, next)
        };

        for (token, state_text) in finished {
            let title = i18n::trf("notify.job_finished", &[("job", token)]);
            let body = state_text.unwrap_or_else(|| i18n::tr("print.job_done"));
            self.ctx
                .notify(&format!("job-finished-{token}"), &title, Some(&body));
            self.ctx.journal.info(Category::Print, title);
        }
        for token in gone {
            let text = i18n::trf("notify.job_gone", &[("job", &token)]);
            self.ctx
                .notify(&format!("job-finished-{token}"), &text, None);
            self.ctx.journal.info(Category::Print, text);
        }
        *self.watched_jobs.borrow_mut() = next;
    }

    /// Подтверждение + IPP-операция над заданием (Hold/Release/Restart)
    /// и обновление очереди/журнала по успеху.
    /// `keys` — (заголовок подтверждения, текст подтверждения, результат).
    fn spawn_job_op(
        self: &Rc<Self>,
        op: ipp::JobOp,
        token: &str,
        job_id: u32,
        printer_uri: String,
        keys: (&str, &str, &str),
    ) {
        let (title_key, body_key, done_key) = keys;
        let token = token.to_string();
        let done_key = done_key.to_string();
        let window = self.window.clone();
        let s = self.clone();
        confirm(
            &window,
            &i18n::tr(title_key),
            &i18n::trf(body_key, &[("job", &token)]),
            move || {
                let s = s.clone();
                let token_cb = token.clone();
                crate::tasks::run_async(
                    move || ipp::job_op(op, &printer_uri, job_id),
                    move |result| match result {
                        Ok(()) => {
                            s.ctx.toast(&i18n::trf(&done_key, &[("job", &token_cb)]));
                            s.ctx
                                .journal
                                .info(Category::Print, i18n::trf(&done_key, &[("job", &token_cb)]));
                            s.refresh_queue();
                        }
                        Err(e) => s.ctx.toast_error(&e),
                    },
                );
            },
        );
    }

    /// Быстрый фотопрофиль печати.
    fn apply_photo_profile(&self) {
        self.quality_dd.set_selected(3); // print.quality.photo
        self.color_dd.set_selected(0); // цвет
        self.sides_dd.set_selected(0); // односторонняя печать
        // Тип носителя «Photo»/«Glossy», если принтер его поддерживает.
        if let Some(model) = self.media_type_dd.model() {
            for i in 0..model.n_items() {
                let text = model
                    .item(i)
                    .and_then(|o| o.downcast::<gtk::StringObject>().ok())
                    .map(|s| s.string().to_ascii_lowercase());
                if let Some(lower) = text {
                    if lower.contains("photo") || lower.contains("glossy") {
                        self.media_type_dd.set_selected(i);
                        break;
                    }
                }
            }
        }
        self.ctx.toast(&i18n::tr("print.photo_profile_applied"));
    }

    /// Пресет фотоформата 4×6/10×15: фотопрофиль + размер листа. Названия
    /// формата у драйверов различаются (`4x6`, `10x15`, `100x150`) — ищем
    /// по списку медиа принтера: точное совпадение, затем без borderless,
    /// затем любое вхождение.
    fn apply_photo_size_preset(self: &Rc<Self>) {
        const ALIASES: [&str; 3] = ["4x6", "10x15", "100x150"];
        self.apply_photo_profile();
        let Some(model) = self.media_dd.model() else {
            self.ctx.toast(&i18n::tr("print.photo_size_missing"));
            return;
        };
        let texts: Vec<String> = (0..model.n_items())
            .filter_map(|i| {
                model
                    .item(i)
                    .and_then(|o| o.downcast::<gtk::StringObject>().ok())
                    .map(|s| s.string().to_string())
            })
            .collect();
        let find = |pred: &dyn Fn(&str) -> bool| -> Option<u32> {
            texts
                .iter()
                .position(|t| pred(&t.to_ascii_lowercase()))
                .map(|i| i as u32)
        };
        let exact = find(&|low| ALIASES.contains(&low));
        let plain = exact.or_else(|| {
            find(&|low| ALIASES.iter().any(|a| low.contains(*a)) && !low.contains("borderless"))
        });
        let hit = plain.or_else(|| find(&|low| ALIASES.iter().any(|a| low.contains(*a))));
        match hit {
            Some(i) => {
                self.media_dd.set_selected(i);
                self.ctx.toast(&i18n::trf(
                    "print.photo_size_applied",
                    &[("size", &texts[i as usize])],
                ));
            }
            None => self.ctx.toast(&i18n::tr("print.photo_size_missing")),
        }
    }

    fn fill_queue(self: &Rc<Self>, jobs: &[linuxprinterg_core::domain::Job]) {
        // Счётчик активных заданий — в сайдбаре («Печать»).
        let active = jobs.iter().filter(|j| !j.state.is_terminal()).count() as u32;
        self.ctx.set_queue_badge(active);

        while let Some(child) = self.queue_list.row_at_index(0) {
            self.queue_list.remove(&child);
        }
        if jobs.is_empty() {
            self.queue_stack.set_visible_child_name("empty");
            return;
        }
        self.queue_stack.set_visible_child_name("list");
        for job in jobs {
            let token = job.token.clone();
            let name_label = gtk::Label::builder()
                .label(&token)
                .xalign(0.0)
                .ellipsize(pango::EllipsizeMode::End)
                .hexpand(true)
                .build();
            let meta = gtk::Label::builder()
                .label(format!("{} · {}", job.owner, human_size(job.size)))
                .css_classes(["dim-label"])
                .xalign(1.0)
                .build();
            let row_box = gtk::Box::builder()
                .orientation(gtk::Orientation::Horizontal)
                .spacing(10)
                .margin_top(8)
                .margin_bottom(8)
                .margin_start(12)
                .margin_end(8)
                .build();
            row_box.append(&name_label);
            row_box.append(&meta);

            // Статус задания из IPP: «печатается», «выполнено», «сбой»… +
            // прогресс листов, если принтер их сообщает.
            if let Some((text, css)) = job_state_label(job) {
                let status = gtk::Label::builder()
                    .label(text)
                    .css_classes([css, "caption"])
                    .xalign(1.0)
                    .build();
                row_box.append(&status);
            }
            if job.state.is_terminal() {
                // Завершённое задание: статус + «Перезапустить» (печать
                // повторится — с подтверждением).
                let restart_btn = gtk::Button::builder()
                    .icon_name("view-refresh-symbolic")
                    .tooltip_text(i18n::tr("print.restart_job"))
                    .css_classes(["flat"])
                    .build();
                {
                    let s = self.clone();
                    let token = token.clone();
                    let id = job.id;
                    let uri = job.resolve_printer_uri();
                    restart_btn.connect_clicked(move |_| {
                        s.spawn_job_op(
                            ipp::JobOp::Restart,
                            &token,
                            id,
                            uri.clone(),
                            (
                                "print.restart_job",
                                "print.restart_job_confirm",
                                "print.job_restarted",
                            ),
                        );
                    });
                }
                row_box.append(&restart_btn);
                self.queue_list
                    .append(&gtk::ListBoxRow::builder().child(&row_box).build());
                continue;
            }

            // Активное задание: «Продолжить» (held) либо «Приостановить».
            let (icon, tip_key, op, title_key, body_key, done_key) =
                if job.state == linuxprinterg_core::domain::JobState::Held {
                    (
                        "media-playback-start-symbolic",
                        "print.resume_job",
                        ipp::JobOp::Release,
                        "print.resume_job",
                        "print.resume_job_confirm",
                        "print.job_resumed",
                    )
                } else {
                    (
                        "media-playback-pause-symbolic",
                        "print.hold_job",
                        ipp::JobOp::Hold,
                        "print.hold_job",
                        "print.hold_job_confirm",
                        "print.job_held",
                    )
                };
            let action_btn = gtk::Button::builder()
                .icon_name(icon)
                .tooltip_text(i18n::tr(tip_key))
                .css_classes(["flat"])
                .build();
            {
                let s = self.clone();
                let token = token.clone();
                let id = job.id;
                let uri = job.resolve_printer_uri();
                action_btn.connect_clicked(move |_| {
                    s.spawn_job_op(op, &token, id, uri.clone(), (title_key, body_key, done_key));
                });
            }
            row_box.append(&action_btn);

            let cancel_btn = gtk::Button::builder()
                .icon_name("edit-delete-symbolic")
                .tooltip_text(i18n::tr("print.cancel_job"))
                .css_classes(["flat"])
                .build();
            {
                let s = self.clone();
                let token = token.clone();
                let window = self.window.clone();
                cancel_btn.connect_clicked(move |_| {
                    let s = s.clone();
                    let token_for_confirm = token.clone();
                    confirm(
                        &window,
                        &i18n::tr("print.cancel_job"),
                        &i18n::trf("print.cancel_job_confirm", &[("job", &token)]),
                        move || {
                            let s = s.clone();
                            let token_cb = token_for_confirm.clone();
                            crate::tasks::run_async(
                                move || cups::cancel_job(&token_for_confirm),
                                move |result| match result {
                                    Ok(()) => {
                                        s.ctx.toast(&i18n::trf(
                                            "print.job_cancelled",
                                            &[("job", &token_cb)],
                                        ));
                                        s.ctx.journal.info(
                                            Category::Print,
                                            i18n::trf(
                                                "journal.entry.print_cancelled",
                                                &[("job", &token_cb)],
                                            ),
                                        );
                                        s.refresh_queue();
                                    }
                                    Err(e) => s.ctx.toast_error(&e),
                                },
                            );
                        },
                    );
                });
            }
            row_box.append(&cancel_btn);
            self.queue_list
                .append(&gtk::ListBoxRow::builder().child(&row_box).build());
        }
    }
}

fn set_items(dd: &gtk::DropDown, items: &[String]) {
    let model = gtk::StringList::new(&[]);
    for item in items {
        model.append(item);
    }
    dd.set_model(Some(&model));
}

/// Соответствие реальных значений `print-scaling` ключам i18n
/// (неизвестные значения показываем как есть).
fn scaling_label_key(choice: &str) -> Option<&'static str> {
    match choice {
        "auto" => Some("print.scaling.auto"),
        "auto-fit" => Some("print.scaling.auto_fit"),
        "fit" => Some("print.scaling.fit"),
        "fill" => Some("print.scaling.fill"),
        "shrink" => Some("print.scaling.shrink"),
        "none" => Some("print.scaling.none"),
        _ => None,
    }
}

fn selected_text(dd: &gtk::DropDown) -> Option<String> {
    let idx = dd.selected();
    let model = dd.model()?;
    model
        .item(idx)
        .and_then(|o| o.downcast::<gtk::StringObject>().ok())
        .map(|s| s.string().to_string())
}

fn state_label(state: &linuxprinterg_core::domain::PrinterState) -> String {
    use linuxprinterg_core::domain::PrinterState;
    match state {
        PrinterState::Idle => i18n::tr("devices.state.idle"),
        PrinterState::Printing => i18n::tr("devices.state.printing"),
        PrinterState::Disabled => i18n::tr("devices.state.disabled"),
        PrinterState::Unknown => i18n::tr("devices.state.unknown"),
    }
}

/// Подпись состояния задания: (текст, CSS-класс).
/// `Pending`/`Unknown` — без подписи (`None`): в очереди это очевидно.
/// Для `Processing` с известным числом листов — прогресс «стр. X из Y».
fn job_state_label(job: &linuxprinterg_core::domain::Job) -> Option<(String, &'static str)> {
    use JobState::*;
    let text = match job.state {
        Pending | Unknown => return None,
        Processing if job.sheets_total > 0 => i18n::trf(
            "print.job_state.processing_of",
            &[
                ("done", &job.sheets_done.to_string()),
                ("total", &job.sheets_total.to_string()),
            ],
        ),
        Processing => i18n::tr("print.job_state.processing"),
        Held => i18n::tr("print.job_state.held"),
        Stopped => i18n::tr("print.job_state.stopped"),
        Completed => i18n::tr("print.job_done"),
        Canceled => i18n::tr("print.job_state.canceled"),
        Aborted => i18n::tr("print.job_state.aborted"),
    };
    let css = match job.state {
        Pending | Unknown => return None,
        Processing => "accent",
        Held | Stopped => "warning",
        Completed => "success",
        Canceled => "dim-label",
        Aborted => "danger",
    };
    Some((text, css))
}
