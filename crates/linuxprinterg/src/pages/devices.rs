//! Страница «Устройства»: принтеры и сканеры (SANE/CUPS), обнаружение в
//! сети через DNS-SD (Avahi), свойства, добавление принтера.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;

use linuxprinterg_core::domain::PrinterState;
use linuxprinterg_core::i18n;
use linuxprinterg_core::services::cups::{self, AddPrinterStrategy};
use linuxprinterg_core::services::discover::{self, DnsSdDevice};
use linuxprinterg_core::storage::journal::Category;

use crate::ctx::{Ctx, show_message};
use crate::tasks;
use crate::window::{PageHandle, make_handle};

pub fn build(ctx: &Ctx) -> (gtk::Widget, PageHandle) {
    let page = adw::PreferencesPage::new();

    let banner = adw::Banner::new("");
    banner.set_button_label(Some(&i18n::tr("devices.open_cups_web")));
    banner.set_revealed(false);

    // Описания-заглушки «ничего не найдено» показывает rebuild() только
    // когда список действительно пуст (см. dim_label в холдерах).
    let printers_group = adw::PreferencesGroup::builder()
        .title(i18n::tr("devices.printers"))
        .build();
    let scanners_group = adw::PreferencesGroup::builder()
        .title(i18n::tr("devices.scanners"))
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

    // Сетевые устройства (DNS-SD/Avahi) — в дополнение к lpstat/scanimage.
    let net_holder = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    let net_group = adw::PreferencesGroup::builder()
        .title(i18n::tr("devices.network"))
        .description(i18n::tr("devices.network_hint"))
        .build();
    net_group.add(&net_holder);

    // Группы должны попасть на страницу — иначе содержимое не отображается.
    page.add(&printers_group);
    page.add(&scanners_group);
    page.add(&net_group);

    let state = Rc::new(DevicesState {
        ctx: ctx.clone(),
        banner,
        printers_holder,
        scanners_holder,
        net_holder,
        net: RefCell::new(Vec::new()),
    });

    {
        let s = state.clone();
        state.banner.connect_button_clicked(move |_| {
            if let Err(e) = gio::AppInfo::launch_default_for_uri(
                "http://localhost:631",
                None::<&gio::AppLaunchContext>,
            ) {
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
        .label(i18n::tr("devices.add_printer"))
        .icon_name("list-add-symbolic")
        .tooltip_text(i18n::tr("devices.add_printer"))
        .css_classes(["flat"])
        .build();
    let refresh_btn = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text(i18n::tr("common.refresh"))
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
    net_holder: gtk::Box,
    /// Последний результат DNS-SD-обхода (avahi-browse).
    net: RefCell<Vec<DnsSdDevice>>,
}

impl DevicesState {
    fn refresh(self: &Rc<Self>) {
        let s = self.clone();
        self.ctx.refresh_devices(Some(Box::new(move || {
            s.rebuild();
            s.check_cups();
            s.load_network();
        })));
    }

    /// Фоновый обход сети через Avahi (`avahi-browse -rtp`): Mopria-принтеры
    /// и AirScan/eSCL-сканеры — в дополнение к спискам CUPS/SANE.
    fn load_network(self: &Rc<Self>) {
        let s = self.clone();
        tasks::run_async(
            || Ok(discover::discover()),
            move |result| {
                if let Ok(devices) = result {
                    *s.net.borrow_mut() = devices;
                    s.rebuild_network();
                }
            },
        );
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
                .subtitle(format!(
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
                    .label(i18n::tr("devices.default_badge"))
                    .css_classes(["success", "caption"])
                    .build();
                row.add_suffix(&badge);
            } else {
                let set_default = gtk::Button::builder()
                    .label(i18n::tr("devices.set_default"))
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
                let name = printer.name.clone();
                let uri = printer.uri.clone();
                let state = printer.state;
                row.connect_activated(move |_| {
                    // ICC-профили драйвера читаем из PPD анонимным HTTP к
                    // cupsd — в фоне; при ошибке строка просто опускается.
                    let task_name = name.clone();
                    let dialog_name = name.clone();
                    let dialog_uri = uri.clone();
                    tasks::run_async(
                        move || cups::ppd_icc_profiles(&task_name),
                        move |icc| {
                            let mut body = format!(
                                "{}: {}\n{}: {}\n{}: {}",
                                i18n::tr("print.printer"),
                                dialog_name,
                                i18n::tr("devices.uri"),
                                dialog_uri,
                                i18n::tr("devices.details"),
                                state_text(&state),
                            );
                            if let Ok(profiles) = icc {
                                body.push('\n');
                                if profiles.is_empty() {
                                    body.push_str(&format!(
                                        "{}: {}",
                                        i18n::tr("devices.icc_title"),
                                        i18n::tr("devices.icc_none")
                                    ));
                                } else {
                                    body.push_str(&format!(
                                        "{}: {} — {}",
                                        i18n::tr("devices.icc_title"),
                                        profiles.join(", "),
                                        i18n::tr("devices.icc_auto")
                                    ));
                                }
                            }
                            show_message(None::<&gtk::Widget>, &dialog_name, &body);
                        },
                    );
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

        self.rebuild_network();
    }

    /// Список устройств, найденных через DNS-SD (Mopria/AirScan), — в
    /// дополнение к уже добавленным в CUPS/SANE.
    fn rebuild_network(self: &Rc<Self>) {
        clear_children(&self.net_holder);
        let devices = self.net.borrow().clone();
        if devices.is_empty() {
            let key = if discover::tool().is_some() {
                "devices.network_empty"
            } else {
                "devices.network_no_avahi"
            };
            self.net_holder.append(&dim_label(&i18n::tr(key)));
            return;
        }
        for device in devices {
            let kind = match device.kind {
                discover::DnsSdKind::Printer => i18n::tr("devices.net_printer"),
                discover::DnsSdKind::Scanner => i18n::tr("devices.net_scanner"),
            };
            let location = if device.address.is_empty() {
                device.host.clone()
            } else if device.port == 0 {
                device.address.clone()
            } else {
                format!("{}:{}", device.address, device.port)
            };
            let subtitle = if location.is_empty() {
                kind
            } else {
                format!("{kind} · {location}")
            };
            let row = adw::ActionRow::builder()
                .title(&device.name)
                .subtitle(subtitle)
                .activatable(true)
                .build();

            let mut body = format!(
                "{}: {}",
                i18n::tr("devices.net_service"),
                device.service_type
            );
            if !device.host.is_empty() {
                body.push_str(&format!(
                    "\n{}: {}",
                    i18n::tr("devices.net_host"),
                    device.host
                ));
            }
            if !device.address.is_empty() {
                let addr = if device.port > 0 {
                    format!("{}:{}", device.address, device.port)
                } else {
                    device.address.clone()
                };
                body.push_str(&format!("\n{}: {}", i18n::tr("devices.net_address"), addr));
            }
            let title = device.name.clone();
            row.connect_activated(move |_| {
                show_message(None::<&gtk::Widget>, &title, &body);
            });
            self.net_holder.append(&row);
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
                if let Err(e) =
                    gio::AppInfo::launch_default_for_uri(&url, None::<&gio::AppLaunchContext>)
                {
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
