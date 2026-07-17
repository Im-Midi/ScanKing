//! 证件拼页：把正反面两张图合成到一张 A4（300 DPI）白底页上

use image::{imageops, RgbImage};

const A4_W: u32 = 2480;
const A4_H: u32 = 3508;
const BOX_W: u32 = 1720;
const BOX_H: u32 = 1160;

pub fn compose(a: &RgbImage, b: &RgbImage) -> RgbImage {
    let mut canvas = RgbImage::from_pixel(A4_W, A4_H, image::Rgb([255, 255, 255]));
    place(&mut canvas, a, A4_H / 4);
    place(&mut canvas, b, A4_H * 3 / 4);
    // 中间分隔虚线
    let y = A4_H / 2;
    for x in (60..A4_W - 60).step_by(28) {
        for dx in 0..14 {
            if x + dx < A4_W {
                canvas.put_pixel(x + dx, y, image::Rgb([190, 190, 190]));
            }
        }
    }
    canvas
}

fn place(canvas: &mut RgbImage, img: &RgbImage, center_y: u32) {
    let (w, h) = (img.width(), img.height());
    // 只缩不放：放大会糊，宁可留白
    let s = (BOX_W as f32 / w as f32).min(BOX_H as f32 / h as f32).min(1.0);
    let (nw, nh) = (((w as f32 * s) as u32).max(1), ((h as f32 * s) as u32).max(1));
    let resized = imageops::resize(img, nw, nh, imageops::FilterType::CatmullRom);
    let x0 = (A4_W - nw) / 2;
    let y0 = center_y.saturating_sub(nh / 2);
    imageops::overlay(canvas, &resized, x0 as i64, y0 as i64);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compose_dims() {
        let a = RgbImage::from_pixel(1000, 630, image::Rgb([200, 100, 100]));
        let b = RgbImage::from_pixel(1000, 630, image::Rgb([100, 100, 200]));
        let c = compose(&a, &b);
        assert_eq!((c.width(), c.height()), (A4_W, A4_H));
        // 上下都有内容
        assert_ne!(c.get_pixel(A4_W / 2, A4_H / 4).0, [255, 255, 255]);
        assert_ne!(c.get_pixel(A4_W / 2, A4_H * 3 / 4).0, [255, 255, 255]);
    }
}
