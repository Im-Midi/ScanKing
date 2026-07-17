//! 端到端管线测试：合成一张"斜放的文档照片"，验证检测→矫正→滤镜→存储→导出全链路

use image::{Rgb, RgbImage};
use scanking_core::detect::detect_quad;
use scanking_core::filters::{apply_filter, Filter};
use scanking_core::geometry::{dist, order_quad, warp_quad, Pt};
use scanking_core::store::Store;

/// 生成深色背景 + 倾斜亮色"文档"的合成照片，返回 (图像, 真实四角)
fn synth_photo() -> (RgbImage, [Pt; 4]) {
    let (w, h) = (1000u32, 750u32);
    let quad: [Pt; 4] = [
        [230.0, 130.0],
        [830.0, 180.0],
        [780.0, 620.0],
        [180.0, 560.0],
    ];
    let mut img = RgbImage::from_pixel(w, h, Rgb([58, 62, 70]));
    let inside = |x: f32, y: f32| -> bool {
        let mut sign = 0i32;
        for i in 0..4 {
            let a = quad[i];
            let b = quad[(i + 1) % 4];
            let c = (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
            let s = if c >= 0.0 { 1 } else { -1 };
            if sign == 0 {
                sign = s;
            } else if s != sign {
                return false;
            }
        }
        true
    };
    for y in 0..h {
        for x in 0..w {
            if inside(x as f32, y as f32) {
                let shade = 232 - (y as f32 * 0.02) as u8;
                img.put_pixel(x, y, Rgb([shade, shade, shade.saturating_sub(3)]));
            }
        }
    }
    for row in 0..6 {
        let y0 = 240 + row * 55;
        for y in y0..(y0 + 14) {
            for x in 320..700 {
                if inside(x as f32, y as f32) {
                    img.put_pixel(x, y as u32, Rgb([40, 40, 45]));
                }
            }
        }
    }
    (img, quad)
}

fn jpeg_bytes(img: &RgbImage) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
    enc.encode_image(img).unwrap();
    out
}

#[test]
fn test_detect_quad_accuracy() {
    let (img, truth) = synth_photo();
    let detected = order_quad(detect_quad(&img));
    let truth = order_quad(truth);
    for i in 0..4 {
        let d = dist(detected[i], truth[i]);
        assert!(
            d < 25.0,
            "角点 {} 偏差过大: {:?} vs {:?} (d={})",
            i,
            detected[i],
            truth[i],
            d
        );
    }
}

#[test]
fn test_warp_produces_upright_page() {
    let (img, truth) = synth_photo();
    let warped = warp_quad(&img, order_quad(truth));
    assert!(warped.width() >= 500 && warped.height() >= 350);
    for (x, y) in [
        (8u32, 8u32),
        (warped.width() - 8, 8),
        (warped.width() - 8, warped.height() - 8),
        (8, warped.height() - 8),
    ] {
        let p = warped.get_pixel(x, y);
        assert!(p[0] > 150, "角 ({},{}) 不是纸面: {:?}", x, y, p);
    }
}

#[test]
fn test_filters_run() {
    let (img, truth) = synth_photo();
    let warped = warp_quad(&img, order_quad(truth));
    for f in [Filter::Original, Filter::Enhance, Filter::Bw, Filter::Gray, Filter::Shadow] {
        let out = apply_filter(&warped, f);
        assert_eq!((out.width(), out.height()), (warped.width(), warped.height()));
    }
    let bw = apply_filter(&warped, Filter::Bw);
    for p in bw.pixels() {
        assert!(p[0] == 0 || p[0] == 255);
    }
    let enh = apply_filter(&warped, Filter::Enhance);
    let center_bg = enh.get_pixel(30, 30);
    assert!(center_bg[0] > 200, "增强后纸面不够亮: {:?}", center_bg);
}

#[test]
fn test_store_full_workflow() {
    let root = std::env::temp_dir().join(format!("sk_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let store = Store::open(&root).unwrap();

    let doc = store.create_doc("测试合同").unwrap();
    let (img, _) = synth_photo();
    let bytes = jpeg_bytes(&img);
    let p1 = store.add_page(&doc.id, &bytes).unwrap();
    let p2 = store.add_page(&doc.id, &bytes).unwrap();
    assert!(std::path::Path::new(&p1.work).exists());
    assert!(std::path::Path::new(&p1.thumb).exists());
    assert!(p1.work_width > 0);

    let full_area = (img.width() * img.height()) as f32;
    let quad_area = scanking_core::geometry::polygon_area(&p1.quad.to_vec());
    assert!(quad_area < full_area * 0.75, "检测到的区域不应是整幅图");

    let list = store.list_docs().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].page_count, 2);
    assert_eq!(store.search_docs("合同").unwrap().len(), 1);
    assert_eq!(store.search_docs("不存在的词").unwrap().len(), 0);

    let info = store.set_filter(&doc.id, &p1.meta.id, "bw").unwrap();
    assert_eq!(info.meta.filter, "bw");

    let manual = [[100.0f32, 100.0], [900.0, 120.0], [880.0, 700.0], [90.0, 680.0]];
    let info = store.set_geometry(&doc.id, &p1.meta.id, Some(manual), 1).unwrap();
    assert_eq!(info.meta.rotation, 1);

    store.save_ocr_text(&doc.id, &p1.meta.id, "识别出的文字ABC").unwrap();
    assert!(store.doc_ocr_text(&doc.id).unwrap().contains("ABC"));
    assert_eq!(store.search_docs("识别出").unwrap().len(), 1);

    let before = store.get_doc(&doc.id).unwrap().pages.len();
    let composed = store.idcard_compose(&doc.id, &p1.meta.id, &p2.meta.id).unwrap();
    assert_eq!(store.get_doc(&doc.id).unwrap().pages.len(), before + 1);
    assert!(composed.work_width > 0);

    let doc_meta = store.get_doc(&doc.id).unwrap();
    let mut ids: Vec<String> = doc_meta.pages.iter().map(|p| p.id.clone()).collect();
    ids.reverse();
    store.reorder_pages(&doc.id, &ids).unwrap();
    assert_eq!(store.get_doc(&doc.id).unwrap().pages[0].id, ids[0]);

    for mode in ["fit", "a4"] {
        let pdf_path = store.export_pdf(&doc.id, mode, 85).unwrap();
        let data = std::fs::read(&pdf_path).unwrap();
        assert!(data.starts_with(b"%PDF"));
        assert!(data.len() > 1000);
    }

    store.delete_page(&doc.id, &p2.meta.id).unwrap();
    store.delete_doc(&doc.id).unwrap();
    assert_eq!(store.list_docs().unwrap().len(), 0);

    let _ = std::fs::remove_dir_all(&root);
}

/// 生成一份 PDF 留在临时目录，供外部工具（pypdf 等）二次校验
#[test]
fn test_pdf_sample_for_external_check() {
    use scanking_core::pdf::{build_pdf, PageMode, PdfPage};
    let (img, truth) = synth_photo();
    let warped = warp_quad(&img, order_quad(truth));
    let enhanced = apply_filter(&warped, Filter::Enhance);
    let make = |i: &RgbImage| PdfPage { jpeg: jpeg_bytes(i), width: i.width(), height: i.height() };
    let pdf = build_pdf(&[make(&warped), make(&enhanced)], PageMode::A4).unwrap();
    std::fs::write(std::env::temp_dir().join("sk_sample.pdf"), pdf).unwrap();
}

#[test]
fn test_rename_and_tags() {
    let root = std::env::temp_dir().join(format!("sk_test2_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let store = Store::open(&root).unwrap();
    let doc = store.create_doc("").unwrap();
    assert!(!doc.title.is_empty(), "空标题应自动生成");
    store.rename_doc(&doc.id, "发票 2026").unwrap();
    store.set_tags(&doc.id, vec!["报销".into(), "7月".into()]).unwrap();
    let list = store.list_docs().unwrap();
    assert_eq!(list[0].title, "发票 2026");
    assert_eq!(store.search_docs("报销").unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}
