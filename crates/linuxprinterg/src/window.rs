//! Главное окно: боковая навигация (OverlaySplitView) + страницы.
//!
//! Страницы создаются лениво при первом выборе. На узких окнах боковая
//! панель сворачивается в выдвижную (свойство `collapsed`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;

use linuxprinterg_core::i18n;

use crate::ctx::Ctx;
use crate::pages;

/// Ссылка на активную страницу для действий (кнопка «Обновить», Ctrl+R).
#[derive(Clone)]
pub struct PageHandle {
    pub refresh: Rc<dyn Fn()>,
}

struct Switcher {
    stack: gtk::Stack,
    pages: RefCell<Vec<Option<PageHandle>>>,
    current: Cell<u32>,
}

pub fn build(app: &adw::Application, ctx: &Ctx) -> adw::ApplicationWindow {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .default_width(1080)
        .default_height(720)
        .title(i18n::tr("app.title"))
        .build();

    *ctx.window.borrow_mut() = Some(window.clone());

    // ---- Боковая панель -------------------------------------------------
    let sidebar_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .css_classes(["navigation-sidebar"])
        .build();
    sidebar_list.set_valign(gtk::Align::Start);

    for (icon, label_key) in [
        ("printer-symbolic", "nav.print"),
        ("document-scan-symbolic", "nav.scan"),
        ("computer-symbolic", "nav.devices"),
        ("emblem-system-symbolic", "nav.settings"),
        ("document-open-recent-symbolic", "nav.journal"),
    ] {
        sidebar_list.append(&sidebar_row(icon, &i18n::tr(label_key)));
    }

    let sidebar_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&sidebar_list)
        .width_request(220)
        .build();

    // ---- Стек страниц ----------------------------------------------------
    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .transition_duration(150)
        .build();

    let switcher = Rc::new(Switcher {
        stack: stack.clone(),
        pages: RefCell::new(vec![None, None, None, None, None]),
        current: Cell::new(0),
    });

    // Ленивая сборка страниц.
    {
        let stack = stack.clone();
        let switcher = switcher.clone();
        let ctx = ctx.clone();
        let window = window.clone();
        stack.connect_visible_child_notify(move |stack| {
            let Some(name) = stack.visible_child_name().map(|s| s.to_string()) else {
                return;
            };
            let Ok(index) = name.parse::<u32>() else {
                return;
            };
            ensure_page(&switcher, &ctx, &window, index);
        });
    }

    // Первая страница — сразу при открытии.
    ensure_page(&switcher, ctx, &window, 0);
    switcher.stack.set_visible_child_name("0");

    // Переключение по выбору в боковой панели.
    {
        let switcher = switcher.clone();
        sidebar_list.connect_row_selected(move |_list, row| {
            let Some(row) = row else { return };
            let index = row.index().max(0) as u32;
            switcher.stack.set_visible_child_name(&index.to_string());
        });
    }

    let content_page = adw::NavigationPage::builder()
        .tag("content")
        .title(i18n::tr("app.title"))
        .child(&stack)
        .build();

    let sidebar_page = adw::NavigationPage::builder()
        .tag("sidebar")
        .title(i18n::tr("app.title"))
        .child(&sidebar_scroll)
        .build();

    let split = adw::OverlaySplitView::builder()
        .sidebar(&sidebar_page)
        .content(&content_page)
        .sidebar_width_fraction(0.24)
        .min_sidebar_width(200.0)
        .enable_show_gesture(true)
        .enable_hide_gesture(true)
        .collapsed(false)
        .build();

    // Адаптивность: сворачиваем сайдбар на узких окнах.
    {
        let split = split.clone();
        window.connect_default_width_notify(move |win| {
            split.set_collapsed(win.default_width() < 760);
        });
    }

    ctx.toasts.set_child(Some(&split));
    window.set_content(Some(&ctx.toasts));

    install_actions(app, &switcher, &sidebar_list);
    window
}

/// Создать страницу `index`, если она ещё не создана, и показать её.
fn ensure_page(switcher: &Switcher, ctx: &Ctx, window: &adw::ApplicationWindow, index: u32) {
    // add_named() вызывает visible-child-notify, который может рекурсивно
    // вернуться сюда: в этом случае внешний вызов уже строит страницу.
    let Ok(mut pages) = switcher.pages.try_borrow_mut() else {
        return;
    };
    if pages
        .get(index as usize)
        .map(|p| p.is_some())
        .unwrap_or(false)
    {
        return;
    }
    let (child, handle) = match index {
        0 => pages::print::build(ctx, window),
        1 => pages::scan::build(ctx, window),
        2 => pages::devices::build(ctx),
        3 => pages::settings::build(ctx),
        4 => pages::journal::build(ctx),
        _ => return,
    };
    switcher.stack.add_named(&child, Some(&index.to_string()));
    // Первичная загрузка данных (асинхронно).
    let refresh = handle.refresh.clone();
    if let Some(slot) = pages.get_mut(index as usize) {
        *slot = Some(handle);
    }
    drop(pages);
    refresh();
}

fn sidebar_row(icon: &str, label: &str) -> gtk::ListBoxRow {
    let image = gtk::Image::builder()
        .icon_name(icon)
        .icon_size(gtk::IconSize::Normal)
        .build();
    let label = gtk::Label::builder()
        .label(label)
        .xalign(0.0)
        .hexpand(true)
        .build();
    let row_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(12)
        .margin_end(12)
        .build();
    row_box.append(&image);
    row_box.append(&label);
    gtk::ListBoxRow::builder().child(&row_box).build()
}

/// Действия приложения: выход, обновление, переключение страниц (Ctrl+1..5).
fn install_actions(app: &adw::Application, switcher: &Rc<Switcher>, sidebar_list: &gtk::ListBox) {
    let quit = gio::SimpleAction::new("quit", None);
    {
        let app = app.clone();
        quit.connect_activate(move |_, _| app.quit());
    }
    app.add_action(&quit);
    app.set_accels_for_action("app.quit", &["<Control>q"]);

    // Обновить текущую страницу.
    let refresh = gio::SimpleAction::new("refresh", None);
    {
        let switcher = switcher.clone();
        refresh.connect_activate(move |_, _| {
            let index = switcher.current.get() as usize;
            if let Some(Some(handle)) = switcher.pages.borrow().get(index) {
                (handle.refresh)();
            }
        });
    }
    app.add_action(&refresh);
    app.set_accels_for_action("app.refresh", &["<Control>r"]);

    // Ctrl+1..5 — прямое переключение страниц.
    for (n, accel, idx) in [
        (1u32, "<Control>1", 0u32),
        (2, "<Control>2", 1),
        (3, "<Control>3", 2),
        (4, "<Control>4", 3),
        (5, "<Control>5", 4),
    ] {
        let name = format!("page{n}");
        let action = gio::SimpleAction::new(&name, None);
        {
            let switcher = switcher.clone();
            let list = sidebar_list.clone();
            action.connect_activate(move |_, _| {
                switcher.stack.set_visible_child_name(&idx.to_string());
                if let Some(row) = list.row_at_index(idx as i32) {
                    list.select_row(Some(&row));
                }
                switcher.current.set(idx);
            });
        }
        app.add_action(&action);
        app.set_accels_for_action(&format!("app.page{n}"), &[accel]);
    }

    // Отслеживаем текущую страницу для «Обновить».
    {
        let stack = switcher.stack.clone();
        let switcher = switcher.clone();
        stack.connect_visible_child_name_notify(move |stack| {
            if let Some(name) = stack.visible_child_name() {
                if let Ok(i) = name.to_string().parse::<u32>() {
                    switcher.current.set(i);
                }
            }
        });
    }
}

// Удобные обёртки для страниц.
pub fn make_handle(refresh: impl Fn() + 'static) -> PageHandle {
    PageHandle {
        refresh: Rc::new(refresh),
    }
}

/// Диалог подтверждения для опасных действий.
pub fn confirm(
    parent: &impl IsA<gtk::Window>,
    heading: &str,
    body: &str,
    on_confirm: impl FnOnce() + 'static,
) {
    let dialog = adw::MessageDialog::builder()
        .heading(heading)
        .body(body)
        .close_response("cancel")
        .build();
    dialog.add_response("cancel", &i18n::tr("common.cancel"));
    dialog.add_response("ok", &i18n::tr("common.confirm"));
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    let on_confirm = std::cell::RefCell::new(Some(on_confirm));
    dialog.connect_response(Some("ok"), move |_, _| {
        if let Some(f) = on_confirm.borrow_mut().take() {
            f();
        }
    });
    // MessageDialog — подкласс gtk::Window: родитель задаётся transient_for.
    dialog.set_transient_for(Some(parent));
    dialog.present();
}
