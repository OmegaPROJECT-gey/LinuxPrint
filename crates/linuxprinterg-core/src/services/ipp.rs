//! Минимальный IPP-клиент к локальному серверу CUPS (RFC 8010/8011).
//!
//! Зачем: точные состояния заданий (`job-state`), которых не даёт `lpstat`:
//! «печатается», «приостановлено», «выполнено», «сбой» и т. д. Только
//! чтение (Get-Jobs) — отправка и отмена заданий остаются за проверенными
//! `lp`/`cancel`.
//!
//! Транспорт: HTTP POST `application/ipp` к локальному CUPS — доменный
//! сокет (`/run/cups/cups.sock`) либо TCP `127.0.0.1:631`; сервер
//! переопределяется переменной `CUPS_SERVER` (`/путь.сокет`,
//! `host[:port]`, `ipp://host[:port]`).
//!
//! Кодирование вручную, без новых зависимостей. Все имена и длины
//! считаются программно: ручная ошибка длины ломает разбор (это выяснялось
//! «вживую» против cupsd), поэтому структура покрыта roundtrip-тестами и
//! фикстурой захваченного запроса libcups (`lpstat`).

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use crate::domain::{Job, JobState};
use crate::error::{Error, Result};

/// Таймаут соединения/чтения/записи.
const TIMEOUT: Duration = Duration::from_secs(5);
/// Сколько последних выполненных заданий запрашивать (как и в lpstat-пути).
pub const COMPLETED_LIMIT: usize = 10;

/// Целевой сервер CUPS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Tcp { host: String, port: u16 },
    Unix(PathBuf),
}

/// Сервер по умолчанию: `CUPS_SERVER` → доменный сокет → `127.0.0.1:631`.
pub fn default_target() -> Target {
    if let Ok(spec) = std::env::var("CUPS_SERVER") {
        if let Some(t) = parse_server_name(&spec) {
            return t;
        }
    }
    let sock = PathBuf::from("/run/cups/cups.sock");
    if sock.exists() {
        return Target::Unix(sock);
    }
    Target::Tcp {
        host: "127.0.0.1".into(),
        port: 631,
    }
}

/// Разбор имени сервера CUPS: `/путь/к.сокету`, `host[:port]`,
/// `ipp://host[:port]`. `None` — пустое/некорректное имя.
pub fn parse_server_name(spec: &str) -> Option<Target> {
    let spec = spec
        .strip_prefix("ipp://")
        .or_else(|| spec.strip_prefix("ipps://"))
        .unwrap_or(spec);
    if spec.starts_with('/') {
        return Some(Target::Unix(PathBuf::from(spec)));
    }
    let (host, port) = match spec.split_once(':') {
        // Псле двоеточия обязан быть числовый порт, иначе имя некорректно.
        Some((h, p)) => (h, p.parse().ok()?),
        None => (spec, 631),
    };
    if host.is_empty() {
        return None;
    }
    Some(Target::Tcp {
        host: host.to_string(),
        port,
    })
}

/// WhichJobs (RFC 8011 §5.3.8.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhichJobs {
    NotCompleted,
    Completed,
}

impl WhichJobs {
    fn as_str(self) -> &'static str {
        match self {
            Self::NotCompleted => "not-completed",
            Self::Completed => "completed",
        }
    }
}

// --- Кодирование ----------------------------------------------------------

fn put_u16(buf: &mut Vec<u8>, v: usize) {
    buf.extend_from_slice(&(v as u16).to_be_bytes());
}

/// Первое значение атрибута: `tag, name-length, name, value-length, value`.
fn put_attr(buf: &mut Vec<u8>, tag: u8, name: &str, value: &[u8]) {
    buf.push(tag);
    put_u16(buf, name.len());
    buf.extend_from_slice(name.as_bytes());
    put_u16(buf, value.len());
    buf.extend_from_slice(value);
}

/// Дополнительное значение существующего атрибута: `tag, name-length=0,
/// value-length, value` (структура сверена с запросом libcups).
fn put_more(buf: &mut Vec<u8>, tag: u8, value: &[u8]) {
    buf.push(tag);
    put_u16(buf, 0);
    put_u16(buf, value.len());
    buf.extend_from_slice(value);
}

/// Запрос Get-Jobs (0x000A): структура проверена против захваченного
/// запроса libcups (`lpstat -W completed -o`) и живого cupsd.
pub fn encode_get_jobs(user: Option<&str>, which: WhichJobs) -> Vec<u8> {
    let mut b = Vec::with_capacity(256);
    b.extend_from_slice(&[0x02, 0x00]); // IPP 2.0
    b.extend_from_slice(&0x000Au16.to_be_bytes()); // Get-Jobs
    b.extend_from_slice(&1u32.to_be_bytes()); // request-id
    b.push(0x01); // operation-attributes-tag
    // Обязательная пара атрибутов любого IPP-запроса.
    put_attr(&mut b, 0x47, "attributes-charset", b"utf-8");
    put_attr(&mut b, 0x48, "attributes-natural-language", b"en-us");
    put_attr(&mut b, 0x45, "printer-uri", b"ipp://localhost/");
    // Запрашиваем ровно те поля, что нужны UI (как lpstat).
    put_attr(&mut b, 0x44, "requested-attributes", b"job-id");
    put_more(&mut b, 0x44, b"job-state");
    put_more(&mut b, 0x44, b"job-name");
    put_more(&mut b, 0x44, b"job-k-octets");
    put_more(&mut b, 0x44, b"job-originating-user-name");
    put_more(&mut b, 0x44, b"job-printer-uri");
    put_more(&mut b, 0x44, b"job-state-reasons");
    // Прогресс печати: отпечатано/всего листов (RFC 8011 §5.3.17/5.3.20).
    put_more(&mut b, 0x44, b"job-media-sheets-completed");
    put_more(&mut b, 0x44, b"job-media-sheets");
    if let Some(u) = user.filter(|u| !u.trim().is_empty()) {
        put_attr(&mut b, 0x42, "requesting-user-name", u.as_bytes());
    }
    put_attr(&mut b, 0x44, "which-jobs", which.as_str().as_bytes());
    b.push(0x03); // end-of-attributes
    b
}

/// Операции IPP над заданием (RFC 8011 §5.6.2–5.6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobOp {
    /// Cancel-Job (0x0008) — отмена задания.
    Cancel,
    /// Hold-Job (0x000C) — приостановить (pending → held).
    Hold,
    /// Release-Job (0x000D) — продолжить (held → pending).
    Release,
    /// Restart-Job (0x000E) — перезапустить завершённое/отменённое.
    Restart,
}

impl JobOp {
    /// Код операции (operation-id запроса).
    pub const fn code(self) -> u16 {
        match self {
            Self::Cancel => 0x0008,
            Self::Hold => 0x000C,
            Self::Release => 0x000D,
            Self::Restart => 0x000E,
        }
    }

    /// Имя операции (для журнала и сообщений).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cancel => "cancel",
            Self::Hold => "hold",
            Self::Release => "release",
            Self::Restart => "restart",
        }
    }
}

/// Запрос операции над заданием: `printer-uri` + `job-id`
/// (состав сверен с обязательными атрибутами RFC 8011 §5.6.2).
pub fn encode_job_op(op: JobOp, printer_uri: &str, job_id: u32, user: Option<&str>) -> Vec<u8> {
    let mut b = Vec::with_capacity(160);
    b.extend_from_slice(&[0x02, 0x00]); // IPP 2.0
    b.extend_from_slice(&op.code().to_be_bytes());
    b.extend_from_slice(&1u32.to_be_bytes()); // request-id
    b.push(0x01); // operation-attributes-tag
    put_attr(&mut b, 0x47, "attributes-charset", b"utf-8");
    put_attr(&mut b, 0x48, "attributes-natural-language", b"en-us");
    put_attr(&mut b, 0x45, "printer-uri", printer_uri.as_bytes());
    // job-id — integer (0x21), значение 4 байта big-endian.
    put_attr(&mut b, 0x21, "job-id", &job_id.to_be_bytes());
    if let Some(u) = user.filter(|u| !u.trim().is_empty()) {
        put_attr(&mut b, 0x42, "requesting-user-name", u.as_bytes());
    }
    b.push(0x03); // end-of-attributes
    b
}

/// Выполнить операцию над заданием через IPP.
///
/// Успешные статусы 0x0000–0x0002 (ok/warnings/ignored-class); прочие —
/// ошибка с кодом (например 0x0404, если задание уже не найдено).
pub fn job_op(op: JobOp, printer_uri: &str, job_id: u32) -> Result<()> {
    let user = std::env::var("USER").ok();
    let user = user.as_deref();
    let body = encode_job_op(op, printer_uri, job_id, user);
    let payload = http_post_ipp(&default_target(), &body)?;
    let msg = parse_ipp(&payload)?;
    if msg.status > 0x0002 {
        return Err(Error::ToolFailed {
            tool: "ipp".into(),
            code: msg.status as i32,
            message: format!(
                "операция «{}» отклонена: IPP status 0x{:04x}",
                op.as_str(),
                msg.status
            ),
        });
    }
    Ok(())
}

// --- Разбор ---------------------------------------------------------------

/// Атрибут IPP: имя, тег значения и одно или несколько значений.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IppAttr {
    pub name: String,
    pub tag: u8,
    pub values: Vec<Vec<u8>>,
}

impl IppAttr {
    /// Первое значение как текст (UTF-8 lossy).
    pub fn first_str(&self) -> Option<String> {
        self.values
            .first()
            .map(|v| String::from_utf8_lossy(v).into_owned())
    }

    /// Первое значение как число: integer (0x21) / boolean (0x22) /
    /// enum (0x23 — так cupsd отдаёт `job-state`), 4 байта BE.
    pub fn first_int(&self) -> Option<i64> {
        let v = self.values.first()?;
        match self.tag {
            0x21..=0x23 if v.len() == 4 => {
                Some(i32::from_be_bytes([v[0], v[1], v[2], v[3]]) as i64)
            }
            _ => None,
        }
    }
}

/// Группа атрибутов IPP-ответа: 0x01 operation, 0x02 job, 0x04 printer …
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IppGroup {
    pub tag: u8,
    pub attrs: Vec<IppAttr>,
}

/// Разобранное IPP-сообщение (запрос или ответ — формат одинаков).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IppMessage {
    /// operation-status (в ответе) или operation-id (в запросе).
    pub status: u16,
    pub groups: Vec<IppGroup>,
}

fn ipp_err(msg: &str) -> Error {
    Error::InvalidInput(format!("IPP: {msg}"))
}

fn read_u16(data: &[u8], pos: &mut usize) -> Result<u16> {
    if *pos + 2 > data.len() {
        return Err(ipp_err("обрыв при чтении длины"));
    }
    let v = u16::from_be_bytes([data[*pos], data[*pos + 1]]);
    *pos += 2;
    Ok(v)
}

fn read_bytes<'a>(data: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8]> {
    if *pos + n > data.len() {
        return Err(ipp_err("обрыв при чтении значения"));
    }
    let s = &data[*pos..*pos + n];
    *pos += n;
    Ok(s)
}

/// Разобрать IPP-сообщение. Любая несогласованность длин — ошибка, а не
/// «тихое» смещение (так cupsd отвечает400 без тела).
pub fn parse_ipp(data: &[u8]) -> Result<IppMessage> {
    if data.len() < 8 {
        return Err(ipp_err("сообщение короче заголовка"));
    }
    let status = u16::from_be_bytes([data[2], data[3]]);
    let mut pos = 8;
    let mut groups: Vec<IppGroup> = Vec::new();
    while pos < data.len() {
        let tag = data[pos];
        pos += 1;
        if tag == 0x03 {
            break; // end-of-attributes
        }
        if (0x01..=0x0f).contains(&tag) {
            groups.push(IppGroup {
                tag,
                attrs: Vec::new(),
            });
            continue;
        }
        let name_len = read_u16(data, &mut pos)? as usize;
        let name = String::from_utf8_lossy(read_bytes(data, &mut pos, name_len)?).into_owned();
        let val_len = read_u16(data, &mut pos)? as usize;
        let value = read_bytes(data, &mut pos, val_len)?.to_vec();
        let group = groups
            .last_mut()
            .ok_or_else(|| ipp_err("атрибут вне группы"))?;
        if name.is_empty() {
            // Дополнительное значение предыдущего атрибута.
            match group.attrs.last_mut() {
                Some(a) => a.values.push(value),
                None => return Err(ipp_err("дополнительное значение без атрибута")),
            }
        } else {
            group.attrs.push(IppAttr {
                name,
                tag,
                values: vec![value],
            });
        }
    }
    Ok(IppMessage { status, groups })
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Разделить HTTP-ответ на статус и тело: тело — по `Content-Length`
/// (заголовок без учёта регистра), без него — до конца потока.
pub fn split_http_response(resp: &[u8]) -> Result<(u16, Vec<u8>)> {
    let header_end =
        find_subsequence(resp, b"\r\n\r\n").ok_or_else(|| ipp_err("нет конца HTTP-заголовков"))?;
    let head =
        std::str::from_utf8(&resp[..header_end]).map_err(|_| ipp_err("заголовки не UTF-8"))?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| ipp_err("нет кода статуса HTTP"))?;
    let body = &resp[header_end + 4..];
    let mut content_length: Option<usize> = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().ok();
            }
        }
    }
    let body = match content_length {
        Some(n) if body.len() >= n => body[..n].to_vec(),
        Some(n) => return Err(ipp_err(&format!("тело короче Content-Length ({n})"))),
        None => body.to_vec(),
    };
    Ok((status, body))
}

/// Минимальная раскодировка `%XX` в имени принтера из URI.
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &s[i + 1..i + 3];
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// --- HTTP-транспорт -------------------------------------------------------

fn write_all<S: Write>(stream: &mut S, data: &[u8]) -> Result<()> {
    stream
        .write_all(data)
        .map_err(|e| Error::io("запрос к CUPS", e))
}

fn read_to_end<S: Read>(stream: &mut S) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .map_err(|e| Error::io("ответ CUPS", e))?;
    Ok(buf)
}

/// HTTP POST `application/ipp` → тело IPP-ответа (только статус200).
pub fn http_post_ipp(target: &Target, body: &[u8]) -> Result<Vec<u8>> {
    let host_label = match target {
        Target::Tcp { host, port } => format!("{host}:{port}"),
        Target::Unix(_) => "localhost".to_string(),
    };
    let head = format!(
        "POST / HTTP/1.1\r\nHost: {host_label}\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut request = Vec::with_capacity(head.len() + body.len());
    request.extend_from_slice(head.as_bytes());
    request.extend_from_slice(body);

    let response = match target {
        Target::Unix(path) => {
            let mut s = UnixStream::connect(path)
                .map_err(|e| Error::io(format!("сокет {}", path.display()), e))?;
            let _ = s.set_read_timeout(Some(TIMEOUT));
            let _ = s.set_write_timeout(Some(TIMEOUT));
            write_all(&mut s, &request)?;
            read_to_end(&mut s)?
        }
        Target::Tcp { host, port } => {
            let addr = (host.as_str(), *port)
                .to_socket_addrs()
                .map_err(|e| Error::io(format!("адрес {host}:{port}"), e))?
                .next()
                .ok_or_else(|| Error::InvalidInput(format!("адрес {host}:{port} не разрешён")))?;
            let mut s = TcpStream::connect_timeout(&addr, TIMEOUT)
                .map_err(|e| Error::io(format!("соединение {host}:{port}"), e))?;
            let _ = s.set_read_timeout(Some(TIMEOUT));
            let _ = s.set_write_timeout(Some(TIMEOUT));
            write_all(&mut s, &request)?;
            read_to_end(&mut s)?
        }
    };

    let (status, payload) = split_http_response(&response)?;
    if status != 200 {
        return Err(Error::ToolFailed {
            tool: "ipp".into(),
            code: status as i32,
            message: format!("HTTP {status} от CUPS"),
        });
    }
    Ok(payload)
}

// --- Чтение заданий -------------------------------------------------------

/// Запросить задания одного класса и разобрать их в доменные [`Job`].
pub fn query_jobs(user: Option<&str>, which: WhichJobs) -> Result<Vec<Job>> {
    let body = encode_get_jobs(user, which);
    let payload = http_post_ipp(&default_target(), &body)?;
    let msg = parse_ipp(&payload)?;
    if msg.status != 0x0000 {
        return Err(Error::ToolFailed {
            tool: "ipp".into(),
            code: msg.status as i32,
            message: format!("IPP status 0x{:04x}", msg.status),
        });
    }
    Ok(msg
        .groups
        .iter()
        .filter(|g| g.tag == 0x02)
        .filter_map(|g| attrs_to_job(&g.attrs))
        .collect())
}

/// Все задания: ожидающие + последние выполненные (до [`COMPLETED_LIMIT`]),
/// ожидающие первыми, внутри группы — по убыванию id.
pub fn jobs() -> Result<Vec<Job>> {
    let user = std::env::var("USER").ok();
    let user = user.as_deref();
    let pending = query_jobs(user, WhichJobs::NotCompleted)?;
    let mut completed = query_jobs(user, WhichJobs::Completed)?;
    completed.truncate(COMPLETED_LIMIT);
    let mut jobs = pending;
    jobs.extend(completed);
    jobs.sort_by_key(|j| (j.state.is_terminal(), std::cmp::Reverse(j.id)));
    Ok(jobs)
}

/// Группа атрибутов одного задания → доменный [`Job`].
fn attrs_to_job(attrs: &[IppAttr]) -> Option<Job> {
    let get = |name: &str| attrs.iter().find(|a| a.name == name);
    let id = get("job-id")?.first_int()?;
    let printer_uri = get("job-printer-uri")?.first_str()?;
    let printer = printer_uri.rsplit('/').next().filter(|s| !s.is_empty())?;
    let printer = percent_decode(printer);
    let state = get("job-state")
        .and_then(|a| a.first_int())
        .map(JobState::from_ipp)
        .unwrap_or(JobState::Unknown);
    let size = get("job-k-octets")
        .and_then(|a| a.first_int())
        .unwrap_or(0)
        .max(0)
        .saturating_mul(1024) as u64; // job-k-octets — килобайты (как у lpstat)
    let sheets_done = get("job-media-sheets-completed")
        .and_then(|a| a.first_int())
        .unwrap_or(0)
        .max(0) as u32;
    let sheets_total = get("job-media-sheets")
        .and_then(|a| a.first_int())
        .unwrap_or(0)
        .max(0) as u32;
    Some(Job {
        token: format!("{printer}-{id}"),
        id: id as u32,
        printer,
        owner: get("job-originating-user-name")
            .and_then(|a| a.first_str())
            .unwrap_or_default(),
        size,
        state,
        sheets_done,
        sheets_total,
        printer_uri,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Захваченный запрос libcups (`CUPS_SERVER=… lpstat -W completed -o`,
    /// тело HTTP после заголовков): эталон структуры Get-Jobs.
    const GOLDEN_GET_JOBS_COMPLETED: &str = "0200000a00000001\
01\
470012617474726962757465732d6368617273657400057574662d38\
48001b617474726962757465732d6e61747572616c2d6c616e6775616765000572752d7275\
45000b7072696e7465722d75726900106970703a2f2f6c6f63616c686f73742f\
4400147265717565737465642d6174747269627574657300066a6f622d6964\
440000000c6a6f622d6b2d6f6374657473\
44000000086a6f622d6e616d65\
44000000196a6f622d6f726967696e6174696e672d757365722d6e616d65\
44000000196a6f622d7072696e7465722d73746174652d6d657373616765\
440000000f6a6f622d7072696e7465722d757269\
44000000116a6f622d73746174652d726561736f6e73\
440000001074696d652d61742d6372656174696f6e\
440000001174696d652d61742d636f6d706c65746564\
42001472657175657374696e672d757365722d6e616d65000c6f6d65676170726f6a656374\
44000a77686963682d6a6f62730009636f6d706c65746564\
03";

    fn decode_hex(s: &str) -> Vec<u8> {
        let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        clean
            .as_bytes()
            .chunks(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    #[test]
    fn parses_golden_libcups_get_jobs_request() {
        // Эталон libcups должен разбираться нашим парсером: длины и
        // дополнительные значения (name-length=0) согласованы.
        let bytes = decode_hex(GOLDEN_GET_JOBS_COMPLETED);
        let msg = parse_ipp(&bytes).expect("эталон libcups должен разбираться");
        assert_eq!(msg.status, 0x000A, "Get-Jobs");
        assert_eq!(msg.groups.len(), 1);
        assert_eq!(msg.groups[0].tag, 0x01);
        let attrs = &msg.groups[0].attrs;
        let get = |n: &str| attrs.iter().find(|a| a.name == n).unwrap();
        assert_eq!(get("attributes-charset").first_str().unwrap(), "utf-8");
        assert_eq!(get("printer-uri").first_str().unwrap(), "ipp://localhost/");
        assert_eq!(get("which-jobs").first_str().unwrap(), "completed");
        assert_eq!(
            get("requesting-user-name").first_str().unwrap(),
            "omegaproject"
        );
        // requested-attrs:9 дополнительных значений (включая первое).
        let requested = get("requested-attributes");
        assert_eq!(requested.values.len(), 9, "{requested:?}");
        assert_eq!(requested.first_str().unwrap(), "job-id");
        assert_eq!(
            String::from_utf8_lossy(&requested.values[8]),
            "time-at-completed"
        );
    }

    #[test]
    fn encode_get_jobs_roundtrips() {
        for which in [WhichJobs::NotCompleted, WhichJobs::Completed] {
            let bytes = encode_get_jobs(Some("alice"), which);
            let msg = parse_ipp(&bytes).expect("собственный запрос должен разбираться");
            assert_eq!(msg.status, 0x000A);
            let attrs = &msg.groups[0].attrs;
            let get = |n: &str| attrs.iter().find(|a| a.name == n).unwrap();
            assert_eq!(get("printer-uri").first_str().unwrap(), "ipp://localhost/");
            assert_eq!(get("which-jobs").first_str().unwrap(), which.as_str());
            assert_eq!(get("requesting-user-name").first_str().unwrap(), "alice");
            // Никаких ручных длин: «job-state» ровно9 байт среди значений
            // requested-attributes (собственная ошибка длины здесь = сломанный запрос).
            let requested = attrs
                .iter()
                .find(|a| a.name == "requested-attributes")
                .unwrap();
            assert!(
                requested
                    .values
                    .iter()
                    .any(|v| v.as_slice() == b"job-state"),
                "{requested:?}"
            );
            assert!(
                requested
                    .values
                    .iter()
                    .any(|v| v.as_slice() == b"job-state-reasons")
            );
        }
        // Без пользователя — атрибут не добавляется.
        let msg = parse_ipp(&encode_get_jobs(None, WhichJobs::NotCompleted)).unwrap();
        assert!(
            !msg.groups[0]
                .attrs
                .iter()
                .any(|a| a.name == "requesting-user-name"),
            "без user не должно быть requesting-user-name"
        );
    }

    /// Ответ cupsd с двумя заданиями (фикстура на наших же кодировщиках).
    fn fixture_response() -> Vec<u8> {
        let mut b = vec![0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01];
        b.push(0x01);
        put_attr(&mut b, 0x47, "attributes-charset", b"utf-8");
        put_attr(&mut b, 0x48, "attributes-natural-language", b"en-us");
        // Задание 1: печатается.
        b.push(0x02);
        put_attr(&mut b, 0x21, "job-id", &3i32.to_be_bytes());
        put_attr(
            &mut b,
            0x45,
            "job-printer-uri",
            b"ipp://localhost/printers/G3030",
        );
        put_attr(&mut b, 0x41, "job-name", b"report.pdf");
        put_attr(&mut b, 0x44, "job-originating-user-name", b"alice");
        put_attr(&mut b, 0x21, "job-k-octets", &10i32.to_be_bytes());
        // job-state — enum (0x23), именно так его отдаёт cupsd.
        put_attr(&mut b, 0x23, "job-state", &5i32.to_be_bytes());
        put_attr(&mut b, 0x44, "job-state-reasons", b"printing-to-device");
        put_attr(
            &mut b,
            0x21,
            "job-media-sheets-completed",
            &3i32.to_be_bytes(),
        );
        put_attr(&mut b, 0x21, "job-media-sheets", &10i32.to_be_bytes());
        // Задание 2: выполнено, имя с пробелом в URI принтера.
        b.push(0x02);
        put_attr(&mut b, 0x21, "job-id", &7i32.to_be_bytes());
        put_attr(
            &mut b,
            0x45,
            "job-printer-uri",
            b"ipp://localhost/printers/My%20Printer",
        );
        put_attr(&mut b, 0x41, "job-name", b"photo.jpg");
        put_attr(&mut b, 0x44, "job-originating-user-name", b"bob");
        put_attr(&mut b, 0x21, "job-k-octets", &0i32.to_be_bytes());
        put_attr(&mut b, 0x23, "job-state", &9i32.to_be_bytes());
        b.push(0x03);
        b
    }

    #[test]
    fn parses_job_groups_into_domain_jobs() {
        let msg = parse_ipp(&fixture_response()).expect("фикстура должна разбираться");
        assert_eq!(msg.status, 0x0000);
        let jobs: Vec<Job> = msg
            .groups
            .iter()
            .filter(|g| g.tag == 0x02)
            .filter_map(|g| attrs_to_job(&g.attrs))
            .collect();
        assert_eq!(jobs.len(), 2);

        let j1 = &jobs[0];
        assert_eq!(j1.id, 3);
        assert_eq!(j1.token, "G3030-3");
        assert_eq!(j1.printer, "G3030");
        assert_eq!(j1.owner, "alice");
        assert_eq!(j1.size, 10 * 1024);
        assert_eq!(j1.state, JobState::Processing);
        assert!(!j1.state.is_terminal(), "печатается — не терминально");
        assert_eq!(j1.sheets_done, 3, "прогресс: отпечатано листов");
        assert_eq!(j1.sheets_total, 10);

        let j2 = &jobs[1];
        assert_eq!(j2.token, "My Printer-7", "%XX декодируется");
        assert_eq!(j2.state, JobState::Completed);
        assert!(j2.state.is_terminal());
        assert_eq!(
            (j2.sheets_done, j2.sheets_total),
            (0, 0),
            "без атрибутов — неизвестный прогресс, а не мусор"
        );
    }

    #[test]
    fn missing_attributes_yield_no_job() {
        // Нет job-id → задание пропускается, а не падает весь список.
        let mut b = vec![0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01];
        b.push(0x02);
        put_attr(&mut b, 0x41, "job-name", b"x");
        b.push(0x03);
        let msg = parse_ipp(&b).unwrap();
        assert!(attrs_to_job(&msg.groups[0].attrs).is_none());
    }

    #[test]
    fn truncated_message_is_error() {
        assert!(parse_ipp(&[0x02, 0x00]).is_err());
        let mut bytes = fixture_response();
        bytes.truncate(bytes.len() - 10);
        assert!(parse_ipp(&bytes).is_err(), "обрыв значений — ошибка");
        // Атрибут до группы — ошибка.
        let mut b = vec![0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01];
        put_attr(&mut b, 0x47, "attributes-charset", b"utf-8");
        assert!(parse_ipp(&b).is_err());
    }

    #[test]
    fn splits_http_response_by_content_length() {
        let ipp_body = fixture_response();
        let mut resp = b"HTTP/1.1 200 OK\r\ncontent-length: ".to_vec();
        resp.extend_from_slice(ipp_body.len().to_string().as_bytes());
        resp.extend_from_slice(b"\r\nServer: CUPS/2.4\r\n\r\n");
        resp.extend_from_slice(&ipp_body);
        // Хвост после Content-Length отбрасывается (keep-alive остаток).
        resp.extend_from_slice(b"HTTP/1.1 junk");
        let (status, body) = split_http_response(&resp).unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, ipp_body);

        // Без Content-Length — тело до конца.
        let resp2 = b"HTTP/1.0 404 Not Found\r\nX: y\r\n\r\nbody".to_vec();
        let (status2, body2) = split_http_response(&resp2).unwrap();
        assert_eq!(status2, 404);
        assert_eq!(body2, b"body");

        // Обрыв заголовков и короткое тело — ошибки.
        assert!(split_http_response(b"HTTP/1.1 200 OK\r\nno-end").is_err());
        let resp3 = b"HTTP/1.1 200 OK\r\nContent-Length: 99\r\n\r\nshort".to_vec();
        assert!(split_http_response(&resp3).is_err());
        assert!(split_http_response(b"no-http-header-here").is_err());
    }

    #[test]
    fn parses_server_names() {
        assert_eq!(
            parse_server_name("/run/cups/cups.sock"),
            Some(Target::Unix(PathBuf::from("/run/cups/cups.sock")))
        );
        assert_eq!(
            parse_server_name("localhost:1631"),
            Some(Target::Tcp {
                host: "localhost".into(),
                port: 1631
            })
        );
        assert_eq!(
            parse_server_name("ipp://printer.local"),
            Some(Target::Tcp {
                host: "printer.local".into(),
                port: 631
            })
        );
        assert_eq!(
            parse_server_name("cups"),
            Some(Target::Tcp {
                host: "cups".into(),
                port: 631
            })
        );
        assert_eq!(parse_server_name(""), None);
        assert_eq!(parse_server_name(":631"), None);
        assert_eq!(
            parse_server_name("host:abc"),
            None,
            "не-числовой порт — не порт"
        );
    }

    #[test]
    fn decodes_percent_sequences() {
        assert_eq!(percent_decode("My%20Printer"), "My Printer");
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(
            percent_decode("100%"),
            "100%",
            "неполная последовательность"
        );
        assert_eq!(percent_decode("a%2"), "a%2", "хвост — как есть");
        assert_eq!(percent_decode("%41%42"), "AB");
    }

    #[test]
    fn parses_numeric_value_tags() {
        // integer (0x21), boolean (0x22), enum (0x23) — 4 байта BE.
        for tag in [0x21u8, 0x22, 0x23] {
            let a = IppAttr {
                name: "x".into(),
                tag,
                values: vec![7i32.to_be_bytes().to_vec()],
            };
            assert_eq!(a.first_int(), Some(7), "тег 0x{tag:02x}");
        }
        // Не число / неправильная длина — None, а не мусор.
        let text = IppAttr {
            name: "x".into(),
            tag: 0x44,
            values: vec![7i32.to_be_bytes().to_vec()],
        };
        assert_eq!(text.first_int(), None, "keyword — не число");
        let short = IppAttr {
            name: "x".into(),
            tag: 0x21,
            values: vec![vec![1, 2]],
        };
        assert_eq!(short.first_int(), None, "короче4 байт — None");
    }

    #[test]
    fn maps_job_state_codes() {
        assert_eq!(JobState::from_ipp(3), JobState::Pending);
        assert_eq!(JobState::from_ipp(4), JobState::Held);
        assert_eq!(JobState::from_ipp(5), JobState::Processing);
        assert_eq!(JobState::from_ipp(6), JobState::Stopped);
        assert_eq!(JobState::from_ipp(7), JobState::Canceled);
        assert_eq!(JobState::from_ipp(8), JobState::Aborted);
        assert_eq!(JobState::from_ipp(9), JobState::Completed);
        assert_eq!(JobState::from_ipp(42), JobState::Unknown);
        assert!(JobState::from_ipp(9).is_terminal());
        assert!(JobState::from_ipp(7).is_terminal());
        assert!(!JobState::from_ipp(5).is_terminal());
    }

    /// Живой тест: требует запущенный локальный CUPS.
    #[test]
    #[ignore = "нужен запущенный локальный CUPS"]
    fn live_query_jobs_against_local_cups() {
        let jobs = jobs().expect("локальный CUPS должен ответить");
        eprintln!("IPP jobs: {jobs:?}");
        // Даже при пустой очереди путь должен сработать без ошибок.
        assert!(jobs.iter().all(|j| !j.token.is_empty()));

        // Диагностика: все атрибуты групп ответа (теги и значения) — при
        // расследовании проблем со статусами смотреть сюда.
        let user = std::env::var("USER").ok();
        let payload = http_post_ipp(
            &default_target(),
            &encode_get_jobs(user.as_deref(), WhichJobs::Completed),
        )
        .expect("ответ CUPS");
        let msg = parse_ipp(&payload).expect("разбор ответа");
        for group in &msg.groups {
            for a in &group.attrs {
                eprintln!(
                    "group=0x{:02x} {} tag=0x{:02x} values={:?}",
                    group.tag, a.name, a.tag, a.values
                );
            }
        }
    }

    #[test]
    fn job_op_codes_match_rfc8011() {
        assert_eq!(JobOp::Cancel.code(), 0x0008);
        assert_eq!(JobOp::Hold.code(), 0x000C);
        assert_eq!(JobOp::Release.code(), 0x000D);
        assert_eq!(JobOp::Restart.code(), 0x000E);
    }

    #[test]
    fn encode_job_op_roundtrip() {
        let b = encode_job_op(JobOp::Release, "ipp://localhost/printers/G3030", 7, None);
        // Заголовок: IPP 2.0, операция 0x000D (Release-Job), request-id 1.
        assert_eq!(&b[0..2], &[0x02, 0x00]);
        assert_eq!(u16::from_be_bytes([b[2], b[3]]), 0x000D);
        assert_eq!(u32::from_be_bytes([b[4], b[5], b[6], b[7]]), 1);
        assert_eq!(b[8], 0x01, "operation-attributes-tag");
        assert_eq!(*b.last().unwrap(), 0x03, "end-of-attributes");

        // Roundtrip собственным парсером: состав и теги атрибутов.
        let msg = parse_ipp(&b).expect("запрос должен разбираться");
        assert_eq!(msg.status, 0x000D);
        assert_eq!(msg.groups.len(), 1);
        assert_eq!(msg.groups[0].tag, 0x01);
        let attrs = &msg.groups[0].attrs;
        let find = |n: &str| {
            attrs
                .iter()
                .find(|a| a.name == n)
                .unwrap_or_else(|| panic!("нет атрибута {n}"))
        };
        assert_eq!(
            find("attributes-charset").first_str().as_deref(),
            Some("utf-8")
        );
        assert_eq!(
            find("printer-uri").first_str().as_deref(),
            Some("ipp://localhost/printers/G3030")
        );
        let job_id = find("job-id");
        assert_eq!(job_id.tag, 0x21, "job-id — integer");
        assert_eq!(job_id.first_int(), Some(7), "job-id = 7");
    }

    #[test]
    fn encode_job_op_with_user_and_cancel() {
        let b = encode_job_op(
            JobOp::Cancel,
            "ipp://localhost/printers/X",
            42,
            Some("vasya"),
        );
        assert_eq!(u16::from_be_bytes([b[2], b[3]]), 0x0008);
        let msg = parse_ipp(&b).unwrap();
        let attrs = &msg.groups[0].attrs;
        let user = attrs.iter().find(|a| a.name == "requesting-user-name");
        assert_eq!(user.and_then(|a| a.first_str()).as_deref(), Some("vasya"));
        let id = attrs.iter().find(|a| a.name == "job-id").unwrap();
        assert_eq!(id.first_int(), Some(42));
    }

    #[test]
    fn resolve_printer_uri_fallback_and_exact() {
        let exact = Job {
            token: "G3030-2".into(),
            id: 2,
            printer: "G3030".into(),
            owner: "u".into(),
            size: 0,
            state: JobState::Completed,
            sheets_done: 0,
            sheets_total: 0,
            printer_uri: "ipp://192.0.2.1:631/printers/G3030".into(),
        };
        assert_eq!(
            exact.resolve_printer_uri(),
            "ipp://192.0.2.1:631/printers/G3030"
        );

        let fallback = Job {
            printer_uri: String::new(),
            printer: "HP LaserJet".into(),
            ..exact
        };
        assert_eq!(
            fallback.resolve_printer_uri(),
            "ipp://localhost/printers/HP%20LaserJet"
        );
    }

    /// Живой тест: операция Release-Job над несуществующим заданием
    /// должна дать IPP-ошибку (а не HTTP 400/обрыв) — при этом ничего
    /// не печатается: идентификатор не существует.
    #[test]
    #[ignore = "нужен запущенный локальный CUPS"]
    fn live_job_op_missing_job_is_ipp_error() {
        let list = jobs().expect("локальный CUPS должен ответить");
        let Some(job) = list.first() else {
            eprintln!("очередь пуста — пропуск");
            return;
        };
        let uri = job.resolve_printer_uri();
        let err = job_op(JobOp::Release, &uri, 999_999_999)
            .expect_err("задания с таким id не существует");
        let text = err.to_string();
        eprintln!("job_op → {text}");
        assert!(
            text.contains("IPP status"),
            "ожидалась IPP-ошибка, получено: {text}"
        );
    }
}
