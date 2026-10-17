//! Запись многопостраничного PDF без сторонних PDF-библиотек.
//!
//! Реализован минимальный, но корректный PDF 1.5: каталог, дерево страниц,
//! содержимое страницы (матрица преобразования) и XObject-изображения.
//! * Растровые страницы сжимаются zlib (`FlateDecode`);
//! * JPEG-страницы встраиваются без перекодирования (`DCTDecode`) — быстрее
//!   и без потерь.
//!
//! Формат проверен утилитами poppler (`pdftoppm`) в интеграционных тестах.

use std::io::Write;
use std::path::Path;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use image::DynamicImage;

use crate::error::{Error, Result};

/// Входная страница PDF.
#[derive(Debug, Clone)]
pub enum PageInput {
    /// 8-битныеamples: RGB (len = w*h*3) или Gray (len = w*h).
    Raster {
        width: u32,
        height: u32,
        gray: bool,
        samples: Vec<u8>,
    },
    /// Сжатый JPEG: встраиваем как есть.
    Jpeg {
        width: u32,
        height: u32,
        gray: bool,
        data: Vec<u8>,
    },
}

impl PageInput {
    /// Преобразовать изображение в растровую страницу (8 бит на канал).
    pub fn from_image(img: &DynamicImage) -> Self {
        let gray = img.color().channel_count() == 1;
        let (width, height) = (img.width(), img.height());
        if gray {
            PageInput::Raster {
                width,
                height,
                gray: true,
                samples: img.to_luma8().into_raw(),
            }
        } else {
            PageInput::Raster {
                width,
                height,
                gray: false,
                samples: img.to_rgb8().into_raw(),
            }
        }
    }

    /// Разобрать JPEG-байты: размеры и каналы из маркеров SOF.
    pub fn from_jpeg_bytes(data: Vec<u8>) -> Result<Self> {
        let (width, height, gray) =
            jpeg_dimensions(&data).ok_or_else(|| Error::msg("не удалось разобрать JPEG"))?;
        Ok(PageInput::Jpeg {
            width,
            height,
            gray,
            data,
        })
    }

    /// Если файл начинается с JPEG-маркера SOI — использовать без перекодирования.
    pub fn from_bytes_auto(data: Vec<u8>) -> Result<Self> {
        if data.len() > 3 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF {
            if let Ok(page) = PageInput::from_jpeg_bytes(data.clone()) {
                return Ok(page);
            }
        }
        let img = image::load_from_memory(&data)
            .map_err(|e| Error::msg(format!("не удалось декодировать изображение: {e}")))?;
        Ok(PageInput::from_image(&img))
    }

    fn dimensions(&self) -> (u32, u32, bool) {
        match self {
            PageInput::Raster {
                width,
                height,
                gray,
                ..
            }
            | PageInput::Jpeg {
                width,
                height,
                gray,
                ..
            } => (*width, *height, *gray),
        }
    }
}

/// Конструктор PDF из страниц.
pub struct PdfWriter {
    dpi: u32,
    pages: Vec<PageInput>,
}

impl PdfWriter {
    /// `dpi` влияет только на размер страницы в пунктах (px * 72 / dpi).
    pub fn new(dpi: u32) -> Self {
        PdfWriter {
            dpi: dpi.clamp(36, 2400).max(1),
            pages: Vec::new(),
        }
    }

    pub fn with_image(mut self, img: &DynamicImage) -> Self {
        self.push_image(img);
        self
    }

    pub fn push_image(&mut self, img: &DynamicImage) {
        self.pages.push(PageInput::from_image(img));
    }

    pub fn push(&mut self, page: PageInput) {
        self.pages.push(page);
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(1 << 20);
        self.write(&mut buf)?;
        Ok(buf)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if self.pages.is_empty() {
            return Err(Error::msg("нечего сохранять: нет страниц"));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::io(format!("создание {}", parent.display()), e))?;
        }
        let file = std::fs::File::create(path)
            .map_err(|e| Error::io(format!("создание {}", path.display()), e))?;
        let mut w = std::io::BufWriter::new(file);
        self.write(&mut w)
            .map_err(|e| Error::io(format!("запись {}", path.display()), e))?;
        w.flush()
            .map_err(|e| Error::io(format!("запись {}", path.display()), e))?;
        Ok(())
    }

    pub fn write<W: Write>(&self, out: W) -> Result<()> {
        if self.pages.is_empty() {
            return Err(Error::msg("нечего сохранять: нет страниц"));
        }
        let mut out = Out { inner: out, pos: 0 };

        // Заголовок с бинарным комментарием — сигнал для парсеров о 8-bit данных.
        out.raw(b"%PDF-1.5\n%\xE2\xE3\xCF\xD3\n")?;

        let n = self.pages.len() as u32;
        let total = 2 + 3 * n; // max id
        let mut offsets: Vec<u64> = vec![0; total as usize + 1];

        offsets[1] = out.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")?;

        let mut kids = String::new();
        for i in 0..n {
            if i > 0 {
                kids.push(' ');
            }
            kids.push_str(&format!("{} 0 R", 3 + 3 * i));
        }
        offsets[2] = out.obj(
            2,
            format!("<< /Type /Pages /Kids [{kids}] /Count {n} >>").as_bytes(),
        )?;

        for (i, page) in self.pages.iter().enumerate() {
            let i = i as u32;
            let page_id = 3 + 3 * i;
            let content_id = page_id + 1;
            let image_id = page_id + 2;
            let (w_px, h_px, gray) = page.dimensions();
            if w_px == 0 || h_px == 0 {
                return Err(Error::msg("страница нулевого размера"));
            }
            let pw = w_px as f64 * 72.0 / self.dpi as f64;
            let ph = h_px as f64 * 72.0 / self.dpi as f64;

            // Данные изображения: zlib для растра, как есть для JPEG.
            let (image_dict, image_data) = match page {
                PageInput::Raster { samples, .. } => {
                    let colorspace = if gray { "/DeviceGray" } else { "/DeviceRGB" };
                    let compressed = deflate(samples)?;
                    let dict = format!(
                        "<< /Type /XObject /Subtype /Image /Width {w_px} /Height {h_px} \
                         /ColorSpace {colorspace} /BitsPerComponent 8 \
                         /Filter /FlateDecode /Length {} >>",
                        compressed.len()
                    );
                    (dict, compressed)
                }
                PageInput::Jpeg { data, .. } => {
                    let colorspace = if gray { "/DeviceGray" } else { "/DeviceRGB" };
                    let dict = format!(
                        "<< /Type /XObject /Subtype /Image /Width {w_px} /Height {h_px} \
                         /ColorSpace {colorspace} /BitsPerComponent 8 \
                         /Filter /DCTDecode /Length {} >>",
                        data.len()
                    );
                    (dict, data.clone())
                }
            };

            // Содержимое страницы: изображение растянуто на весь лист.
            let content = content_stream(pw, ph);
            let content_dict = format!("<< /Length {} >>", content.len());

            offsets[page_id as usize] = out.obj(
                page_id,
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.2} {ph:.2}] \
                     /Resources << /XObject << /Im{} {image_id} 0 R >> >> \
                     /Contents {content_id} 0 R >>",
                    i + 1
                )
                .as_bytes(),
            )?;
            offsets[content_id as usize] =
                out.stream(content_id, &content_dict, content.as_bytes())?;
            offsets[image_id as usize] = out.stream(image_id, &image_dict, &image_data)?;
        }

        // Таблица xref + trailer.
        let xref_pos = out.pos;
        let mut xref = format!("xref\n0 {}\n0000000000 65535 f \n", total + 1);
        for id in 1..=total {
            xref.push_str(&format!("{:010} 00000 n \n", offsets[id as usize]));
        }
        out.raw(xref.as_bytes())?;
        out.raw(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
                total + 1,
                xref_pos
            )
            .as_bytes(),
        )?;
        Ok(())
    }
}

fn content_stream(pw: f64, ph: f64) -> String {
    // Изображение страницы (/Im1) растянуто на весь MediaBox.
    format!("q {pw:.2} 0 0 {ph:.2} 0 0 cm /Im1 Do Q")
}

fn deflate(data: &[u8]) -> Result<Vec<u8>> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::fast());
    enc.write_all(data)
        .map_err(|e| Error::io("сжатие zlib", e))?;
    enc.finish().map_err(|e| Error::io("сжатие zlib", e))
}

struct Out<W: Write> {
    inner: W,
    pos: u64,
}

impl<W: Write> Out<W> {
    fn raw(&mut self, bytes: &[u8]) -> Result<()> {
        self.inner
            .write_all(bytes)
            .map_err(|e| Error::io("запись PDF", e))?;
        self.pos += bytes.len() as u64;
        Ok(())
    }

    fn obj(&mut self, id: u32, body: &[u8]) -> Result<u64> {
        let off = self.pos;
        self.raw(format!("{id} 0 obj\n").as_bytes())?;
        self.raw(body)?;
        self.raw(b"\nendobj\n")?;
        Ok(off)
    }

    fn stream(&mut self, id: u32, dict: &str, data: &[u8]) -> Result<u64> {
        let off = self.pos;
        self.raw(format!("{id} 0 obj\n{dict}\nstream\n").as_bytes())?;
        self.raw(data)?;
        self.raw(b"\nendstream\nendobj\n")?;
        Ok(off)
    }
}

/// Размеры и признак оттенков серого у JPEG по маркерам SOFn.
pub fn jpeg_dimensions(data: &[u8]) -> Option<(u32, u32, bool)> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return None;
    }
    let mut i = 2usize;
    while i + 4 <= data.len() {
        if data[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = data[i + 1];
        match marker {
            0xD8 | 0x01 | 0xD0..=0xD7 => {
                i += 2;
            }
            0xDA => break, // начало сканирования
            _ => {
                if i + 4 > data.len() {
                    return None;
                }
                let seg_len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
                let is_sof = matches!(
                    marker,
                    0xC0 | 0xC1
                        | 0xC2
                        | 0xC3
                        | 0xC5
                        | 0xC6
                        | 0xC7
                        | 0xC9
                        | 0xCA
                        | 0xCB
                        | 0xCD
                        | 0xCE
                        | 0xCF
                );
                if is_sof && i + 9 <= data.len() {
                    let precision = data[i + 4];
                    let height = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
                    let width = u16::from_be_bytes([data[i + 7], data[i + 8]]) as u32;
                    let components = data.get(i + 9).copied().unwrap_or(3);
                    if precision == 8 && width > 0 && height > 0 {
                        return Some((width, height, components == 1));
                    }
                }
                if seg_len < 2 {
                    return None;
                }
                i += 2 + seg_len;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_dimensions_parsed() {
        // SOI, SOF0 (640x480, 3 компоненты), EOI.
        let mut jpeg = vec![0xFF, 0xD8];
        jpeg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        jpeg.extend_from_slice(&[0x01, 0xE0]); // height 480
        jpeg.extend_from_slice(&[0x02, 0x80]); // width 640
        jpeg.extend_from_slice(&[0x03]);
        jpeg.extend_from_slice(&[0x01, 0x11, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00]);
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        let (w, h, gray) = jpeg_dimensions(&jpeg).unwrap();
        assert_eq!((w, h, gray), (640, 480, false));
    }

    #[test]
    fn builds_valid_pdf_structure() {
        let img = DynamicImage::new_rgb8(4, 6);
        let bytes = PdfWriter::new(96).with_image(&img).to_bytes().unwrap();
        assert!(bytes.starts_with(b"%PDF-1.5"));
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Type /Catalog"));
        assert!(text.contains("/Count 1"));
        assert!(text.contains("/Filter /FlateDecode"));
        assert!(text.contains("%%EOF"));

        // Проверяем корректность xref: смещение объекта 1 ведёт на "1 0 obj".
        let startxref = text.rfind("startxref").expect("startxref");
        let num_start = startxref + "startxref".len();
        let xref_off: usize = text[num_start..]
            .split_whitespace()
            .next()
            .expect("число после startxref")
            .parse()
            .unwrap();
        assert_eq!(&bytes[xref_off..xref_off + 4], b"xref");
        // Первая запись xref после "xref\n0 N\n" — свободная, вторая — объект 1.
        let after_header = &bytes[xref_off..];
        let after_text = String::from_utf8_lossy(after_header).into_owned();
        let lines: Vec<&str> = after_text.lines().collect();
        let obj1_line = lines[3];
        let obj1_off: usize = obj1_line[..10].parse().unwrap();
        assert_eq!(
            &bytes[obj1_off..obj1_off + 8],
            b"1 0 obj\n",
            "некорректный xref"
        );
    }

    #[test]
    fn multipage_pdf_has_all_pages() {
        let mut w = PdfWriter::new(150);
        for _ in 0..3 {
            w.push_image(&DynamicImage::new_rgb8(8, 8));
        }
        let bytes = w.to_bytes().unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Count 3"));
        assert!(text.matches("/Type /Page ").count() == 3);
    }

    #[test]
    fn jpeg_page_embedded_raw() {
        let mut jpeg = vec![
            0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x10, 0x00, 0x20, 0x03,
        ];
        jpeg.extend_from_slice(&[0x01, 0x11, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00]);
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        let mut w = PdfWriter::new(96);
        w.push(PageInput::from_jpeg_bytes(jpeg).unwrap());
        let bytes = w.to_bytes().unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Filter /DCTDecode"));
        assert!(text.contains("/Width 32"));
    }

    #[test]
    fn empty_writer_rejected() {
        assert!(PdfWriter::new(96).to_bytes().is_err());
    }

    #[test]
    fn raster_from_image_channels() {
        let rgb = DynamicImage::new_rgb8(2, 2);
        match PageInput::from_image(&rgb) {
            PageInput::Raster { gray, samples, .. } => {
                assert!(!gray);
                assert_eq!(samples.len(), 12);
            }
            _ => panic!(),
        }
        let gray = DynamicImage::new_luma8(2, 2);
        match PageInput::from_image(&gray) {
            PageInput::Raster { gray, samples, .. } => {
                assert!(gray);
                assert_eq!(samples.len(), 4);
            }
            _ => panic!(),
        }
    }
}
