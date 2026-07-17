//! 极简 PDF 生成器：把 JPEG 页面直接以 DCTDecode 嵌入，零外部依赖，输出体积小。

use anyhow::{bail, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageMode {
    /// 页面大小 = 图像大小（按 150 DPI 折算）
    Fit,
    /// A4 纵向，图像等比缩放居中
    A4,
}

impl PageMode {
    pub fn from_name(s: &str) -> PageMode {
        if s == "a4" {
            PageMode::A4
        } else {
            PageMode::Fit
        }
    }
}

pub struct PdfPage {
    pub jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

const A4_W: f64 = 595.28;
const A4_H: f64 = 841.89;
const A4_MARGIN: f64 = 18.0;
const FIT_DPI: f64 = 150.0;

/// 把若干 JPEG 页组装为一份 PDF 文件字节
pub fn build_pdf(pages: &[PdfPage], mode: PageMode) -> Result<Vec<u8>> {
    if pages.is_empty() {
        bail!("没有页面可导出");
    }
    let mut buf: Vec<u8> = Vec::with_capacity(pages.iter().map(|p| p.jpeg.len() + 512).sum());
    buf.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");

    let nobj = 2 + pages.len() * 3;
    let mut offsets = vec![0usize; nobj + 1];

    let write_obj = |buf: &mut Vec<u8>, offsets: &mut Vec<usize>, id: usize, body: &[u8]| {
        offsets[id] = buf.len();
        buf.extend_from_slice(format!("{} 0 obj\n", id).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    };

    // 1: Catalog
    write_obj(&mut buf, &mut offsets, 1, b"<< /Type /Catalog /Pages 2 0 R >>");

    // 2: Pages
    let kids: Vec<String> = (0..pages.len()).map(|i| format!("{} 0 R", 3 + i * 3)).collect();
    write_obj(
        &mut buf,
        &mut offsets,
        2,
        format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), pages.len()).as_bytes(),
    );

    for (i, page) in pages.iter().enumerate() {
        let page_id = 3 + i * 3;
        let content_id = page_id + 1;
        let image_id = page_id + 2;

        let (pw, ph, iw, ih, tx, ty) = match mode {
            PageMode::Fit => {
                let pw = page.width as f64 * 72.0 / FIT_DPI;
                let ph = page.height as f64 * 72.0 / FIT_DPI;
                (pw, ph, pw, ph, 0.0, 0.0)
            }
            PageMode::A4 => {
                let avail_w = A4_W - A4_MARGIN * 2.0;
                let avail_h = A4_H - A4_MARGIN * 2.0;
                let s = (avail_w / page.width as f64).min(avail_h / page.height as f64);
                let iw = page.width as f64 * s;
                let ih = page.height as f64 * s;
                (A4_W, A4_H, iw, ih, (A4_W - iw) / 2.0, (A4_H - ih) / 2.0)
            }
        };

        write_obj(
            &mut buf,
            &mut offsets,
            page_id,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.2} {:.2}] \
                 /Resources << /XObject << /Im0 {} 0 R >> >> /Contents {} 0 R >>",
                pw, ph, image_id, content_id
            )
            .as_bytes(),
        );

        let content = format!("q\n{:.2} 0 0 {:.2} {:.2} {:.2} cm\n/Im0 Do\nQ", iw, ih, tx, ty);
        write_obj(
            &mut buf,
            &mut offsets,
            content_id,
            format!("<< /Length {} >>\nstream\n{}\nendstream", content.len(), content).as_bytes(),
        );

        offsets[image_id] = buf.len();
        buf.extend_from_slice(format!("{} 0 obj\n", image_id).as_bytes());
        buf.extend_from_slice(
            format!(
                "<< /Type /XObject /Subtype /Image /Width {} /Height {} \
                 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
                page.width,
                page.height,
                page.jpeg.len()
            )
            .as_bytes(),
        );
        buf.extend_from_slice(&page.jpeg);
        buf.extend_from_slice(b"\nendstream\nendobj\n");
    }

    // xref
    let xref_pos = buf.len();
    buf.extend_from_slice(format!("xref\n0 {}\n", nobj + 1).as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for id in 1..=nobj {
        buf.extend_from_slice(format!("{:010} 00000 n \n", offsets[id]).as_bytes());
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
            nobj + 1,
            xref_pos
        )
        .as_bytes(),
    );
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_jpeg() -> (Vec<u8>, u32, u32) {
        let img = image::RgbImage::from_pixel(60, 40, image::Rgb([200, 220, 240]));
        let mut out = Vec::new();
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85);
        enc.encode_image(&img).unwrap();
        (out, 60, 40)
    }

    #[test]
    fn test_pdf_structure() {
        let (j, w, h) = tiny_jpeg();
        let pages = vec![
            PdfPage { jpeg: j.clone(), width: w, height: h },
            PdfPage { jpeg: j, width: w, height: h },
        ];
        for mode in [PageMode::Fit, PageMode::A4] {
            let pdf = build_pdf(&pages, mode).unwrap();
            assert!(pdf.starts_with(b"%PDF-1.4"));
            let s = String::from_utf8_lossy(&pdf);
            assert!(s.contains("/Count 2"));
            assert!(s.contains("startxref"));
            assert!(s.ends_with("%%EOF\n"));
        }
    }
}
