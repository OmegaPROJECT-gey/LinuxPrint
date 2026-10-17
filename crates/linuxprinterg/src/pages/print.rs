//! Страница «Печать»: выбор файла (кнопка/перетаскивание), предпросмотр,
//! параметры, отправка в CUPS, очередь с отменой заданий.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk::gdk;
use gtk::gio;
use gtk::pango;

use linuxprinterg_core::Error;
use linuxprinterg_core::domain::{
    Orientation, PrintColorMode, PrintQuality, PrintRequest, Printer, Sides,
};
use linuxprinterg_core::i18n;
use linuxprinterg_core::services::{convert, cups, naming, preview};
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
    sides_dd: gtk::DropDown,
    color_dd: gtk::DropDown,
    quality_dd: gtk::DropDown,
    orientation_dd: gtk::DropDown,
    file_name: gtk::Label,
    file_meta: gtk::Label,
    picture: gtk::Picture,
    spinner: gtk::Spinner,
    print_btn: gtk::Button,
    queue_list: gtk::ListBox,
    queue_stack: gtk::Stack,
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
    let sides_dd = translated_dd(SIDES_LABELS);
    let color_dd = translated_dd(COLOR_LABELS);
    let quality_dd = translated_dd(QUALITY_LABELS);
    let orientation_dd = translated_dd(ORIENTATION_LABELS);

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
    add_grid_row(&grid, &mut row, &i18n::tr("print.sides"), &sides_dd);
    add_grid_row(&grid, &mut row, &i18n::tr("print.color"), &color_dd);
    add_grid_row(&grid, &mut row, &i18n::tr("print.quality"), &quality_dd);
    add_grid_row(
        &grid,
        &mut row,
        &i18n::tr("print.orientation"),
        &orientation_dd,
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
        sides_dd,
        color_dd,
        quality_dd,
        orientation_dd,
        file_name,
        file_meta,
        picture,
        spinner,
        print_btn: print_btn.clone(),
        queue_list,
        queue_stack,
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
        queue_refresh.connect_clicked(move |_| s.refresh_queue());
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
            Ok(jobs) => s.fill_queue(&jobs),
            Err(e) => {
                // Пустая очередь при недоступном CUPS не должна громить UI.
                tracing::warn!("очередь недоступна: {e}");
                s.fill_queue(&[]);
            }
        });
    }

    fn fill_queue(self: &Rc<Self>, jobs: &[linuxprinterg_core::domain::Job]) {
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
