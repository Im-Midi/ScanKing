//! 文档边缘自动检测：缩放 → 灰度 → 模糊 → Canny → 膨胀 → 轮廓 → 四边形拟合

use crate::geometry::{approx_quad, convex_hull, min_area_rect, polygon_area, Pt};
use image::{imageops, GrayImage, RgbImage};
use imageproc::contours::find_contours;
use imageproc::distance_transform::Norm;
use imageproc::edges::canny;
use imageproc::morphology::dilate;

const DETECT_SIZE: u32 = 600;

/// 检测图中最像"一页文档"的四边形，返回原图坐标系下的四角（左上/右上/右下/左下）。
/// 检测不到时返回整幅图像的四角。
pub fn detect_quad(img: &RgbImage) -> [Pt; 4] {
    let (ow, oh) = (img.width(), img.height());
    let scale = (DETECT_SIZE as f32 / ow.max(oh) as f32).min(1.0);
    let (sw, sh) = (
        ((ow as f32 * scale).round() as u32).max(8),
        ((oh as f32 * scale).round() as u32).max(8),
    );
    let small = imageops::resize(img, sw, sh, imageops::FilterType::Triangle);
    let gray = imageops::grayscale(&small);
    let blurred = imageproc::filter::gaussian_blur_f32(&gray, 1.4);

    // 用中位数自适应 Canny 阈值
    let med = median(&blurred) as f32;
    let lo = (0.55 * med).clamp(8.0, 120.0);
    let hi = (1.35 * med).clamp(30.0, 240.0);
    let mut best = try_contours(&dilate(&canny(&blurred, lo, hi), Norm::LInf, 1), sw, sh);

    // 第二遍：更宽松的阈值，处理弱对比场景
    if best.is_none() {
        let edges = dilate(&canny(&blurred, lo * 0.5, hi * 0.5), Norm::LInf, 2);
        best = try_contours(&edges, sw, sh);
    }

    match best {
        Some(q) => {
            let inv = 1.0 / scale;
            let mut out = q;
            for p in out.iter_mut() {
                p[0] = (p[0] * inv).clamp(0.0, ow as f32 - 1.0);
                p[1] = (p[1] * inv).clamp(0.0, oh as f32 - 1.0);
            }
            out
        }
        None => full_quad(ow, oh),
    }
}

pub fn full_quad(w: u32, h: u32) -> [Pt; 4] {
    [
        [0.0, 0.0],
        [w as f32 - 1.0, 0.0],
        [w as f32 - 1.0, h as f32 - 1.0],
        [0.0, h as f32 - 1.0],
    ]
}

fn try_contours(edges: &GrayImage, sw: u32, sh: u32) -> Option<[Pt; 4]> {
    let img_area = (sw * sh) as f32;
    let contours = find_contours::<i32>(edges);
    let mut best: Option<([Pt; 4], f32)> = None;
    for c in &contours {
        if c.points.len() < 8 {
            continue;
        }
        let pts: Vec<Pt> = c.points.iter().map(|p| [p.x as f32, p.y as f32]).collect();
        let hull = convex_hull(&pts);
        if hull.len() < 4 {
            continue;
        }
        let hull_area = polygon_area(&hull);
        // 面积必须占画面 12% 以上，且不能几乎等于整个画面（那是图像边框）
        if hull_area < img_area * 0.12 || hull_area > img_area * 0.985 {
            continue;
        }
        let quad = match approx_quad(&hull) {
            Some(q) => q,
            None => {
                let r = min_area_rect(&hull);
                // 最小外接矩形与凸包面积差太多说明形状不像四边形
                if polygon_area(&r.to_vec()) > hull_area * 1.35 {
                    continue;
                }
                r
            }
        };
        let qa = polygon_area(&quad.to_vec());
        if qa < img_area * 0.10 {
            continue;
        }
        match best {
            Some((_, a)) if a >= qa => {}
            _ => best = Some((quad, qa)),
        }
    }
    best.map(|(q, _)| q)
}

fn median(g: &GrayImage) -> u8 {
    let mut hist = [0u32; 256];
    for p in g.pixels() {
        hist[p.0[0] as usize] += 1;
    }
    let half = (g.width() * g.height()) / 2;
    let mut acc = 0u32;
    for (i, &c) in hist.iter().enumerate() {
        acc += c;
        if acc >= half {
            return i as u8;
        }
    }
    128
}
