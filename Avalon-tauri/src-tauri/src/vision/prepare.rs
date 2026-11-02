// 图片预处理
//
// 纯本地、无网络：解码 → EXIF 方向 → 缩放 → 重编码（顺带剥离元数据）→ 缩略图。
// 目标不是「识别」，而是把送进 VLM 的图变小、方向正确、没有隐私元数据。

use std::io::Cursor;

use anyhow::{bail, Context, Result};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, ImageReader};
use sha2::{Digest, Sha256};

use crate::config::VisionConfig;

use super::types::{
    ImageProfile, PreparedImage, Thumbnail, MAX_IMAGE_SIDE, MAX_PIXELS,
};

/// 缩略图长边
pub(crate) const THUMB_EDGE: u32 = 320;
/// 缩略图 JPEG 质量
const THUMB_QUALITY: u8 = 80;

/// 解码并整形一张图片。
///
/// - 只接受常见位图格式；SVG/HEIC 等交给调用方明确报错；
/// - 解码前先读头部尺寸，超过像素/单边上限直接拒绝，避免解压炸弹；
/// - 重新编码不会写回 EXIF，GPS/相机信息在这一步自然丢失。
pub fn prepare(bytes: &[u8], cfg: &VisionConfig) -> Result<PreparedImage> {
    if bytes.is_empty() {
        bail!("图片内容为空");
    }

    let format = image::guess_format(bytes).context("无法识别图片格式")?;
    if !is_supported(format) {
        bail!("不支持的图片格式: {:?}", format);
    }

    // 只读头部，先做尺寸校验；真正解码前就把明显超限的图挡掉
    let reader = ImageReader::with_format(Cursor::new(bytes), format);
    let (width, height) = reader.into_dimensions().context("读取图片尺寸失败")?;
    check_dimensions(width, height)?;

    // 解码时再套一层限制，防止头部尺寸与真实数据不一致
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    // Limits 是 #[non_exhaustive]，只能先取默认值再改字段
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(MAX_PIXELS.saturating_mul(4));
    reader.limits(limits);
    let decoded = reader.decode().context("解码图片失败")?;

    let orientation = read_orientation(bytes);
    let oriented = apply_orientation(decoded, orientation);
    let profile = ImageProfile {
        width,
        height,
        source_bytes: bytes.len() as u64,
    };

    let (encoded, mime) = encode_within_budget(&oriented, cfg)?;
    let thumbnail = make_thumbnail(&oriented)?;

    Ok(PreparedImage {
        bytes: encoded,
        mime,
        sha256: sha256_hex(bytes),
        phash: dhash(&oriented),
        profile,
        thumbnail,
    })
}

/// 64 位差值哈希：灰度 → 9×8 → 比较每行相邻像素。
///
/// 在方向校正后的原图上计算，因此 max_edge 改变不会影响哈希；JPEG 重编码
/// 带来的像素抖动也很少改变比较结果，适合识别「同一张图另存/转发」。
pub(crate) fn dhash(image: &DynamicImage) -> String {
    let gray = image.to_luma8();
    let small = image::imageops::resize(&gray, 9, 8, FilterType::Triangle);

    let mut bits = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            let left = small.get_pixel(x, y)[0];
            let right = small.get_pixel(x + 1, y)[0];
            bits = (bits << 1) | u64::from(left > right);
        }
    }
    format!("{bits:016x}")
}

fn check_dimensions(width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 {
        bail!("图片尺寸为 0");
    }
    if width > MAX_IMAGE_SIDE || height > MAX_IMAGE_SIDE {
        bail!(
            "图片单边超过上限 {MAX_IMAGE_SIDE}: {width}x{height}"
        );
    }
    if width as u64 * height as u64 > MAX_PIXELS {
        bail!("图片像素超过上限 {MAX_PIXELS}: {width}x{height}");
    }
    Ok(())
}

fn is_supported(format: ImageFormat) -> bool {
    matches!(
        format,
        ImageFormat::Png
            | ImageFormat::Jpeg
            | ImageFormat::WebP
            | ImageFormat::Gif
            | ImageFormat::Bmp
            | ImageFormat::Tiff
    )
}

/// 按预算编码：先压质量，再缩边；最多 6 轮，避免极端图片卡住
fn encode_within_budget(
    image: &DynamicImage,
    cfg: &VisionConfig,
) -> Result<(Vec<u8>, &'static str)> {
    let has_alpha = image.color().has_alpha();
    let max_edge = cfg.max_edge;
    let mut edge = max_edge;
    let mut quality = cfg.jpeg_quality;

    for attempt in 0..6 {
        let resized = if image.width().max(image.height()) > edge {
            image.resize(edge, edge, FilterType::Lanczos3)
        } else {
            image.clone()
        };
        let (bytes, mime) = encode(&resized, has_alpha, quality)?;
        if bytes.len() as u64 <= cfg.max_bytes || attempt == 5 {
            return Ok((bytes, mime));
        }

        if quality > 55 {
            quality = quality.saturating_sub(15);
        } else {
            edge = ((edge as f64) * 0.75).round().max(256.0) as u32;
        }
    }

    unreachable!("loop 最后一轮必定返回")
}

fn encode(image: &DynamicImage, has_alpha: bool, quality: u8) -> Result<(Vec<u8>, &'static str)> {
    let mut buf = Vec::new();
    if has_alpha {
        image
            .write_with_encoder(PngEncoder::new(&mut buf))
            .context("编码 PNG 失败")?;
        Ok((buf, "image/png"))
    } else {
        let rgb = DynamicImage::ImageRgb8(image.to_rgb8());
        rgb.write_with_encoder(JpegEncoder::new_with_quality(&mut buf, quality))
            .context("编码 JPEG 失败")?;
        Ok((buf, "image/jpeg"))
    }
}

fn make_thumbnail(image: &DynamicImage) -> Result<Thumbnail> {
    let thumb = image.thumbnail(THUMB_EDGE, THUMB_EDGE);
    if thumb.color().has_alpha() {
        let mut buf = Vec::new();
        thumb
            .write_with_encoder(PngEncoder::new(&mut buf))
            .context("编码缩略图 PNG 失败")?;
        Ok(Thumbnail {
            bytes: buf,
            ext: "png",
        })
    } else {
        let mut buf = Vec::new();
        let rgb = DynamicImage::ImageRgb8(thumb.to_rgb8());
        rgb.write_with_encoder(JpegEncoder::new_with_quality(&mut buf, THUMB_QUALITY))
            .context("编码缩略图 JPEG 失败")?;
        Ok(Thumbnail {
            bytes: buf,
            ext: "jpg",
        })
    }
}

/// 读取 EXIF Orientation（1–8）。没有 EXIF 或读取失败都按正常方向处理
fn read_orientation(bytes: &[u8]) -> u8 {
    let mut cursor = Cursor::new(bytes);
    exif::Reader::new()
        .read_from_container(&mut cursor)
        .ok()
        .and_then(|exif| {
            exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                .and_then(|field| field.value.get_uint(0))
        })
        .unwrap_or(1) as u8
}

fn apply_orientation(image: DynamicImage, orientation: u8) -> DynamicImage {
    match orientation {
        2 => image.fliph(),
        3 => image.rotate180(),
        4 => image.flipv(),
        5 => image.rotate90().fliph(),
        6 => image.rotate90(),
        7 => image.rotate270().fliph(),
        8 => image.rotate270(),
        _ => image,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::VisionConfig;

    fn png(width: u32, height: u32, alpha: bool) -> Vec<u8> {
        let mut buf = Vec::new();
        if alpha {
            let img = image::RgbaImage::from_pixel(width, height, image::Rgba([200, 30, 40, 128]));
            DynamicImage::ImageRgba8(img)
                .write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
                .unwrap();
        } else {
            let img = image::RgbImage::from_pixel(width, height, image::Rgb([200, 30, 40]));
            DynamicImage::ImageRgb8(img)
                .write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
                .unwrap();
        }
        buf
    }

    fn cfg() -> VisionConfig {
        VisionConfig {
            enabled: true,
            max_edge: 64,
            max_bytes: 1024 * 1024,
            ..VisionConfig::default()
        }
    }

    #[test]
    fn prepare_resizes_and_strips_to_jpeg() {
        let bytes = png(512, 256, false);
        let out = prepare(&bytes, &cfg()).unwrap();
        assert_eq!(out.mime, "image/jpeg");
        let decoded = image::load_from_memory(&out.bytes).unwrap();
        assert_eq!(decoded.width(), 64);
        assert_eq!(decoded.height(), 32);
        assert_eq!(out.profile.width, 512);
        assert!(!out.thumbnail.bytes.is_empty());
    }

    #[test]
    fn prepare_keeps_alpha_as_png() {
        let bytes = png(128, 128, true);
        let out = prepare(&bytes, &cfg()).unwrap();
        assert_eq!(out.mime, "image/png");
        let decoded = image::load_from_memory(&out.bytes).unwrap();
        assert!(decoded.color().has_alpha());
    }

    #[test]
    fn prepare_rejects_garbage() {
        assert!(prepare(b"not an image", &cfg()).is_err());
    }

    #[test]
    fn prepare_rejects_oversized_dimensions() {
        // 头部声明 20000x1：超过单边上限，应在解码前被拒绝
        let bytes = png(20_000, 1, false);
        assert!(prepare(&bytes, &cfg()).is_err());
    }

    /// 生成有明确横向梯度与棋盘纹理的图，避免纯色图 dHash 退化
    fn pattern(invert: bool) -> DynamicImage {
        let img = image::RgbImage::from_fn(128, 128, |x, y| {
            let checker: u8 = if (x / 16 + y / 16) % 2 == 0 { 40 } else { 200 };
            let gradient: u8 = (x * 120 / 127) as u8;
            let base = if invert { 255 - checker } else { checker };
            let v = base.saturating_add(gradient / 3);
            image::Rgb([v, v / 2, 255 - v])
        });
        DynamicImage::ImageRgb8(img)
    }

    fn encode(image: &DynamicImage, format: ImageFormat) -> Vec<u8> {
        let mut buf = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut buf), format)
            .unwrap();
        buf
    }

    #[test]
    fn dhash_is_stable_across_reencoding() {
        let source = pattern(false);
        let png = encode(&source, ImageFormat::Png);
        let jpeg = encode(&source, ImageFormat::Jpeg);

        let a = prepare(&png, &cfg()).unwrap();
        let b = prepare(&jpeg, &cfg()).unwrap();
        assert_eq!(a.phash, b.phash, "JPEG 重编码不应改变 dHash");
        assert_eq!(a.phash.len(), 16);
    }

    #[test]
    fn dhash_separates_different_images() {
        let a = prepare(&encode(&pattern(false), ImageFormat::Png), &cfg()).unwrap();
        let b = prepare(&encode(&pattern(true), ImageFormat::Png), &cfg()).unwrap();
        let distance = crate::vision::cache::phash_distance(&a.phash, &b.phash).unwrap();
        assert!(distance > 4, "不同图案的 Hamming 距离应明显大于去重阈值");
    }

    #[test]
    fn dhash_of_solid_color_is_flat() {
        let solid = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            64,
            64,
            image::Rgb([128, 128, 128]),
        ));
        let out = prepare(&encode(&solid, ImageFormat::Png), &cfg()).unwrap();
        assert_eq!(out.phash, "0000000000000000");
        assert!(!crate::vision::cache::is_textured(&out.phash));
    }
}
