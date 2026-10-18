//! Запись многостраничного TIFF без сторонних TIFF-библиотек.
//!
//! `image`-crate умеет только одиночный TIFF, поэтому страницы пишутся
//! вручную: little-endian заголовок, цепочка IFD (по одному на страницу),
//! сжатие Deflate (tag 259 = 32946, Adobe Deflate, zlib-поток через flate2).
//!
//! Формат проверен чтением через image-crate (roundtrip-тесты) и внешними
//! декодерами (ImageMagick/libtiff `identify`, Pillow `n_frames`).

use std::io::Write;
use std::path::Path;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use image::DynamicImage;

use crate::error::{Error, Result};

// Типы значений TIFF.
const T_ASCII: u16 = 2;
const T_SHORT: u16 = 3;
const T_LONG: u16 = 4;
const T_RATIONAL: u16 = 5;

/// Adobe Deflate — читается libtiff/Pillow/ImageMagick/GNOME.
const COMPRESSION_ADOBE_DEFLATE: u16 = 32946;

/// Одна подготовленная страница (сырые 8-битные байты уже сжаты).
struct Page {
    width: u32,
    height: u32,
    gray: bool,
    /// Сжатые данные strip'а.
    data: Vec<u8>,
    /// Смещение strip'а в файле.
    offset: u32,
}

/// Писатель многостраничного TIFF (по одной странице на IFD).
pub struct TiffWriter {
    dpi: u32,
    pages: Vec<Page>,
}

impl TiffWriter {
    /// Создать писатель; `dpi` пишется в разрешение всех страниц.
    pub fn new(dpi: u32) -> Self {
        Self {
            dpi: dpi.max(1),
            pages: Vec::new(),
        }
    }

    /// Добавить страницу: RGB/Gray 8 бит (альфа отбрасывается).
    pub fn push_image(&mut self, img: &DynamicImage) {
        let gray = img.color().channel_count() == 1;
        let raw: Vec<u8> = if gray {
            img.to_luma8().into_raw()
        } else {
            img.to_rgb8().into_raw()
        };
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        // Пишем ошибку в Vec — не может случиться (Vec не IOError).
        let data = encoder
            .write_all(&raw)
            .and_then(|_| encoder.finish())
            .unwrap_or_default();
        self.pages.push(Page {
            width: img.width(),
            height: img.height(),
            gray,
            data,
            offset: 0,
        });
    }

    /// Количество добавленных страниц.
    pub fn len(&self) -> usize {
        self.pages.len()
    }

    /// Нет страниц.
    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    /// Собрать файл в байты.
    pub fn to_bytes(&mut self) -> Result<Vec<u8>> {
        if self.pages.is_empty() {
            return Err(Error::msg("TIFF: нет страниц"));
        }

        let mut buf: Vec<u8> = Vec::new();
        // Заголовок: II, 42, смещение IFD0 (заполним после).
        buf.extend_from_slice(&0x4949u16.to_le_bytes());
        buf.extend_from_slice(&42u16.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());

        // Данные страниц подряд сразу после заголовка.
        for page in &mut self.pages {
            page.offset = buf.len() as u32;
            buf.extend_from_slice(&page.data);
        }

        // Рациональные X/Y-разрешения (общие значения, свои на страницу).
        let res = self.dpi;
        // IFD-цепочка: каждый IFD сразу после extras предыдущего.
        let mut ifd_offsets: Vec<u32> = Vec::with_capacity(self.pages.len());
        for page in &self.pages {
            ifd_offsets.push(buf.len() as u32);
            write_page_ifd(&mut buf, page, res);
        }

        // Пропатчить next-указатели: адреса уже записаны по порядку,
        // но адреса известны только сейчас — перезапишем entries последним
        // полем. Проще: записывать next заранее нельзя, поэтому фиксируем
        // смещения заранее (выше) и пропатчим здесь.
        patch_next_pointers(&mut buf, &ifd_offsets);

        let ifd0 = ifd_offsets[0];
        buf[4..8].copy_from_slice(&ifd0.to_le_bytes());
        Ok(buf)
    }

    /// Записать файл на диск.
    pub fn save(&mut self, path: &Path) -> Result<()> {
        let bytes = self.to_bytes()?;
        std::fs::write(path, bytes).map_err(|e| Error::io(format!("запись {}", path.display()), e))
    }
}

/// Записать один IFD: count, 13 entries (отсортированы по tag), next (0 —
/// запатчится), затем extras-блок (bits, разрешения, software).
///
/// next-указатель временно 0; реальное значение следующего IFD
/// проставляется `patch_next_pointers` (адреса всех IFD известны после записи).
fn write_page_ifd(buf: &mut Vec<u8>, page: &Page, dpi: u32) {
    let spp: u32 = if page.gray { 1 } else { 3 };

    // Extras-блок (пишется после entries+next; смещения известны синхронно).
    // Разметку сначала посчитаем: entries занимают count(2)+12*13+next(4).
    let entries_count: u16 = 13;
    let ifd_header_len = 2 + 12 * entries_count as usize + 4;
    let extras_start = buf.len() + ifd_header_len;

    // Блок extras: bits per sample (6 байт для RGB, 1 байт inline для gray),
    // XRes (8), YRes (8), software (ASCII с нулём).
    let mut extras: Vec<u8> = Vec::new();
    let bits_offset = extras_start as u32;
    if !page.gray {
        for _ in 0..3 {
            extras.extend_from_slice(&8u16.to_le_bytes());
        }
    }
    let xres_offset = (extras_start + extras.len()) as u32;
    extras.extend_from_slice(&dpi.to_le_bytes());
    extras.extend_from_slice(&1u32.to_le_bytes());
    let yres_offset = (extras_start + extras.len()) as u32;
    extras.extend_from_slice(&dpi.to_le_bytes());
    extras.extend_from_slice(&1u32.to_le_bytes());
    let sw_offset = (extras_start + extras.len()) as u32;
    const SOFTWARE: &[u8] = b"LinuxPrinterG\x00";
    extras.extend_from_slice(SOFTWARE);

    let mut entries: Vec<(u16, u16, u32, u32)> = vec![
        (256, T_LONG, 1, page.width),
        (257, T_LONG, 1, page.height),
        (258, T_SHORT, spp, if page.gray { 8 } else { bits_offset }),
        (259, T_SHORT, 1, COMPRESSION_ADOBE_DEFLATE as u32),
        (262, T_SHORT, 1, if page.gray { 1 } else { 2 }),
        (273, T_LONG, 1, page.offset),
        (277, T_SHORT, 1, spp),
        (278, T_LONG, 1, page.height),
        (279, T_LONG, 1, page.data.len() as u32),
        (282, T_RATIONAL, 1, xres_offset),
        (283, T_RATIONAL, 1, yres_offset),
        (296, T_SHORT, 1, 2),
        (305, T_ASCII, SOFTWARE.len() as u32, sw_offset),
    ];
    entries.sort_by_key(|&(tag, ..)| tag);

    buf.extend_from_slice(&entries_count.to_le_bytes());
    for (tag, typ, count, value) in &entries {
        buf.extend_from_slice(&tag.to_le_bytes());
        buf.extend_from_slice(&typ.to_le_bytes());
        buf.extend_from_slice(&count.to_le_bytes());
        // Inline-значения для SHORT count=1: 2 байта в начале поля, остальное 0.
        if *typ == T_SHORT && *count == 1 {
            buf.extend_from_slice(&(*value as u16).to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
        } else {
            buf.extend_from_slice(&value.to_le_bytes());
        }
    }
    buf.extend_from_slice(&0u32.to_le_bytes()); // next — патчится
    buf.extend_from_slice(&extras);
}

/// Проставить next-указатели всех IFD (адреса записаны при построении).
fn patch_next_pointers(buf: &mut [u8], ifd_offsets: &[u32]) {
    let entries_count: u16 = 13;
    let next_pos_from_ifd = 2 + 12 * entries_count as usize;
    for (i, &offset) in ifd_offsets.iter().enumerate() {
        let next = if i + 1 < ifd_offsets.len() {
            ifd_offsets[i + 1]
        } else {
            0
        };
        let pos = offset as usize + next_pos_from_ifd;
        buf[pos..pos + 4].copy_from_slice(&next.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Прочитать смещение IFD0 из заголовка.
    fn ifd0_offset(bytes: &[u8]) -> usize {
        u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize
    }

    /// Прочитать (count, next) одного IFD.
    fn ifd_head(bytes: &[u8], off: usize) -> (u16, u32) {
        let count = u16::from_le_bytes(bytes[off..off + 2].try_into().unwrap());
        let next_pos = off + 2 + 12 * count as usize;
        let next = u32::from_le_bytes(bytes[next_pos..next_pos + 4].try_into().unwrap());
        (count, next)
    }

    #[test]
    fn header_and_ifd_chain() {
        let mut w = TiffWriter::new(96);
        for _ in 0..3 {
            w.push_image(&DynamicImage::new_rgb8(4, 6));
        }
        let bytes = w.to_bytes().unwrap();
        assert_eq!(&bytes[0..2], b"II");
        assert_eq!(u16::from_le_bytes(bytes[2..4].try_into().unwrap()), 42);

        let mut off = ifd0_offset(&bytes);
        for page in 1..=3u16 {
            let (count, next) = ifd_head(&bytes, off);
            assert_eq!(count, 13, "IFD должен содержать 13 тегов");
            if page < 3 {
                assert_ne!(next, 0, "next не должен быть 0 у страницы {page}");
                off = next as usize;
            } else {
                assert_eq!(next, 0, "последний IFD заканчивает цепочку");
            }
        }
    }

    #[test]
    fn roundtrip_rgb_pixels() {
        let mut img = DynamicImage::new_rgb8(5, 4);
        {
            let px = img.as_mut_rgb8().unwrap();
            for (i, p) in px.pixels_mut().enumerate() {
                let v = (i % 251) as u8;
                *p = image::Rgb([v, v.wrapping_add(7), v.wrapping_add(13)]);
            }
        }
        let mut w = TiffWriter::new(300);
        w.push_image(&img);
        let bytes = w.to_bytes().unwrap();

        let decoded = image::load_from_memory(&bytes).expect("декод TIFF");
        assert_eq!(decoded.width(), 5);
        assert_eq!(decoded.height(), 4);
        let got = decoded.to_rgb8();
        let want = img.to_rgb8();
        assert_eq!(
            got.as_raw(),
            want.as_raw(),
            "пиксели должны совпадать (lossless)"
        );
    }

    #[test]
    fn roundtrip_gray() {
        let img = DynamicImage::new_luma8(7, 3);
        let mut w = TiffWriter::new(96);
        w.push_image(&img);
        let bytes = w.to_bytes().unwrap();
        let decoded = image::load_from_memory(&bytes).expect("декод gray TIFF");
        assert_eq!(decoded.color().channel_count(), 1);
        assert_eq!(decoded.width(), 7);
        assert_eq!(decoded.height(), 3);
    }

    #[test]
    fn first_page_wins_for_multipage_decode() {
        let a = DynamicImage::ImageRgb8(image::ImageBuffer::from_pixel(
            8,
            8,
            image::Rgb([30u8, 30, 30]),
        ));
        let b = DynamicImage::ImageRgb8(image::ImageBuffer::from_pixel(
            8,
            8,
            image::Rgb([200u8, 200, 200]),
        ));
        let mut w = TiffWriter::new(96);
        w.push_image(&a);
        w.push_image(&b);
        let bytes = w.to_bytes().unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        let px = decoded.to_rgb8().get_pixel(0, 0).0;
        assert_eq!(px[0], 30, "декодер берёт первую страницу");
    }

    #[test]
    fn empty_rejected() {
        let mut w = TiffWriter::new(96);
        assert!(w.is_empty());
        assert!(w.to_bytes().is_err());
    }

    #[test]
    fn saves_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.tiff");
        let mut w = TiffWriter::new(96);
        w.push_image(&DynamicImage::new_rgb8(3, 3));
        w.save(&path).unwrap();
        assert!(path.is_file());
        let meta = image::image_dimensions(&path).unwrap();
        assert_eq!(meta, (3, 3));
    }
}
