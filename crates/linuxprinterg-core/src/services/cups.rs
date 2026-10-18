//! Интеграция с CUPS через штатные CLI-утилиты: `lpstat`, `lp`, `cancel`,
//! `lpoptions`.
//!
//! Приложение намеренно **не** использует `lpadmin` и не требует root:
//! добавление принтера выполняется системным диалогом (`system-config-printer`)
//! или веб-интерфейсом CUPS, которые сами запрашивают polkit-разрешение.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use regex::Regex;

use crate::domain::{Job, JobState, PageRange, PrintRequest, Printer, PrinterState};
use crate::error::{Error, Result};
use crate::services::ipp;
use crate::services::proc;

const TIMEOUT: Duration = Duration::from_secs(30);
const SUBMIT_TIMEOUT: Duration = Duration::from_secs(120);

/// Проверить, что CUPS доступен (утилиты установлены).
pub fn ensure_available() -> Result<()> {
    if proc::tool_available("lpstat").is_none() {
        return Err(Error::ToolMissing {
            tool: "lpstat".to_string(),
        });
    }
    // Проба работоспособности: `lpstat -r` возвращает0 при доступном CUPS.
    // `--version` часть сборок CUPS не поддерживает («unknown option»).
    let out = proc::run("lpstat", &["-r"], TIMEOUT)?;
    if out.status != 0 {
        return Err(Error::ToolFailed {
            tool: "lpstat".into(),
            code: out.status,
            message: out.stderr.trim().to_string(),
        });
    }
    Ok(())
}

/// Работает ли планировщик CUPS (`lpstat -r`).
pub fn scheduler_running() -> Result<bool> {
    ensure_available()?;
    let out = proc::run("lpstat", &["-r"], TIMEOUT)?;
    Ok(out.stdout_str().contains("scheduler is running"))
}

/// Имя принтера по умолчанию (глобальное, `lpstat -d`).
pub fn system_default_printer() -> Result<Option<String>> {
    let out = proc::run("lpstat", &["-d"], TIMEOUT).unwrap_or(proc::CmdOutput {
        status: 1,
        stdout: Vec::new(),
        stderr: String::new(),
    });
    Ok(parse_default_printer(&out.stdout_str()))
}

/// Пользовательский принтер по умолчанию (`lpoptions -d`).
pub fn user_default_printer() -> Result<Option<String>> {
    let out = proc::run("lpoptions", &["-d"], TIMEOUT).unwrap_or(proc::CmdOutput {
        status: 1,
        stdout: Vec::new(),
        stderr: String::new(),
    });
    if out.status != 0 {
        return Ok(None);
    }
    let line = out.stdout_str();
    let line = line.trim().to_string();
    Ok(line.strip_prefix("dest=").map(str::to_string))
}

/// Установить принтер по умолчанию для текущего пользователя (без root:
/// `lpoptions -d` пишет в ~/.cups/lpoptions).
pub fn set_user_default_printer(name: &str) -> Result<()> {
    ensure_available()?;
    proc::run("lpoptions", &["-d", name], TIMEOUT)?.ensure_ok("lpoptions")?;
    Ok(())
}

/// Список принтеров с состояниями.
pub fn list_printers() -> Result<Vec<Printer>> {
    ensure_available()?;
    let p = proc::run("lpstat", &["-p"], TIMEOUT)?;
    // `lpstat -p` возвращает ненулевой код, если принтеров нет — это не ошибка.
    let v = proc::run("lpstat", &["-v"], TIMEOUT).unwrap_or(proc::CmdOutput {
        status: 1,
        stdout: Vec::new(),
        stderr: String::new(),
    });
    let default_name = system_default_printer().ok().flatten();
    Ok(parse_printers(
        &p.stdout_str(),
        &v.stdout_str(),
        default_name.as_deref(),
    ))
}

/// Очередь печати: задания всех принтеров — ожидающие и последние выполненные.
///
/// Сначала пробуется **IPP** ([`ipp::jobs`]): точные состояния заданий
/// («печатается», «приостановлено», «выполнено»). При любой ошибке —
/// fallback на `lpstat -W … -o`, который различает только грубые
/// `Pending`/`Completed`, но работает всегда, где есть CUPS-клиент.
/// Выполненные ограничены [`ipp::COMPLETED_LIMIT`].
pub fn queue() -> Result<Vec<Job>> {
    ensure_available()?;
    match ipp::jobs() {
        Ok(jobs) => return Ok(jobs),
        Err(e) => tracing::debug!("IPP-статусы недоступны, используется lpstat: {e}"),
    }

    let pending = query_jobs("-W", "not-completed", JobState::Pending)?;
    let mut completed = query_jobs("-W", "completed", JobState::Completed)?;
    completed.truncate(ipp::COMPLETED_LIMIT);

    let mut jobs = pending;
    jobs.extend(completed);
    jobs.sort_by_key(|j| (j.state.is_terminal(), std::cmp::Reverse(j.id)));
    Ok(jobs)
}

/// Запрос заданий одной категории через `lpstat` (fallback-путь).
/// `state` — какое состояние проставлять: lpstat не сообщает подробностей.
fn query_jobs(flag: &str, value: &str, state: JobState) -> Result<Vec<Job>> {
    let out = proc::run("lpstat", &[flag, value, "-o"], TIMEOUT)?;
    if out.status != 0 {
        let text = out.stderr.trim();
        // «No jobs in system» — не ошибка.
        if text.is_empty() || text.contains("No jobs") {
            return Ok(Vec::new());
        }
        return Err(Error::ToolFailed {
            tool: "lpstat".into(),
            code: out.status,
            message: text.to_string(),
        });
    }
    Ok(parse_jobs(&out.stdout_str(), state))
}

/// Отправить файл на печать. Возвращает идентификатор задания.
pub fn submit(req: &PrintRequest) -> Result<u32> {
    ensure_available()?;
    if req.printer.trim().is_empty() {
        return Err(Error::InvalidInput("не выбран принтер".into()));
    }
    if !req.file.is_file() {
        return Err(Error::InvalidInput(format!(
            "файл не найден: {}",
            req.file.display()
        )));
    }
    let copies = req.copies.clamp(1, 999);

    let mut args: Vec<OsString> = vec!["-d".into(), req.printer.clone().into()];
    args.push("-n".into());
    args.push(copies.to_string().into());
    if !req.title.trim().is_empty() {
        args.push("-t".into());
        args.push(req.title.trim().into());
    }
    for (key, value) in build_options(req)? {
        args.push("-o".into());
        args.push(format!("{key}={value}").into());
    }
    args.push("--".into());
    args.push(req.file.clone().into_os_string());

    let out = proc::run("lp", &args, SUBMIT_TIMEOUT)?;
    if out.status != 0 {
        return Err(classify_lp_error(&out.stderr));
    }
    match parse_job_id(&out.stdout_str()) {
        Some(id) => Ok(id),
        None => {
            tracing::warn!(
                "не удалось разобрать идентификатор задания: {}",
                out.stdout_str()
            );
            Ok(0)
        }
    }
}

/// Отмена задания по его токену вида `PRINTER-23`.
pub fn cancel_job(token: &str) -> Result<()> {
    ensure_available()?;
    let token = token.trim();
    if token.is_empty() || token.contains(['/', ' ']) {
        return Err(Error::InvalidInput(format!(
            "некорректный id задания: {token}"
        )));
    }
    proc::run("cancel", &[token], TIMEOUT)?.ensure_ok("cancel")?;
    Ok(())
}

/// Поддерживаемые опции принтера из `lpoptions -p <name> -l`:
/// `PageSize -> [A3, A4, ...]`, `Duplex -> [...]` и т. д.
pub fn printer_options(name: &str) -> Result<BTreeMap<String, Vec<String>>> {
    ensure_available()?;
    let out = proc::run("lpoptions", &["-p", name, "-l"], TIMEOUT)?;
    if out.status != 0 {
        return Err(Error::ToolFailed {
            tool: "lpoptions".into(),
            code: out.status,
            message: out.stderr.trim().to_string(),
        });
    }
    Ok(parse_lpoptions_list(&out.stdout_str()))
}

/// Преобразовать запрос печати в список CUPS-опций `-o key=value`.
///
/// Выборы (`media`, `sides`, кассета, тип носителя) сверяются с реальными
/// возможностями принтера, если их удалось получить: неизвестные значения
/// отклоняются заранее с понятной ошибкой вместо неудачи в `lp`.
pub fn build_options(req: &PrintRequest) -> Result<Vec<(String, String)>> {
    let mut opts: Vec<(String, String)> = Vec::new();

    if let Some(range) = &req.page_range {
        let pr = PageRange::parse(range)?;
        if let Some(s) = pr.to_lp_string() {
            opts.push(("page-ranges".into(), s));
        }
    }
    if let Some(media) = &req.media {
        if !media.trim().is_empty() {
            opts.push(("media".into(), media.trim().to_string()));
        }
    }
    if req.sides.is_duplex() {
        opts.push(("sides".into(), req.sides.as_cups().to_string()));
    }
    opts.push((
        "print-color-mode".into(),
        req.color_mode.as_cups().to_string(),
    ));
    opts.push(("print-quality".into(), req.quality.as_cups().to_string()));
    opts.push((
        "orientation-requested".into(),
        req.orientation.as_cups().to_string(),
    ));
    if req.copies > 1 {
        opts.push(("Collate".into(), "true".into()));
    }
    if let Some(tray) = &req.tray {
        if !tray.trim().is_empty() {
            opts.push(("InputSlot".into(), tray.trim().to_string()));
        }
    }
    if let Some(mt) = &req.media_type {
        if !mt.trim().is_empty() {
            opts.push(("MediaType".into(), mt.trim().to_string()));
        }
    }
    // Отложенная печать (job-hold-until): именованные значения или HH:MM.
    if let Some(h) = req.hold_until.as_deref().map(str::trim) {
        if !h.is_empty() {
            opts.push(("job-hold-until".into(), h.to_string()));
        }
    }

    // Ленивая сверка с поддерживаемыми опциями принтера.
    if let Ok(supported) = printer_options(&req.printer) {
        validate_choice(&supported, "PageSize", req.media.as_deref(), &req.printer)?;
        validate_choice(&supported, "InputSlot", req.tray.as_deref(), &req.printer)?;
        validate_choice(
            &supported,
            "MediaType",
            req.media_type.as_deref(),
            &req.printer,
        )?;
        // Ручное время (HH:MM) в списке choices отсутствует по определению.
        if let Some(h) = req.hold_until.as_deref().map(str::trim) {
            if !h.is_empty() && !h.contains(':') {
                validate_choice(&supported, "job-hold-until", Some(h), &req.printer)?;
            }
        }
        if req.sides.is_duplex() {
            validate_choice(
                &supported,
                "Duplex",
                Some(req.sides.as_cups()),
                &req.printer,
            )?;
        }
        // Подгонка под лист (print-scaling): отправляем только когда принтер
        // опцию заявляет, иначе молча пропускаем — «не мешаем».
        push_print_scaling(
            &mut opts,
            &supported,
            req.print_scaling.as_deref(),
            &req.printer,
        )?;
    }
    Ok(opts)
}

/// Добавить `print-scaling`, если принтер опцию заявляет, сверив значение
/// со списком поддерживаемых choices. Необъявленная опция не отправляется
/// (даже если значение валидно), пустое значение игнорируется.
fn push_print_scaling(
    opts: &mut Vec<(String, String)>,
    supported: &BTreeMap<String, Vec<String>>,
    value: Option<&str>,
    printer: &str,
) -> Result<()> {
    let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(());
    };
    if !supported.contains_key("print-scaling") {
        return Ok(());
    }
    validate_choice(supported, "print-scaling", Some(value), printer)?;
    opts.push(("print-scaling".into(), value.to_string()));
    Ok(())
}

fn validate_choice(
    supported: &BTreeMap<String, Vec<String>>,
    option: &str,
    value: Option<&str>,
    printer: &str,
) -> Result<()> {
    let Some(value) = value else { return Ok(()) };
    let Some(choices) = supported.get(option) else {
        // Принтер не заявляет опцию — не мешаем, CUPS сам решит.
        return Ok(());
    };
    if choices.iter().any(|c| c == value) {
        return Ok(());
    }
    Err(Error::InvalidInput(format!(
        "принтер {printer} не поддерживает {option}={value}; доступно: {}",
        choices.join(", ")
    )))
}

/// Стратегия «Добавить принтер»: сначала системный диалог, затем веб-интерфейс.
pub enum AddPrinterStrategy {
    /// Запустить графический мастер (он сам попросит polkit при необходимости).
    SystemConfigPrinter,
    /// Открыть веб-интерфейс CUPS.
    CupsWebUi(String),
    /// Ничего доступного нет — показать инструкцию.
    Manual(String),
}

pub fn add_printer_strategy() -> AddPrinterStrategy {
    if proc::tool_available("system-config-printer").is_some() {
        return AddPrinterStrategy::SystemConfigPrinter;
    }
    let base = "http://localhost:631".to_string();
    if proc::tool_available("xdg-open").is_some() {
        return AddPrinterStrategy::CupsWebUi(format!("{base}/admin"));
    }
    AddPrinterStrategy::Manual(format!(
        "{base}/admin — откройте этот адрес в браузере и добавьте принтер; \
         либо установите пакет system-config-printer."
    ))
}

// ---------------------------------------------------------------------
// ICC-профили драйвера (PPD `cupsICCProfile`)
// ---------------------------------------------------------------------

/// Порт веб-интерфейса локального cupsd (HTTP).
const CUPS_WEB_PORT: u16 = 631;

/// Описания ICC-профилей, объявленных в PPD принтера
/// (`*cupsICCProfile Селектор/Описание: "файл"`), в порядке объявления,
/// без дублей.
///
/// Скачивается анонимным GET `http://localhost:631/printers/<name>.ppd` —
/// так PPD доступен и без группы `lp` (файл `/etc/cups/ppd/` обычно
/// читает только root/cupsd). Профили применяются автоматически стеком
/// CUPS (cupsd регистрирует их у colord) — приложение их только показывает;
/// выбор произвольного ICC-файла через `lp` не предусмотрен ни CUPS, ни
/// спецификацией IPP (см. CUPS PPD Extensions, `cupsICCProfile`).
///
/// Пустой список — норма: driverless-принтеры (IPP Everywhere) PPD-профили
/// не объявляют.
pub fn ppd_icc_profiles(printer: &str) -> Result<Vec<String>> {
    let name = printer.trim();
    if name.is_empty() || name.contains('/') || name.contains("..") {
        return Err(Error::InvalidInput(format!(
            "некорректное имя принтера: {printer}"
        )));
    }
    let path = format!("/printers/{}.ppd", encode_path_component(name));
    let body = ipp::http_get("localhost", CUPS_WEB_PORT, &path)?;
    Ok(parse_ppd_icc(&String::from_utf8_lossy(&body)))
}

/// Percent-кодирование компонента URL-пути: все байты, кроме
/// нерезервированных `[A-Za-z0-9-._~]`.
pub fn encode_path_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Разобрать PPD-текст и собрать уникальные описания `cupsICCProfile`.
///
/// Формат строки: `*cupsICCProfile Селектор/Описание: "путь"`. В реальных
/// PPD (например, HP) строки бывают только с языковым префиксом
/// (`*ru.cupsICCProfile …`) — такие тоже читаем: описание профиля от
/// локали не зависит, дубли отбрасываются.
fn parse_ppd_icc(ppd: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in ppd.lines() {
        let Some(rest) = line.strip_prefix('*') else {
            continue;
        };
        let rest = match rest.strip_prefix("cupsICCProfile ") {
            Some(r) => r,
            None => {
                // Локализованная строка: *<locale>.cupsICCProfile …
                let Some((locale, tail)) = rest.split_once('.') else {
                    continue;
                };
                if locale.is_empty()
                    || !locale
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    continue;
                }
                match tail.strip_prefix("cupsICCProfile ") {
                    Some(t) => t,
                    None => continue,
                }
            }
        };
        // rest = `Селектор/Описание: "путь"` (путь не нужен).
        let Some((selector_desc, _)) = rest.split_once(':') else {
            continue;
        };
        let Some((_, desc)) = selector_desc.split_once('/') else {
            continue;
        };
        let desc = desc.trim();
        if !desc.is_empty() && !out.iter().any(|d| d == desc) {
            out.push(desc.to_string());
        }
    }
    out
}

// ---------------------------------------------------------------------
// Разбор вывода утилит (чистые функции, покрыты тестами)
// ---------------------------------------------------------------------

fn parse_printers(lpstat_p: &str, lpstat_v: &str, default: Option<&str>) -> Vec<Printer> {
    let uris = parse_device_uris(lpstat_v);
    let mut printers: Vec<Printer> = Vec::new();
    for line in lpstat_p.lines() {
        let mut parts = line.split_whitespace();
        if parts.next() != Some("printer") {
            continue;
        }
        let Some(name) = parts.next() else { continue };
        let state = if line.contains("is idle") {
            PrinterState::Idle
        } else if line.contains("printing") {
            PrinterState::Printing
        } else if line.contains("disabled") {
            PrinterState::Disabled
        } else {
            PrinterState::Unknown
        };
        // Описание: всё после "printer NAME is ..." до точки состояния.
        let info = uris.get(name).cloned().unwrap_or_default();
        printers.push(Printer {
            name: name.to_string(),
            state,
            is_default: default == Some(name),
            uri: info,
            info: String::new(),
        });
    }
    printers.sort_by_key(|p| p.name.to_lowercase());
    printers
}

fn parse_device_uris(lpstat_v: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in lpstat_v.lines() {
        // "device for NAME: usb://Vendor/Product?serial=...":
        if let Some(rest) = line.strip_prefix("device for ") {
            if let Some((name, uri)) = rest.split_once(':') {
                map.insert(name.trim().to_string(), uri.trim().to_string());
            }
        }
    }
    map
}

fn parse_default_printer(lpstat_d: &str) -> Option<String> {
    // "system default destination: NAME"
    let line = lpstat_d
        .lines()
        .find(|l| l.contains("system default destination"))?;
    line.split(':')
        .nth(1)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn parse_jobs(lpstat_o: &str, state: JobState) -> Vec<Job> {
    let mut jobs = Vec::new();
    for line in lpstat_o.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() < 3 {
            continue;
        }
        let token = tokens[0];
        let Some((printer, id)) = token.rsplit_once('-') else {
            continue;
        };
        let Ok(id) = id.parse::<u32>() else { continue };
        let size = tokens[2].parse::<u64>().unwrap_or(0);
        jobs.push(Job {
            token: token.to_string(),
            id,
            printer: printer.to_string(),
            owner: tokens[1].to_string(),
            size,
            state,
            sheets_done: 0, // lpstat не сообщает прогресс — только IPP
            sheets_total: 0,
            printer_uri: String::new(), // lpstat не сообщает URI принтера
        });
    }
    jobs.sort_by_key(|j| std::cmp::Reverse(j.id));
    jobs
}

fn parse_job_id(lp_stdout: &str) -> Option<u32> {
    // "request id is NAME-23 (1 file(s))"
    let re = Regex::new(r"request id is \S+-(\d+)").ok()?;
    let caps = re.captures(lp_stdout)?;
    caps.get(1)?.as_str().parse().ok()
}

fn parse_lpoptions_list(out: &str) -> BTreeMap<String, Vec<String>> {
    let mut map = BTreeMap::new();
    for line in out.lines() {
        // "PageSize/Media Size: *A4 A5 Letter ..."
        let Some((key_part, choices_part)) = line.split_once(':') else {
            continue;
        };
        let key = key_part.split('/').next().unwrap_or(key_part).trim();
        if key.is_empty() {
            continue;
        }
        let choices: Vec<String> = choices_part
            .split_whitespace()
            .map(|c| c.trim_start_matches('*').to_string())
            .filter(|c| !c.is_empty())
            .collect();
        if !choices.is_empty() {
            map.insert(key.to_string(), choices);
        }
    }
    map
}

fn classify_lp_error(stderr: &str) -> Error {
    let text = stderr.trim();
    let lower = text.to_ascii_lowercase();
    if lower.contains("not accept jobs") || lower.contains("disabled") {
        Error::msg(format!(
            "{text}. Принтер остановлен — проверьте его состояние и бумагу."
        ))
    } else if lower.contains("connection refused") || lower.contains("offline") {
        Error::msg(format!(
            "{text}. Принтер недоступен по сети — проверьте соединение."
        ))
    } else if lower.contains("permission") || lower.contains("not allowed") {
        Error::Permission(text.to_string())
    } else if lower.contains("unknown destination") {
        Error::InvalidInput(format!("принтер не найден: {text}"))
    } else if text.is_empty() {
        Error::msg("lp завершился с ошибкой без описания")
    } else {
        Error::msg(text.to_string())
    }
}

/// Проверить, что файл существует и похож на документ/изображение, которое CUPS
/// сможет принять (расширение — подсказка, финальное решение за CUPS).
pub fn check_printable_file(path: &Path) -> Result<()> {
    if !path.is_file() {
        return Err(Error::InvalidInput(format!(
            "файл не найден: {}",
            path.display()
        )));
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    const ALLOWED: &[&str] = &[
        "pdf", "png", "jpg", "jpeg", "tif", "tiff", "webp", "bmp", "svg", "ps", "eps", "txt",
        "doc", "docx", "odt", "rtf", "ppt", "pptx", "ods", "xls", "xlsx",
    ];
    if !ALLOWED.contains(&ext.as_str()) {
        return Err(Error::InvalidInput(format!(
            "формат .{ext} не поддерживается для печати; сохраните документ как PDF"
        )));
    }
    Ok(())
}

/// Признаки офисного формата, требующего конвертации через LibreOffice.
pub fn is_office_document(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "doc" | "docx" | "odt" | "rtf" | "ppt" | "pptx" | "ods" | "xls" | "xlsx"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lpstat_p_and_v() {
        let p = "\
printer MFC-L2700DW is idle. enabled since Sat 17 Oct 2026 10:00:00 AM
printer HP_LaserJet is printing job-123. enabled since Sat 17 Oct 2026 11:00:00 AM
printer OldPrinter is disabled since Sat 17 Oct 2026 09:00:00 AM
";
        let v = "\
device for MFC-L2700DW: ipp://192.168.1.10:631/printers/MFC
device for HP_LaserJet: usb://HP/LaserJet?serial=ABC
";
        let printers = parse_printers(p, v, Some("MFC-L2700DW"));
        assert_eq!(printers.len(), 3);
        let mfc = &printers[0]; // сортировка по алфавиту: HP..., MFC..., Old...
        assert_eq!(mfc.name, "HP_LaserJet");
        assert_eq!(mfc.state, PrinterState::Printing);
        let mfc = printers.iter().find(|p| p.name == "MFC-L2700DW").unwrap();
        assert!(mfc.is_default);
        assert_eq!(mfc.uri, "ipp://192.168.1.10:631/printers/MFC");
        let old = printers.iter().find(|p| p.name == "OldPrinter").unwrap();
        assert_eq!(old.state, PrinterState::Disabled);
    }

    #[test]
    fn parses_jobs() {
        let out = "\
MFC-L2700DW-23 root 4096 Sat 17 Oct 2026 12:00:00
HP_LaserJet-7 user 0 Sat 17 Oct 2026 12:05:00
";
        let jobs = parse_jobs(out, JobState::Pending);
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].token, "MFC-L2700DW-23");
        assert_eq!(jobs[0].id, 23);
        assert_eq!(jobs[0].printer, "MFC-L2700DW");
        assert_eq!(jobs[0].owner, "root");
        assert_eq!(jobs[0].size, 4096);
        assert_eq!(jobs[0].state, JobState::Pending, "активное — Pending");
        assert!(!jobs[0].state.is_terminal());
        assert_eq!(jobs[1].id, 7);

        // Задания из `-W completed` помечаются выполненными.
        let done_jobs = parse_jobs(out, JobState::Completed);
        assert!(done_jobs.iter().all(|j| j.state == JobState::Completed));
        assert!(done_jobs.iter().all(|j| j.state.is_terminal()));
    }

    #[test]
    fn parses_job_id_from_lp() {
        assert_eq!(
            parse_job_id("request id is MFC-L2700DW-42 (1 file(s))\n"),
            Some(42)
        );
        assert_eq!(parse_job_id("что-то не то"), None);
    }

    #[test]
    fn parses_lpoptions() {
        let out = "\
PageSize/Media Size: *A4 A5 Letter A6
Duplex/Duplex: None *DuplexNoTumble DuplexTumble
InputSlot/Media Source: *Auto Tray1 Tray2
MediaType/Media Type: *Plain Transparency
ColorModel/Color Mode: *RGB Grayscale KGray
";
        let map = parse_lpoptions_list(out);
        assert_eq!(map["PageSize"], vec!["A4", "A5", "Letter", "A6"]);
        assert_eq!(
            map["Duplex"],
            vec!["None", "DuplexNoTumble", "DuplexTumble"]
        );
        assert_eq!(map["InputSlot"][1], "Tray1");
        assert_eq!(map["ColorModel"], vec!["RGB", "Grayscale", "KGray"]);
    }

    #[test]
    fn default_printer_parsed() {
        assert_eq!(
            parse_default_printer("system default destination: MyPrinter\n"),
            Some("MyPrinter".to_string())
        );
        assert_eq!(
            parse_default_printer("no system default destination\n"),
            None
        );
    }

    #[test]
    fn builds_options_with_validation() {
        let req = PrintRequest {
            printer: "X".into(),
            file: "/tmp".into(),
            title: "Doc".into(),
            copies: 2,
            page_range: Some("1-2".into()),
            media: Some("A4".into()),
            sides: crate::domain::Sides::TwoSidedLongEdge,
            color_mode: Default::default(),
            quality: Default::default(),
            orientation: Default::default(),
            tray: None,
            media_type: None,
            hold_until: Some("evening".into()),
            print_scaling: None,
        };
        // В тестовом окружении нет lpoptions — сверка пропускается.
        let opts = build_options(&req).unwrap();
        let map: std::collections::BTreeMap<_, _> = opts.into_iter().collect();
        assert_eq!(map["page-ranges"], "1-2");
        assert_eq!(map["media"], "A4");
        assert_eq!(map["sides"], "two-sided-long-edge");
        assert_eq!(map["print-color-mode"], "color");
        assert_eq!(map["Collate"], "true");
        assert_eq!(map["job-hold-until"], "evening");
    }

    #[test]
    fn empty_hold_until_not_sent() {
        let req = PrintRequest {
            printer: "X".into(),
            file: "/tmp".into(),
            title: "Doc".into(),
            copies: 1,
            page_range: None,
            media: None,
            sides: Default::default(),
            color_mode: Default::default(),
            quality: Default::default(),
            orientation: Default::default(),
            tray: None,
            media_type: None,
            hold_until: Some("   ".into()),
            print_scaling: None,
        };
        let opts = build_options(&req).unwrap();
        assert!(
            !opts.iter().any(|(k, _)| k == "job-hold-until"),
            "пустая задержка не должна отправляться"
        );
    }

    #[test]
    fn rejects_bad_tokens_and_files() {
        assert!(cancel_job("../etc/passwd").is_err());
        assert!(cancel_job("a b").is_err());
        assert!(check_printable_file(Path::new("/nonexistent/x.pdf")).is_err());
        assert!(is_office_document(Path::new("report.docx")));
        assert!(!is_office_document(Path::new("photo.png")));
    }

    fn scaling_map(choices: &[&str]) -> BTreeMap<String, Vec<String>> {
        if choices.is_empty() {
            return BTreeMap::new();
        }
        let mut m = BTreeMap::new();
        m.insert(
            "print-scaling".to_string(),
            choices.iter().map(|s| s.to_string()).collect(),
        );
        m
    }

    #[test]
    fn print_scaling_sent_when_printer_supports_it() {
        let supported = scaling_map(&["auto", "fit", "fill", "none"]);
        let mut opts = Vec::new();
        push_print_scaling(&mut opts, &supported, Some("fit"), "X").unwrap();
        assert_eq!(opts, vec![("print-scaling".to_string(), "fit".to_string())]);
    }

    #[test]
    fn print_scaling_skipped_when_not_declared() {
        // Принтер не заявляет опцию — значение (даже валидное) не мешаем.
        let supported = scaling_map(&[]);
        let mut opts = Vec::new();
        push_print_scaling(&mut opts, &supported, Some("fit"), "X").unwrap();
        assert!(opts.is_empty());
        // Пустое значение тоже не отправляется.
        let supported = scaling_map(&["auto", "fit"]);
        let mut opts = Vec::new();
        push_print_scaling(&mut opts, &supported, Some("   "), "X").unwrap();
        push_print_scaling(&mut opts, &supported, None, "X").unwrap();
        assert!(opts.is_empty());
    }

    #[test]
    fn print_scaling_rejects_unknown_value() {
        let supported = scaling_map(&["auto", "fit", "fill", "none"]);
        let mut opts = Vec::new();
        let err = push_print_scaling(&mut opts, &supported, Some("stretch"), "X").unwrap_err();
        assert!(err.to_string().contains("print-scaling=stretch"));
        assert!(opts.is_empty(), "неверное значение не должно отправляться");
    }

    #[test]
    fn ppd_icc_parsed_from_base_lines() {
        // Формат из CUPS PPD Extensions (базовые строки без локали).
        let ppd = r#"
*% comment
*cupsICCProfile CMYK..360dpi/360dpi CMYK: "/Library/Printers/vendor/Profiles/foo-360-cmyk.icc"
*cupsICCProfile RGB.Glossy.720dpi/720dpi Glossy: "/path/foo-720-glossy-rgb.icc"
*cupsICCProfile ../Default: "/path/foo-default.icc"
*OpenUI *PageSize/Page Size: PickOne
"#;
        assert_eq!(
            parse_ppd_icc(ppd),
            vec!["360dpi CMYK", "720dpi Glossy", "Default"]
        );
    }

    #[test]
    fn ppd_icc_parsed_from_localized_lines_with_dedup() {
        // Реальные строки HP Color LaserJet PS: только языковые префиксы,
        // описания повторяются между локалями.
        let ppd = "\
*da.cupsICCProfile RGB../sRGB Matching Profile: \"\"
*da.cupsICCProfile CMYK../CMYK Matching Profile: \"\"
*de.cupsICCProfile RGB../sRGB Matching Profile: \"\"
*de.cupsICCProfile Gray../Gray Matching Profile: \"\"
*OpenUI *ColorModel/Color Mode: PickOne
";
        assert_eq!(
            parse_ppd_icc(ppd),
            vec![
                "sRGB Matching Profile",
                "CMYK Matching Profile",
                "Gray Matching Profile"
            ]
        );
        // Чужие атрибуты с точкой не принимаются.
        assert!(parse_ppd_icc("*ru.media.Tray/Лоток: \"x\"").is_empty());
    }

    #[test]
    fn path_component_percent_encoded() {
        assert_eq!(encode_path_component("G3030"), "G3030");
        assert_eq!(encode_path_component("My Printer"), "My%20Printer");
        assert_eq!(encode_path_component("a/b"), "a%2Fb");
        // Нерезервированные байты UTF-8 кодируются по байтам.
        assert_eq!(
            encode_path_component("Принтер"),
            "%D0%9F%D1%80%D0%B8%D0%BD%D1%82%D0%B5%D1%80"
        );
    }

    #[test]
    fn icc_profiles_reject_bad_printer_names() {
        assert!(ppd_icc_profiles("").is_err());
        assert!(ppd_icc_profiles("../etc").is_err());
        assert!(ppd_icc_profiles("a/b").is_err());
    }

    #[test]
    #[ignore = "нужен запущенный локальный CUPS"]
    fn live_ppd_icc_profiles_from_local_cups() {
        // Driverless-принтер может не объявлять профилей — важно, что
        // HTTP GET PPD работает без прав группы lp.
        let profiles = ppd_icc_profiles("G3030").expect("PPD должен скачиваться анонимно");
        eprintln!("ICC-профили G3030: {profiles:?}");
    }
}
