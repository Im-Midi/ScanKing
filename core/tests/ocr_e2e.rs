//! OCR 端到端测试：需要真实模型文件，通过环境变量启用：
//!   SK_MODELS=模型目录 SK_OCR_IMG=测试图片 cargo test -p scanking-core --test ocr_e2e -- --nocapture
//! 未设置环境变量时自动跳过（CI/无模型环境安全）。

#![cfg(feature = "ocr")]

use scanking_core::ocr::OcrEngine;
use std::path::PathBuf;

#[test]
fn test_ocr_real_models() {
    let Ok(models) = std::env::var("SK_MODELS") else {
        eprintln!("SK_MODELS 未设置，跳过 OCR 端到端测试");
        return;
    };
    let Ok(img_path) = std::env::var("SK_OCR_IMG") else {
        eprintln!("SK_OCR_IMG 未设置，跳过");
        return;
    };
    let dir = PathBuf::from(models);
    assert!(OcrEngine::ready(&dir), "模型文件不齐全");

    let t0 = std::time::Instant::now();
    let engine = OcrEngine::load(&dir).expect("加载 OCR 模型失败");
    eprintln!("模型加载耗时: {:?}", t0.elapsed());

    let img = image::open(&img_path).expect("读取测试图片失败").to_rgb8();
    let t1 = std::time::Instant::now();
    let lines = engine.ocr(&img).expect("OCR 推理失败");
    eprintln!("推理耗时: {:?}", t1.elapsed());

    for l in &lines {
        eprintln!("[{:.2}] {}", l.score, l.text);
    }
    let all: String = lines.iter().map(|l| l.text.to_lowercase()).collect::<Vec<_>>().join("\n");
    assert!(!lines.is_empty(), "没有识别到任何文本");
    assert!(all.contains("hello"), "未识别出 hello: {}", all);
    assert!(all.contains("123"), "未识别出 123: {}", all);

    let text = engine.ocr_to_text(&img).unwrap();
    eprintln!("--- 合成文本 ---\n{}", text);
    assert!(!text.trim().is_empty());
}
