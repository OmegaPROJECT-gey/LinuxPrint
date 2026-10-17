//! Страница «Устройства»: принтеры и сканеры, свойства, добавление принтера.

use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;

use linuxprinterg_core::domain::PrinterState;
use linuxprinterg_core::i18n;
use linuxprinterg_core::services::cups::{self, AddPrinterStrategy};
use linuxprinterg_core::storage::journal::Category;

use crate::ctx::{Ctx, show_message};
use crate::tasks;
use crate::window::{PageHandle, make_handle};

pub fn build(ctx: &Ctx) -> (gtk::Widget, PageHandle) {
    let page = adw::PreferencesPage::new();

    let banner = adw::Banner::new("");
    banner.set_button_label(Some(&i18n::tr("devices.open_cups_web")));
    banner.set_revealed(false);

    let printers_group = adw::PreferencesGroup::builder()
        .title(&i18n::tr("devices.printers"))
        .description(&i18n::tr("devices.empty_printers"))
        .build();
    let scanners_group = adw::PreferencesGroup::builder()
        .title(&i18n::tr("devices.scanners"))
        .description(&i18n::tr("devices.empty_scanners"))
        .build();

    let printers_holder = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    let scanners_holder = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    printers_group.add(&printers_holder);
    scanners_group.add(&scanners_holder);

    let state = Rc::new(DevicesState {
        ctx: ctx.clone(),
        banner,
        printers_holder,
        scanners_holder,
    });

    {
        let s = state.clone();
        state.banner.connect_button_clicked(move |_| {
            if let Err(e) =
                gio::AppInfo::launch_default_for_uri("http://localhost:631", None::<&gio::AppLaunchContext>)
            {
                s.ctx
                    .toast_error(&linuxprinterg_core::Error::msg(e.to_string()));
            }
        });
    }

    // Заголовок страницы с кнопками.
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&gtk::Label::new(Some(&i18n::tr("nav.devices")))));
    let add_btn = gtk::Button::builder()
        .label(&i18n::tr("devices.add_printer"))
        .icon_name("list-add-symbolic")
        .tooltip_text(&i18n::tr("devices.add_printer"))
        .css_classes(["flat"])
        .build();
    let refresh_btn = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text(&i18n::tr("common.refresh"))
        .css_classes(["flat"])
        .build();
    header.pack_end(&refresh_btn);
    header.pack_end(&add_btn);
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&page));
    // Баннер над содержимым.
    toolbar.add_top_bar(&state.banner);

    {
        let s = state.clone();
        refresh_btn.connect_clicked(move |_| s.refresh());
    }
    {
        let s = state.clone();
        add_btn.connect_clicked(move |_| s.add_printer());
    }

    let refresh = {
        let s = state.clone();
        move || s.refresh()
    };
    let handle = make_handle(refresh.clone());
    refresh();

    (toolbar.upcast::<gtk::Widget>(), handle)
}

struct DevicesState {
    ctx: Ctx,
    banner: adw::Banner,
    printers_holder: gtk::Box,
    scanners_holder: gtk::Box,
}

impl DevicesState {
    fn refresh(self: &Rc<Self>) {
        let s = self.clone();
        self.ctx.refresh_devices(Some(Box::new(move || {
            s.rebuild();
            s.check_cups();
        })));
    }

    fn check_cups(self: &Rc<Self>) {
        let s = self.clone();
        tasks::run_async(cups::scheduler_running, move |result| match result {
            Ok(true) => s.banner.set_revealed(false),
            Ok(false) => {
                s.banner.set_title(&i18n::tr("devices.cups_stopped"));
                s.banner.set_revealed(true);
            }
            Err(e) => {
                s.banner.set_title(&i18n::trf(
                    "devices.cups_banner",
                    &[("msg", &e.ui_message())],
                ));
                s.banner.set_revealed(true);
            }
        });
    }

    fn rebuild(self: &Rc<Self>) {
        // Принтеры.
        clear_children(&self.printers_holder);
        let printers = self.ctx.printers.borrow().clone();
        if printers.is_empty() {
            self.printers_holder
                .append(&dim_label(&i18n::tr("devices.empty_printers")));
        }
        for printer in printers {
            let is_default =
                self.ctx.effective_default_printer().as_deref() == Some(printer.name.as_str());
            let row = adw::ActionRow::builder()
                .title(&printer.name)
                .subtitle(&format!(
                    "{} · {}",
                    state_text(&printer.state),
                    if printer.uri.is_empty() {
                        i18n::tr("common.none")
                    } else {
                        printer.uri.clone()
                    }
                ))
                .activatable(true)
                .build();

            if is_default {
                let badge = gtk::Label::builder()
                    .label(&i18n::tr("devices.default_badge"))
                    .css_classes(["success", "caption"])
                    .build();
                row.add_suffix(&badge);
            } else {
                let set_default = gtk::Button::builder()
                    .label(&i18n::tr("devices.set_default"))
                    .css_classes(["flat"])
                    .valign(gtk::Align::Center)
                    .build();
                {
                    let s = self.clone();
                    let name = printer.name.clone();
                    set_default.connect_clicked(move |_| s.set_default_printer(&name));
                }
                row.add_suffix(&set_default);
            }

            {
                let ctx = self.ctx.clone();
                let name = printer.name.clone();
                let uri = printer.uri.clone();
                let state = printer.state;
                row.connect_activated(move |_| {
                    let body = format!(
                        "{}: {}\n{}: {}\n{}: {}",
                        i18n::tr("print.printer"),
                        name,
                        i18n::tr("devices.uri"),
                        uri,
                        i18n::tr("devices.details"),
                        state_text(&state),
                    );
                    show_message(None::<&gtk::Widget>, &name, &body);
                    let _ = &ctx;
                });
            }
            self.printers_holder.append(&row);
        }

        // Сканеры.
        clear_children(&self.scanners_holder);
        let scanners = self.ctx.scanners.borrow().clone();
        if scanners.is_empty() {
            self.scanners_holder
                .append(&dim_label(&i18n::tr("devices.empty_scanners")));
        }
        for scanner in scanners {
            let row = adw::ActionRow::builder()
                .title(&scanner.name)
                .subtitle(&scanner.id)
                .activatable(true)
                .build();
            let id = scanner.id.clone();
            let name = scanner.name.clone();
            let s = self.clone();
            row.connect_activated(move |_| s.scanner_details(&id, &name));
            self.scanners_holder.append(&row);
        }
    }

    fn scanner_details(self: &Rc<Self>, id: &str, name: &str) {
        let id = id.to_string();
        let name = name.to_string();
        let s = self.clone();
        tasks::run_async(
            move || linuxprinterg_core::services::sane::scanner_options(&id),
            move |result| {
                let body = match result {
                    Ok(o) => format!(
                        "{}: {}\n{}: {}\n{}: {}",
                        i18n::tr("scan.resolution"),
                        o.resolutions
                            .iter()
                            .map(|r| r.to_string())
                            .collect::<Vec<_>>()
                            .join(", "),
                        i18n::tr("scan.mode"),
                        o.modes.join(", "),
                        i18n::tr("scan.source"),
                        o.sources.join(", "),
                    ),
                    Err(e) => e.ui_message(),
                };
                show_message(None::<&gtk::Widget>, &name, &body);
                let _ = &s;
            },
        );
    }

    fn set_default_printer(self: &Rc<Self>, name: &str) {
        let name = name.to_string();
        let name_for_cb = name.clone();
        let s = self.clone();
        tasks::run_async(
            move || cups::set_user_default_printer(&name),
            move |result| match result {
                Ok(()) => {
                    s.ctx.settings.borrow_mut().default_printer = Some(name_for_cb.clone());
                    s.ctx.save_settings();
                    s.ctx.journal.info(
                        Category::Settings,
                        format!("{}: {}", i18n::tr("devices.set_default"), name_for_cb),
                    );
                    s.refresh();
                }
                Err(e) => s.ctx.toast_error(&e),
            },
        );
    }

    /// Добавление принтера: системный мастер → веб-интерфейс → инструкция.
    /// Приложение не выполняет `lpadmin` и не запрашивает root самостоятельно.
    fn add_printer(self: &Rc<Self>) {
        match cups::add_printer_strategy() {
            AddPrinterStrategy::SystemConfigPrinter => {
                let s = self.clone();
                tasks::run_async(
                    move || {
                        std::process::Command::new("system-config-printer")
                            .spawn()
                            .map_err(|e| {
                                linuxprinterg_core::Error::io("запуск system-config-printer", e)
                            })?;
                        Ok(())
                    },
                    move |result| match result {
                        Ok(()) => s.ctx.toast(&i18n::tr("devices.add_printer_done")),
                        Err(e) => s.ctx.toast_error(&e),
                    },
                );
            }
            AddPrinterStrategy::CupsWebUi(url) => {
                if let Err(e) = gio::AppInfo::launch_default_for_uri(
                    &url,
                    None::<&gio::AppLaunchContext>,
                ) {
                    self.ctx
                        .toast_error(&linuxprinterg_core::Error::msg(e.to_string()));
                } else {
                    self.ctx.toast(&i18n::tr("devices.open_cups_web"));
                }
            }
            AddPrinterStrategy::Manual(text) => {
                show_message(
                    None::<&gtk::Widget>,
                    &i18n::tr("devices.add_printer"),
                    &text,
                );
            }
        }
    }
}

fn clear_children(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn dim_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .css_classes(["dim-label"])
        .wrap(true)
        .xalign(0.0)
        .build()
}

fn state_text(state: &PrinterState) -> String {
    match state {
        PrinterState::Idle => i18n::tr("devices.state.idle"),
        PrinterState::Printing => i18n::tr("devices.state.printing"),
        PrinterState::Disabled => i18n::tr("devices.state.disabled"),
        PrinterState::Unknown => i18n::tr("devices.state.unknown"),
    }
}
