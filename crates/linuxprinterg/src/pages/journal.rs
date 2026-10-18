//! Страница «Журнал»: история операций, экспорт, очистка.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;
use gtk::glib;
use gtk::pango;

use linuxprinterg_core::i18n;
use linuxprinterg_core::storage::journal::{Category, Entry, Level};

use crate::ctx::Ctx;
use crate::window::{PageHandle, confirm, make_handle};

pub fn build(ctx: &Ctx) -> (gtk::Widget, PageHandle) {
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&gtk::Label::new(Some(&i18n::tr("nav.journal")))));

    let refresh_btn = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text(i18n::tr("common.refresh"))
        .css_classes(["flat"])
        .build();
    let export_btn = gtk::Button::builder()
        .icon_name("document-save-symbolic")
        .tooltip_text(i18n::tr("common.export"))
        .css_classes(["flat"])
        .build();
    let clear_btn = gtk::Button::builder()
        .icon_name("edit-clear-symbolic")
        .tooltip_text(i18n::tr("common.clear"))
        .css_classes(["flat"])
        .build();

    // Фильтр по категории: 0 = все, далее Category::ALL.
    let mut filter_labels: Vec<String> = vec![i18n::tr("journal.filter_all")];
    filter_labels.extend(
        Category::ALL
            .iter()
            .map(|c| i18n::tr(&format!("journal.category.{}", c.as_str()))),
    );
    let filter_refs: Vec<&str> = filter_labels.iter().map(String::as_str).collect();
    let filter_dd = gtk::DropDown::from_strings(&filter_refs);
    filter_dd.set_tooltip_text(Some(&i18n::tr("journal.filter_all")));

    header.pack_end(&clear_btn);
    header.pack_end(&export_btn);
    header.pack_end(&refresh_btn);
    header.pack_start(&filter_dd);
    toolbar.add_top_bar(&header);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .margin_start(18)
        .margin_end(18)
        .margin_top(12)
        .margin_bottom(12)
        .build();
    let empty = gtk::Label::builder()
        .label(i18n::tr("journal.empty"))
        .css_classes(["dim-label"])
        .margin_top(24)
        .build();
    let stack = gtk::Stack::new();
    stack.add_named(&list, Some("list"));
    stack.add_named(&empty, Some("empty"));
    stack.set_visible_child_name("empty");

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&stack)
        .build();
    toolbar.set_content(Some(&scrolled));

    let state = Rc::new(JournalState {
        ctx: ctx.clone(),
        list,
        stack,
        empty,
        filter_dd: filter_dd.clone(),
        filter: Cell::new(0),
        last_seq: Cell::new(0),
    });

    {
        let s = state.clone();
        filter_dd.connect_selected_notify(move |_| {
            s.filter.set(s.filter_dd.selected());
            s.rebuild();
        });
    }
    {
        let s = state.clone();
        refresh_btn.connect_clicked(move |_| s.rebuild());
    }
    {
        let s = state.clone();
        export_btn.connect_clicked(move |_| s.export());
    }
    {
        let s = state.clone();
        clear_btn.connect_clicked(move |_| {
            let window = s.ctx.parent();
            let do_clear = {
                let s = s.clone();
                move || {
                    s.ctx.journal.clear();
                    s.rebuild();
                }
            };
            if let Some(window) = window {
                confirm(
                    &window,
                    &i18n::tr("common.clear"),
                    &i18n::tr("journal.clear_confirm"),
                    do_clear,
                );
            } else {
                do_clear();
            }
        });
    }

    // Автообновление при появлении новых записей (главный поток → local).
    {
        let s = state.clone();
        glib::timeout_add_seconds_local(5, move || {
            s.refresh_if_changed();
            glib::ControlFlow::Continue
        });
    }

    let refresh = {
        let s = state.clone();
        move || s.rebuild()
    };
    let handle = make_handle(refresh.clone());
    refresh();

    (toolbar.upcast::<gtk::Widget>(), handle)
}

struct JournalState {
    ctx: Ctx,
    list: gtk::ListBox,
    stack: gtk::Stack,
    empty: gtk::Label,
    filter_dd: gtk::DropDown,
    /// Выбранный фильтр: 0 — все категории, иначе `Category::ALL[idx - 1]`.
    filter: Cell<u32>,
    last_seq: Cell<u64>,
}

impl JournalState {
    /// Категория текущего фильтра (`None` — все).
    fn selected_category(&self) -> Option<Category> {
        match self.filter.get() {
            0 => None,
            i => Category::ALL.get(i as usize - 1).copied(),
        }
    }

    fn rebuild(self: &Rc<Self>) {
        while let Some(row) = self.list.row_at_index(0) {
            self.list.remove(&row);
        }
        let filter = self.selected_category();
        let mut entries = self.ctx.journal.entries_filtered(filter);
        self.last_seq.set(self.ctx.journal.seq());
        if entries.is_empty() {
            // Пусто из-за фильтра или журнала вовсе нет — разные подсказки.
            let key = if self.ctx.journal.entries().is_empty() {
                "journal.empty"
            } else {
                "journal.filter_empty"
            };
            self.empty.set_label(&i18n::tr(key));
            self.stack.set_visible_child_name("empty");
            return;
        }
        self.stack.set_visible_child_name("list");
        entries.reverse(); // новые сверху
        for entry in entries {
            self.list.append(&entry_row(&entry));
        }
    }

    fn refresh_if_changed(self: &Rc<Self>) {
        let seq = self.ctx.journal.seq();
        if seq != self.last_seq.get() {
            self.rebuild();
        }
    }

    fn export(self: &Rc<Self>) {
        let dialog = gtk::FileDialog::builder()
            .title(i18n::tr("common.export"))
            .initial_name(format!("linuxprinterg-journal-{}.csv", self.export_date()))
            .build();
        // Оба формата доступны; фактический выбирается по расширению файла.
        let csv_filter = gtk::FileFilter::new();
        csv_filter.set_name(Some("CSV"));
        csv_filter.add_pattern("*.csv");
        let txt_filter = gtk::FileFilter::new();
        txt_filter.set_name(Some("Text"));
        txt_filter.add_pattern("*.txt");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&csv_filter);
        filters.append(&txt_filter);
        dialog.set_filters(Some(&filters));

        let s = self.clone();
        dialog.save(
            self.ctx.parent().as_ref(),
            gio::Cancellable::NONE,
            move |result| {
                if let Ok(file) = result {
                    if let Some(path) = file.path() {
                        let category = s.selected_category();
                        let is_csv = path
                            .extension()
                            .is_some_and(|e| e.eq_ignore_ascii_case("csv"));
                        let outcome = if is_csv {
                            s.ctx.journal.export_csv_to(&path, category)
                        } else {
                            s.ctx.journal.export_to(&path, category)
                        };
                        match outcome {
                            Ok(()) => s.ctx.toast(&i18n::trf(
                                "journal.export_done",
                                &[("path", &path.to_string_lossy())],
                            )),
                            Err(e) => s.ctx.toast_error(&e),
                        }
                    }
                }
            },
        );
    }

    /// Дата для имени файла экспорта: из последней записи или «export».
    fn export_date(&self) -> String {
        self.ctx
            .journal
            .entries()
            .last()
            .map(|e| {
                e.ts_display()
                    .chars()
                    .take(10)
                    .collect::<String>()
                    .replace('-', "")
            })
            .unwrap_or_else(|| "export".to_string())
    }
}

fn entry_row(entry: &Entry) -> gtk::ListBoxRow {
    let time = gtk::Label::builder()
        .label(entry.ts_display())
        .css_classes(["dim-label", "monospace"])
        .xalign(0.0)
        .build();
    let level = gtk::Label::builder()
        .label(i18n::tr(&format!("journal.level.{}", entry.level.as_str())))
        .css_classes([level_css(entry.level)])
        .build();
    let category = gtk::Label::builder()
        .label(i18n::tr(&format!(
            "journal.category.{}",
            entry.category.as_str()
        )))
        .css_classes(["caption"])
        .build();
    let top = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    top.append(&time);
    top.append(&level);
    top.append(&category);
    let message = gtk::Label::builder()
        .label(&entry.message)
        .xalign(0.0)
        .wrap(true)
        .selectable(true)
        .build();
    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(12)
        .margin_end(12)
        .build();
    column.append(&top);
    column.append(&message);
    if let Some(path) = &entry.path {
        let path_label = gtk::Label::builder()
            .label(path)
            .xalign(0.0)
            .css_classes(["dim-label", "monospace"])
            .ellipsize(pango::EllipsizeMode::Middle)
            .selectable(true)
            .tooltip_text(path)
            .build();
        column.append(&path_label);
    }
    gtk::ListBoxRow::builder().child(&column).build()
}

fn level_css(level: Level) -> &'static str {
    match level {
        Level::Info => "success",
        Level::Warn => "warning",
        Level::Error => "danger",
    }
}
