//! Страница «Сканирование»: выбор сканера, параметры, предпросмотр с
//! кадрированием и поворотом, сохранение в PDF/PNG/JPEG/TIFF/WebP, OCR.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use gtk::gdk;
use gtk::gio;
use gtk::pango;

use linuxprinterg_core::Error;
use linuxprinterg_core::domain::{
    ColorMode, OutputFormat, RectMm, Rotation, ScanRequest, ScannerOptions,
};
use linuxprinterg_core::i18n;
use linuxprinterg_core::services::naming::{self, NameCtx};
use linuxprinterg_core::services::pdf::PdfWriter;
use linuxprinterg_core::services::proc::CancelHandle;
use linuxprinterg_core::services::sane::{self, ScanProgress};
use linuxprinterg_core::services::tiff::TiffWriter;
use linuxprinterg_core::services::{ocr, save};
use linuxprinterg_core::storage::journal::Category;
use linuxprinterg_core::storage::settings::Settings;

use crate::ctx::Ctx;
use crate::tasks;
use crate::window::{PageHandle, make_handle};

const FORMAT_NAMES: &[&str] = &["PDF", "PNG", "JPEG", "TIFF", "WebP"];
const ROTATION_LABELS: &[&str] = &["0°", "90°", "180°", "270°"];

/// Текущее состояние сканирования (главный поток).
pub struct Runtime {
    cancel: Option<CancelHandle>,
    busy: bool,
    options: Option<ScannerOptions>,
}

pub struct ScanState {
    pub ctx: Ctx,
    pub window: adw::ApplicationWindow,
    pub scanner_dd: gtk::DropDown,
    pub source_dd: gtk::DropDown,
    pub res_dd: gtk::DropDown,
    pub mode_dd: gtk::DropDown,
    pub pages_spin: gtk::SpinButton,
    pub rotation_dd: gtk::DropDown,
    pub crop_x: gtk::SpinButton,
    pub crop_y: gtk::SpinButton,
    pub crop_w: gtk::SpinButton,
    pub crop_h: gtk::SpinButton,
    pub format_dd: gtk::DropDown,
    pub template_entry: gtk::Entry,
    pub folder_row_label: gtk::Label,
    pub ocr_switch: gtk::Switch,
    pub ocr_lang_dd: gtk::DropDown,
    pub preview_btn: gtk::Button,
    pub scan_btn: gtk::Button,
    pub cancel_btn: gtk::Button,
    pub progress: gtk::ProgressBar,
    pub progress_label: gtk::Label,
    pub picture: gtk::Picture,
    pub runtime: RefCell<Runtime>,
}

pub fn build(ctx: &Ctx, window: &adw::ApplicationWindow) -> (gtk::Widget, PageHandle) {
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&gtk::Label::new(Some(&i18n::tr("nav.scan")))));
    let refresh_btn = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text(i18n::tr("common.refresh"))
        .css_classes(["flat"])
        .build();
    header.pack_end(&refresh_btn);
    toolbar.add_top_bar(&header);

    // --- Сканер и параметры -----------------------------------------------
    let scanner_dd = gtk::DropDown::from_strings(&[]);
    scanner_dd.set_hexpand(true);
    let source_dd = gtk::DropDown::from_strings(&[]);
    let res_dd = gtk::DropDown::from_strings(&["75", "100", "150", "200", "300", "600", "1200"]);
    let mode_dd = gtk::DropDown::from_strings(&[]);
    let pages_spin = gtk::SpinButton::with_range(0.0, 999.0, 1.0);
    let rotation_dd = gtk::DropDown::from_strings(ROTATION_LABELS);
    let format_dd = gtk::DropDown::from_strings(FORMAT_NAMES);
    let template_entry = gtk::Entry::builder()
        .text(&ctx.settings.borrow().scan.template)
        .placeholder_text(i18n::tr("scan.template_hint"))
        .build();
    let folder_btn = gtk::Button::builder()
        .icon_name("folder-open-symbolic")
        .tooltip_text(i18n::tr("common.browse"))
        .css_classes(["flat"])
        .build();
    let folder_row_label = gtk::Label::builder()
        .label(&ctx.settings.borrow().scan.scan_dir)
        .xalign(0.0)
        .ellipsize(pango::EllipsizeMode::Middle)
        .hexpand(true)
        .tooltip_text(&ctx.settings.borrow().scan.scan_dir)
        .build();
    let ocr_switch = gtk::Switch::builder()
        .valign(gtk::Align::Center)
        .active(ctx.settings.borrow().scan.ocr_enabled)
        .build();
    let ocr_lang_dd = gtk::DropDown::from_strings(&["rus+eng", "rus", "eng"]);

    let grid = gtk::Grid::builder()
        .row_spacing(10)
        .column_spacing(18)
        .margin_start(18)
        .margin_end(18)
        .margin_top(12)
        .margin_bottom(12)
        .build();
    let mut row = 0;
    add_grid_row(&grid, &mut row, &i18n::tr("scan.scanner"), &scanner_dd);
    add_grid_row(&grid, &mut row, &i18n::tr("scan.source"), &source_dd);
    add_grid_row(&grid, &mut row, &i18n::tr("scan.resolution"), &res_dd);
    add_grid_row(&grid, &mut row, &i18n::tr("scan.mode"), &mode_dd);
    add_grid_row(&grid, &mut row, &i18n::tr("scan.pages"), &pages_spin);
    add_grid_row(&grid, &mut row, &i18n::tr("scan.rotation"), &rotation_dd);

    // Кадрирование.
    let crop_x = gtk::SpinButton::with_range(0.0, 1200.0, 1.0);
    let crop_y = gtk::SpinButton::with_range(0.0, 1200.0, 1.0);
    let crop_w = gtk::SpinButton::with_range(0.0, 1200.0, 1.0);
    let crop_h = gtk::SpinButton::with_range(0.0, 1200.0, 1.0);
    crop_w.set_value(0.0);
    crop_h.set_value(0.0);
    let crop_reset = gtk::Button::builder()
        .label(i18n::tr("scan.crop_reset"))
        .css_classes(["flat"])
        .build();
    let crop_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let crop_w_label = i18n::tr("scan.crop_w");
    let crop_h_label = i18n::tr("scan.crop_h");
    for (label, spin) in [
        ("X", &crop_x),
        ("Y", &crop_y),
        (crop_w_label.as_str(), &crop_w),
        (crop_h_label.as_str(), &crop_h),
    ] {
        let l = gtk::Label::new(Some(label));
        l.add_css_class("dim-label");
        crop_box.append(&l);
        spin.set_width_request(64);
        crop_box.append(spin);
    }
    crop_box.append(&crop_reset);
    add_grid_row(&grid, &mut row, &i18n::tr("scan.crop"), &crop_box);

    // Формат, шаблон, папка.
    add_grid_row(&grid, &mut row, &i18n::tr("scan.format"), &format_dd);
    add_grid_row(&grid, &mut row, &i18n::tr("scan.template"), &template_entry);
    let folder_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    folder_box.append(&folder_row_label);
    folder_box.append(&folder_btn);
    add_grid_row(&grid, &mut row, &i18n::tr("scan.folder"), &folder_box);

    // OCR.
    let ocr_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    ocr_box.append(&ocr_switch);
    ocr_box.append(&ocr_lang_dd);
    add_grid_row(&grid, &mut row, &i18n::tr("scan.ocr"), &ocr_box);

    // --- Предпросмотр -----------------------------------------------------
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Contain)
        .can_shrink(true)
        .height_request(300)
        .css_classes(["card"])
        .build();
    let spinner = gtk::Spinner::new();
    spinner.set_visible(false);
    let preview_stack = gtk::Stack::new();
    preview_stack.add_named(&picture, Some("picture"));
    preview_stack.add_named(&spinner, Some("spinner"));

    // --- Кнопки и прогресс ------------------------------------------------
    let preview_btn = gtk::Button::builder()
        .label(i18n::tr("scan.preview_btn"))
        .icon_name("zoom-fit-best-symbolic")
        .tooltip_text(i18n::trf("scan.preview_hint", &[("dpi", "96")]))
        .build();
    let scan_btn = gtk::Button::builder()
        .label(i18n::tr("scan.scan_save"))
        .icon_name("document-scan-symbolic")
        .css_classes(["suggested-action"])
        .build();
    let cancel_btn = gtk::Button::builder()
        .label(i18n::tr("scan.cancel"))
        .icon_name("process-stop-symbolic")
        .css_classes(["destructive-action"])
        .sensitive(false)
        .build();
    let progress = gtk::ProgressBar::builder()
        .margin_start(18)
        .margin_end(18)
        .show_text(false)
        .build();
    let progress_label = gtk::Label::builder()
        .label("")
        .css_classes(["dim-label"])
        .margin_start(18)
        .margin_end(18)
        .build();
    let btn_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .margin_start(18)
        .margin_end(18)
        .margin_bottom(12)
        .halign(gtk::Align::Center)
        .build();
    btn_row.append(&preview_btn);
    btn_row.append(&scan_btn);
    btn_row.append(&cancel_btn);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    content.append(&grid);
    content.append(&preview_stack);
    content.append(&progress);
    content.append(&progress_label);
    content.append(&btn_row);
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&content)
        .build();
    toolbar.set_content(Some(&scrolled));

    let state = Rc::new(ScanState {
        ctx: ctx.clone(),
        window: window.clone(),
        scanner_dd: scanner_dd.clone(),
        source_dd,
        res_dd,
        mode_dd,
        pages_spin,
        rotation_dd,
        crop_x,
        crop_y,
        crop_w,
        crop_h,
        format_dd,
        template_entry,
        folder_row_label,
        ocr_switch,
        ocr_lang_dd,
        preview_btn: preview_btn.clone(),
        scan_btn: scan_btn.clone(),
        cancel_btn: cancel_btn.clone(),
        progress,
        progress_label,
        picture,
        runtime: RefCell::new(Runtime {
            cancel: None,
            busy: false,
            options: None,
        }),
    });

    // --- Сигналы ----------------------------------------------------------
    {
        let s = state.clone();
        refresh_btn.connect_clicked(move |_| s.refresh());
    }
    {
        let s = state.clone();
        scanner_dd.connect_selected_notify(move |_| s.load_scanner_options());
    }
    {
        let s = state.clone();
        preview_btn.connect_clicked(move |_| s.start_scan(true));
    }
    {
        let s = state.clone();
        scan_btn.connect_clicked(move |_| s.start_scan(false));
    }
    {
        let s = state.clone();
        cancel_btn.connect_clicked(move |_| {
            if let Some(handle) = s.runtime.borrow().cancel.clone() {
                handle.cancel();
            }
        });
    }
    {
        let s = state.clone();
        crop_reset.connect_clicked(move |_| {
            s.crop_x.set_value(0.0);
            s.crop_y.set_value(0.0);
            s.crop_w.set_value(0.0);
            s.crop_h.set_value(0.0);
        });
    }
    {
        let s = state.clone();
        folder_btn.connect_clicked(move |_| s.choose_folder());
    }

    let refresh = {
        let s = state.clone();
        move || s.refresh()
    };
    let handle = make_handle(refresh.clone());
    refresh();

    (toolbar.upcast::<gtk::Widget>(), handle)
}

fn translated(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|k| i18n::tr(k)).collect()
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

fn set_selected_text(dd: &gtk::DropDown, value: &str) {
    let model = match dd.model() {
        Some(m) => m,
        None => return,
    };
    for i in 0..model.n_items() {
        if let Some(text) = model
            .item(i)
            .and_then(|o| o.downcast::<gtk::StringObject>().ok())
            .map(|s| s.string())
        {
            if text == value {
                dd.set_selected(i);
                return;
            }
        }
    }
}

impl ScanState {
    fn refresh(self: &Rc<Self>) {
        if self.runtime.borrow().busy {
            return;
        }
        let s = self.clone();
        self.ctx.refresh_devices(Some(Box::new(move || {
            s.fill_scanners();
        })));
    }

    fn fill_scanners(self: &Rc<Self>) {
        let scanners = self.ctx.scanners.borrow().clone();
        if scanners.is_empty() {
            set_items(&self.scanner_dd, &[i18n::tr("scan.no_scanner")]);
            self.scan_btn.set_sensitive(false);
            self.preview_btn.set_sensitive(false);
            return;
        }
        self.scan_btn.set_sensitive(true);
        self.preview_btn.set_sensitive(true);
        let labels: Vec<String> = scanners
            .iter()
            .map(|s| format!("{} — {}", s.name, s.id))
            .collect();
        set_items(&self.scanner_dd, &labels);
        if let Some(default_id) = self.ctx.effective_default_scanner() {
            if let Some(idx) = scanners.iter().position(|s| s.id == default_id) {
                self.scanner_dd.set_selected(idx as u32);
            }
        }
        self.load_scanner_options();
    }

    fn selected_scanner_id(self: &Rc<Self>) -> Option<String> {
        let scanners = self.ctx.scanners.borrow();
        let idx = self.scanner_dd.selected() as usize;
        scanners.get(idx).map(|s| s.id.clone())
    }

    /// Загрузить возможности сканера и заполнить соответствующие списки.
    fn load_scanner_options(self: &Rc<Self>) {
        let Some(id) = self.selected_scanner_id() else {
            return;
        };
        let defaults = self.ctx.settings.borrow().scan.clone();
        let s = self.clone();
        tasks::run_async(
            move || sane::scanner_options(&id),
            move |result| match result {
                Ok(options) => {
                    let res_items: Vec<String> =
                        options.resolutions.iter().map(|r| r.to_string()).collect();
                    if !res_items.is_empty() {
                        set_items(&s.res_dd, &res_items);
                        set_selected_text(&s.res_dd, &defaults.resolution.to_string());
                    }
                    let modes = if options.modes.is_empty() {
                        translated(&["scan.mode.lineart", "scan.mode.gray", "scan.mode.color"])
                    } else {
                        options.modes.clone()
                    };
                    set_items(&s.mode_dd, &modes);
                    // Канонические имена режимов есть только у реальных списков.
                    let preferred_mode = if options.modes.is_empty() {
                        "Color".to_string()
                    } else {
                        defaults.mode.as_scanimage().to_string()
                    };
                    set_selected_text(&s.mode_dd, &preferred_mode);

                    let sources = if options.sources.is_empty() {
                        vec!["Flatbed".to_string()]
                    } else {
                        options.sources.clone()
                    };
                    set_items(&s.source_dd, &sources);
                    if sources.iter().any(|x| x == &defaults.source) {
                        set_selected_text(&s.source_dd, &defaults.source);
                    }
                    s.runtime.borrow_mut().options = Some(options);
                }
                Err(e) => {
                    tracing::warn!("возможности сканера недоступны: {e}");
                    // Оставляем значения по умолчанию, UI остаётся рабочим.
                    s.runtime.borrow_mut().options = None;
                }
            },
        );
    }

    fn choose_folder(self: &Rc<Self>) {
        let dialog = gtk::FileDialog::builder()
            .title(i18n::tr("scan.folder"))
            .build();
        let s = self.clone();
        dialog.select_folder(Some(&self.window), gio::Cancellable::NONE, move |result| {
            if let Ok(folder) = result {
                if let Some(path) = folder.path() {
                    let text = path.to_string_lossy().into_owned();
                    s.folder_row_label.set_label(&text);
                    s.folder_row_label.set_tooltip_text(Some(&text));
                    s.ctx.settings.borrow_mut().scan.scan_dir = text;
                    s.ctx.save_settings();
                }
            }
        });
    }

    /// Собрать запрос сканирования из UI.
    fn build_request(self: &Rc<Self>, dpi: u32) -> Option<ScanRequest> {
        let scanner_id = self.selected_scanner_id()?;
        let source = selected_text(&self.source_dd).unwrap_or_else(|| "Flatbed".into());
        let resolution = selected_text(&self.res_dd)
            .and_then(|t| t.parse().ok())
            .unwrap_or(300);
        // Точное имя режима передаём scanimage, только если оно заявлено
        // сканером (иначе каноническое имя из ColorMode).
        let mode_name = selected_text(&self.mode_dd).filter(|m| {
            self.runtime
                .borrow()
                .options
                .as_ref()
                .map(|o| o.modes.contains(m))
                .unwrap_or(false)
        });
        let mode = mode_name
            .as_deref()
            .and_then(ColorMode::from_scanimage)
            .unwrap_or(ColorMode::Color);
        let crop = {
            let (x, y, w, h) = (
                self.crop_x.value(),
                self.crop_y.value(),
                self.crop_w.value(),
                self.crop_h.value(),
            );
            if w <= 0.0 || h <= 0.0 {
                None
            } else {
                Some(RectMm { x, y, w, h })
            }
        };
        let rotation = match self.rotation_dd.selected() {
            1 => Rotation::R90,
            2 => Rotation::R180,
            3 => Rotation::R270,
            _ => Rotation::R0,
        };
        Some(ScanRequest {
            scanner_id,
            resolution: if dpi > 0 { dpi } else { resolution },
            mode,
            mode_name,
            source,
            crop,
            rotation,
            pages: self.pages_spin.value() as u32,
        })
    }

    /// Общая точка запуска: preview=true — быстрый предпросмотр (малое dpi).
    fn start_scan(self: &Rc<Self>, preview: bool) {
        if self.runtime.borrow().busy {
            return;
        }
        let dpi = if preview {
            self.ctx.settings.borrow().scan.preview_dpi
        } else {
            0 // 0 — использовать выбранное разрешение
        };
        let Some(request) = self.build_request(dpi) else {
            self.ctx.toast(&i18n::tr("scan.no_scanner"));
            return;
        };
        let options = self.runtime.borrow().options.clone().unwrap_or_default();
        if !options.sources.is_empty() && !options.sources.contains(&request.source) {
            self.ctx
                .toast(&i18n::trf("scan-invalid-arg", &[("msg", &request.source)]));
            return;
        }

        let output_dir = self.ctx.settings.borrow().scan_dir();
        if let Err(e) = Settings::check_writable_dir(&output_dir) {
            self.ctx.toast_error(&e);
            return;
        }
        self.persist_settings();
        let format = self.selected_format();

        self.set_busy(true);
        let cancel = CancelHandle::new();
        self.runtime.borrow_mut().cancel = Some(cancel.clone());

        let progress_state = self.clone();
        let done_state = self.clone();
        tasks::run_with_progress(
            move |progress_tx| {
                sane::scan_pages(&request, &options, &cancel, move |p| {
                    let _ = progress_tx.send_blocking(p);
                })
            },
            move |p: ScanProgress| progress_state.on_progress(p),
            move |result| match result {
                Ok(pages) if preview => done_state.finish_preview(pages),
                Ok(pages) => done_state.finish_save(pages, format, output_dir),
                Err(e) if e.is_cancelled() => {
                    done_state.set_busy(false);
                    done_state.progress_label.set_label("");
                }
                Err(e) => {
                    done_state.set_busy(false);
                    done_state.ctx.toast_error(&e);
                }
            },
        );
    }

    fn on_progress(&self, progress: ScanProgress) {
        match progress {
            ScanProgress::PageStarted { page, total } => {
                let label = match total {
                    Some(t) => i18n::trf(
                        "scan.scanning",
                        &[("page", &page.to_string()), ("total", &t.to_string())],
                    ),
                    None => i18n::trf(
                        "scan.page_of",
                        &[("page", &page.to_string()), ("total", "?")],
                    ),
                };
                self.progress_label.set_label(&label);
                match total {
                    Some(t) if t > 0 => self
                        .progress
                        .set_fraction((page.saturating_sub(1)) as f64 / t as f64),
                    _ => self.progress.pulse(),
                }
            }
            ScanProgress::PageDone { page, total } => match total {
                Some(t) if t > 0 => self.progress.set_fraction(page as f64 / t as f64),
                _ => self.progress.pulse(),
            },
            // Постраничный процент из scanimage --progress: уточняет полосу
            // прогресса внутри страницы (события страниц остаются ведущими).
            ScanProgress::Percent {
                page,
                total,
                percent,
            } => {
                let label = match total {
                    Some(t) => i18n::trf(
                        "scan.page_of_percent",
                        &[
                            ("page", &page.to_string()),
                            ("total", &t.to_string()),
                            ("percent", &percent.to_string()),
                        ],
                    ),
                    None => i18n::trf(
                        "scan.page_percent",
                        &[
                            ("page", &page.to_string()),
                            ("percent", &percent.to_string()),
                        ],
                    ),
                };
                self.progress_label.set_label(&label);
                self.progress.set_fraction(percent as f64 / 100.0);
            }
        }
    }

    /// Показать отсканированное изображение в области предпросмотра.
    fn finish_preview(self: &Rc<Self>, pages: Vec<sane::ScanPage>) {
        self.set_busy(false);
        self.progress_label.set_label("");
        let Some(first) = pages.first() else { return };
        let path = linuxprinterg_core::storage::cache_dir().join("scan-preview.png");
        if let Err(e) = save::save_image(&first.image, &path, OutputFormat::Png) {
            self.ctx.toast_error(&e);
            return;
        }
        self.show_preview_image(&path);
    }

    /// Сохранить страницы в выбранный формат (+ опциональный OCR).
    fn finish_save(
        self: &Rc<Self>,
        pages: Vec<sane::ScanPage>,
        format: OutputFormat,
        dir: PathBuf,
    ) {
        self.set_busy(false);
        if pages.is_empty() {
            return;
        }
        let settings = self.ctx.settings.borrow().clone();
        let device = self.selected_scanner_id().unwrap_or_default();
        let total = pages.len();
        let mut template = settings.scan.template.clone();
        if total > 1 && !template.contains("{page}") {
            template.push_str("_{page}");
        }

        let result: Result<Vec<PathBuf>, Error> = (|| {
            let mut saved = Vec::new();
            if format.is_multi_page() {
                // Один файл на весь скан: PDF (PdfWriter) или
                // многостраничный TIFF (TiffWriter).
                let ext = format.extension();
                let ctx = NameCtx::now("scan", &device, ext);
                let name = naming::render(&template, &ctx);
                let path = naming::unique_path(&dir, &name, ext);
                if format == OutputFormat::Tiff {
                    let mut writer = TiffWriter::new(pages[0].dpi);
                    for page in &pages {
                        writer.push_image(&page.image);
                    }
                    writer.save(&path)?;
                } else {
                    let mut writer = PdfWriter::new(pages[0].dpi);
                    for page in &pages {
                        writer.push_image(&page.image);
                    }
                    writer.save(&path)?;
                }
                saved.push(path);
            } else {
                let ext = format.extension();
                for (i, page) in pages.iter().enumerate() {
                    let mut ctx = NameCtx::now("scan", &device, ext);
                    ctx.page = Some((i + 1) as u32);
                    ctx.counter = (i + 1) as u32;
                    let name = naming::render(&template, &ctx);
                    let path = naming::unique_path(&dir, &name, ext);
                    save::save_image(&page.image, &path, format)?;
                    saved.push(path);
                }
            }
            Ok(saved)
        })();

        let saved = match result {
            Ok(paths) => paths,
            Err(e) => {
                self.ctx.toast_error(&e);
                return;
            }
        };

        // Первый файл — в предпросмотр и уведомление.
        if let Some(first) = saved.first() {
            self.ctx.toast(&i18n::trf(
                "scan.saved",
                &[("path", &first.to_string_lossy())],
            ));
            for path in &saved {
                self.ctx.journal.info(
                    Category::Scan,
                    i18n::trf(
                        "journal.entry.scan_saved",
                        &[("path", &path.to_string_lossy())],
                    ),
                );
            }
            if !format.is_multi_page() {
                self.show_preview_image(first);
            }
        }

        // OCR (опционально) — текстовый файл рядом со сканом.
        if settings.scan.ocr_enabled {
            self.run_ocr(&pages, &settings.scan.ocr_lang, &template, &device, &dir);
        }
    }

    fn run_ocr(
        self: &Rc<Self>,
        pages: &[sane::ScanPage],
        lang: &str,
        template: &str,
        device: &str,
        dir: &Path,
    ) {
        if !ocr::available() {
            self.ctx
                .toast(&i18n::tr("err-tool-missing").replace("{tool}", "tesseract"));
            return;
        }
        self.progress_label.set_label(&i18n::tr("scan.ocr"));
        self.progress.pulse();
        let mut text = String::new();
        let cache = linuxprinterg_core::storage::cache_dir().join("ocr");
        for (i, page) in pages.iter().enumerate() {
            let tmp = cache.join(format!("page-{i}.png"));
            if let Err(e) = save::save_image(&page.image, &tmp, OutputFormat::Png) {
                self.ctx.toast_error(&e);
                return;
            }
            match ocr::ocr_file(&tmp, lang) {
                Ok(t) => {
                    if i > 0 {
                        text.push_str("\n\n— — —\n\n");
                    }
                    text.push_str(&t);
                }
                Err(e) => {
                    tracing::warn!("OCR страницы {i} не удался: {e}");
                }
            }
            let _ = std::fs::remove_file(&tmp);
        }
        self.progress_label.set_label("");
        if text.trim().is_empty() {
            return;
        }
        let mut ctx = NameCtx::now("scan", device, "txt");
        ctx.page = None;
        let name = naming::render(template, &ctx);
        let path = naming::unique_path(dir, &name, "txt");
        match std::fs::write(&path, text) {
            Ok(()) => {
                self.ctx.journal.info(
                    Category::Scan,
                    i18n::trf(
                        "journal.entry.ocr_done",
                        &[("path", &path.to_string_lossy())],
                    ),
                );
                self.ctx.toast(&i18n::trf(
                    "journal.entry.ocr_done",
                    &[("path", &path.to_string_lossy())],
                ));
            }
            Err(e) => self.ctx.toast_error(&Error::io("запись OCR", e)),
        }
    }

    fn persist_settings(self: &Rc<Self>) {
        let mut settings = self.ctx.settings.borrow_mut();
        settings.scan.resolution = selected_text(&self.res_dd)
            .and_then(|t| t.parse().ok())
            .unwrap_or(settings.scan.resolution);
        if let Some(mode) = selected_text(&self.mode_dd) {
            if let Some(m) = ColorMode::from_scanimage(&mode) {
                settings.scan.mode = m;
            }
        }
        if let Some(source) = selected_text(&self.source_dd) {
            settings.scan.source = source;
        }
        settings.scan.format = self.selected_format();
        settings.scan.template = self.template_entry.text().to_string();
        settings.scan.ocr_enabled = ocr_switch_active(&self.ocr_switch);
        if let Some(lang) = selected_text(&self.ocr_lang_dd) {
            settings.scan.ocr_lang = lang;
        }
        drop(settings);
        self.ctx.save_settings();
    }

    fn selected_format(&self) -> OutputFormat {
        match self.format_dd.selected() {
            1 => OutputFormat::Png,
            2 => OutputFormat::Jpeg,
            3 => OutputFormat::Tiff,
            4 => OutputFormat::Webp,
            _ => OutputFormat::Pdf,
        }
    }

    fn set_busy(self: &Rc<Self>, busy: bool) {
        self.runtime.borrow_mut().busy = busy;
        self.scan_btn.set_sensitive(!busy);
        self.preview_btn.set_sensitive(!busy);
        self.cancel_btn.set_sensitive(busy);
        if busy {
            self.progress.set_fraction(0.0);
            self.progress_label.set_label("");
        }
    }
}

fn ocr_switch_active(sw: &gtk::Switch) -> bool {
    sw.is_active()
}

// Помощники страницы сканирования.
impl ScanState {
    /// Показать изображение в области предпросмотра.
    fn show_preview_image(self: &Rc<Self>, path: &PathBuf) {
        match gdk::Texture::from_filename(path) {
            Ok(texture) => self.picture.set_paintable(Some(&texture)),
            Err(e) => self.ctx.toast(&i18n::trf(
                "print.preview_failed",
                &[("msg", &e.to_string())],
            )),
        }
    }
}
