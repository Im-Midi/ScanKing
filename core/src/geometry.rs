//! 几何工具：四角排序、凸包、多边形简化、最小外接矩形、单应矩阵、透视矫正

use image::RgbImage;
use rayon::prelude::*;

pub type Pt = [f32; 2];

#[inline]
pub fn dist(a: Pt, b: Pt) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// 将四个角点排序为 [左上, 右上, 右下, 左下]
pub fn order_quad(pts: [Pt; 4]) -> [Pt; 4] {
    let (mut tl, mut tr, mut br, mut bl) = (pts[0], pts[0], pts[0], pts[0]);
    let (mut min_s, mut max_s, mut min_d, mut max_d) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for p in pts {
        let s = p[0] + p[1];
        let d = p[0] - p[1];
        if s < min_s { min_s = s; tl = p; }
        if s > max_s { max_s = s; br = p; }
        if d > max_d { max_d = d; tr = p; }
        if d < min_d { min_d = d; bl = p; }
    }
    [tl, tr, br, bl]
}

/// 鞋带公式求多边形面积（绝对值）
pub fn polygon_area(pts: &[Pt]) -> f32 {
    if pts.len() < 3 { return 0.0; }
    let n = pts.len();
    let mut s = 0.0f64;
    for i in 0..n {
        let j = (i + 1) % n;
        s += pts[i][0] as f64 * pts[j][1] as f64 - pts[j][0] as f64 * pts[i][1] as f64;
    }
    (s.abs() / 2.0) as f32
}

/// Andrew 单调链凸包，返回逆时针顶点
pub fn convex_hull(points: &[Pt]) -> Vec<Pt> {
    let mut pts: Vec<Pt> = points.to_vec();
    pts.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap().then(a[1].partial_cmp(&b[1]).unwrap()));
    pts.dedup_by(|a, b| a[0] == b[0] && a[1] == b[1]);
    let n = pts.len();
    if n < 3 { return pts; }
    let cross = |o: Pt, a: Pt, b: Pt| -> f64 {
        (a[0] as f64 - o[0] as f64) * (b[1] as f64 - o[1] as f64)
            - (a[1] as f64 - o[1] as f64) * (b[0] as f64 - o[0] as f64)
    };
    let mut hull: Vec<Pt> = Vec::with_capacity(n * 2);
    for &p in &pts {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
            hull.pop();
        }
        hull.push(p);
    }
    let lower_len = hull.len() + 1;
    for &p in pts.iter().rev() {
        while hull.len() >= lower_len && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
            hull.pop();
        }
        hull.push(p);
    }
    hull.pop();
    hull
}

fn point_seg_dist(p: Pt, a: Pt, b: Pt) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    if len2 <= 1e-9 { return dist(p, a); }
    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0);
    dist(p, [a[0] + t * dx, a[1] + t * dy])
}

fn rdp(points: &[Pt], eps: f32, out: &mut Vec<Pt>) {
    if points.len() < 3 {
        out.extend_from_slice(&points[..points.len().saturating_sub(1)]);
        return;
    }
    let (a, b) = (points[0], points[points.len() - 1]);
    let (mut idx, mut maxd) = (0usize, -1.0f32);
    for (i, &p) in points.iter().enumerate().skip(1).take(points.len() - 2) {
        let d = point_seg_dist(p, a, b);
        if d > maxd { maxd = d; idx = i; }
    }
    if maxd > eps {
        rdp(&points[..=idx], eps, out);
        rdp(&points[idx..], eps, out);
    } else {
        out.push(a);
    }
}

/// Douglas-Peucker 简化开放折线（保留首尾点）
fn rdp_run(points: &[Pt], eps: f32) -> Vec<Pt> {
    let mut out = Vec::new();
    rdp(points, eps, &mut out);
    out.push(points[points.len() - 1]);
    out
}

/// 把凸包简化为四边形（逐步放大容差），失败返回 None
pub fn approx_quad(hull: &[Pt]) -> Option<[Pt; 4]> {
    let n = hull.len();
    if n < 4 {
        return None;
    }
    // 找最远点对作为两条链的锚点
    let (mut bi, mut bj, mut bd) = (0usize, 0usize, -1.0f32);
    for i in 0..n {
        for j in (i + 1)..n {
            let d = dist(hull[i], hull[j]);
            if d > bd { bd = d; bi = i; bj = j; }
        }
    }
    let chain_a: Vec<Pt> = (bi..=bj).map(|k| hull[k]).collect();
    let chain_b: Vec<Pt> = (bj..n).chain(0..=bi).map(|k| hull[k]).collect();
    let mut perim = 0.0f32;
    for i in 0..n { perim += dist(hull[i], hull[(i + 1) % n]); }
    let mut eps = perim * 0.02;
    for _ in 0..14 {
        let a = rdp_run(&chain_a, eps);
        let b = rdp_run(&chain_b, eps);
        let total = a.len() + b.len() - 2;
        if total == 4 {
            let mut pts: Vec<Pt> = a;
            pts.extend_from_slice(&b[1..b.len() - 1]);
            return Some(order_quad([pts[0], pts[1], pts[2], pts[3]]));
        }
        if total < 4 { return None; }
        eps *= 1.4;
    }
    None
}

/// 旋转卡壳求最小外接矩形
pub fn min_area_rect(hull: &[Pt]) -> [Pt; 4] {
    let n = hull.len();
    if n == 0 { return [[0.0, 0.0]; 4]; }
    if n < 3 {
        let p = hull[0];
        return [p, p, p, p];
    }
    let mut best_area = f32::MAX;
    let mut best: [Pt; 4] = [[0.0, 0.0]; 4];
    for i in 0..n {
        let a = hull[i];
        let b = hull[(i + 1) % n];
        let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
        let len = (ex * ex + ey * ey).sqrt();
        if len < 1e-6 { continue; }
        let (ux, uy) = (ex / len, ey / len);
        let (vx, vy) = (-uy, ux);
        let (mut umin, mut umax, mut vmin, mut vmax) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for &p in hull {
            let u = p[0] * ux + p[1] * uy;
            let v = p[0] * vx + p[1] * vy;
            umin = umin.min(u); umax = umax.max(u);
            vmin = vmin.min(v); vmax = vmax.max(v);
        }
        let area = (umax - umin) * (vmax - vmin);
        if area < best_area {
            best_area = area;
            let corner = |u: f32, v: f32| -> Pt { [u * ux + v * vx, u * uy + v * vy] };
            best = [corner(umin, vmin), corner(umax, vmin), corner(umax, vmax), corner(umin, vmax)];
        }
    }
    order_quad(best)
}

/// 由 4 对对应点求单应矩阵 H（src -> dst），返回行优先 [h0..h8]，h8 = 1
pub fn homography(src: [Pt; 4], dst: [Pt; 4]) -> Option<[f64; 9]> {
    let mut m = [[0.0f64; 9]; 8];
    for i in 0..4 {
        let (x, y) = (src[i][0] as f64, src[i][1] as f64);
        let (u, v) = (dst[i][0] as f64, dst[i][1] as f64);
        m[i * 2] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        m[i * 2 + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }
    // 高斯消元（部分主元）
    for col in 0..8 {
        let mut piv = col;
        for r in (col + 1)..8 {
            if m[r][col].abs() > m[piv][col].abs() { piv = r; }
        }
        if m[piv][col].abs() < 1e-10 { return None; }
        m.swap(col, piv);
        let d = m[col][col];
        for c in col..9 { m[col][c] /= d; }
        for r in 0..8 {
            if r != col && m[r][col].abs() > 1e-15 {
                let f = m[r][col];
                for c in col..9 { m[r][c] -= f * m[col][c]; }
            }
        }
    }
    let mut h = [0.0f64; 9];
    for i in 0..8 { h[i] = m[i][8]; }
    h[8] = 1.0;
    Some(h)
}

#[inline]
fn apply_h(h: &[f64; 9], x: f64, y: f64) -> (f64, f64) {
    let w = h[6] * x + h[7] * y + h[8];
    let w = if w.abs() < 1e-12 { 1e-12 } else { w };
    ((h[0] * x + h[1] * y + h[2]) / w, (h[3] * x + h[4] * y + h[5]) / w)
}

/// 按四角（左上/右上/右下/左下顺序）做透视矫正，输出摆正后的图像
pub fn warp_quad(src: &RgbImage, quad: [Pt; 4]) -> RgbImage {
    let [tl, tr, br, bl] = quad;
    let w = dist(tl, tr).max(dist(bl, br)).round().clamp(16.0, 6000.0) as u32;
    let h = dist(tl, bl).max(dist(tr, br)).round().clamp(16.0, 6000.0) as u32;
    let dst_rect: [Pt; 4] = [
        [0.0, 0.0],
        [w as f32 - 1.0, 0.0],
        [w as f32 - 1.0, h as f32 - 1.0],
        [0.0, h as f32 - 1.0],
    ];
    // 求 目标 -> 源 的映射
    let hm = match homography(dst_rect, quad) {
        Some(h) => h,
        None => return src.clone(),
    };
    let (sw, sh) = (src.width() as i64, src.height() as i64);
    let sbuf = src.as_raw();
    let mut out = vec![0u8; (w * h * 3) as usize];
    // Catmull-Rom 双三次采样：比双线性明显更锐，文字边缘不发糊
    #[inline]
    fn cr(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
        p1 + 0.5
            * t
            * (p2 - p0
                + t * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3 + t * (3.0 * (p1 - p2) + p3 - p0)))
    }
    out.par_chunks_mut((w * 3) as usize).enumerate().for_each(|(y, row)| {
        for x in 0..w as usize {
            let (sx, sy) = apply_h(&hm, x as f64, y as f64);
            let x0 = sx.floor() as i64;
            let y0 = sy.floor() as i64;
            let fx = (sx - x0 as f64) as f32;
            let fy = (sy - y0 as f64) as f32;
            let cx = |dx: i64| (x0 + dx).clamp(0, sw - 1) as usize;
            let cy = |dy: i64| (y0 + dy).clamp(0, sh - 1) as usize;
            for c in 0..3 {
                let px = |xx: usize, yy: usize| sbuf[(yy * sw as usize + xx) * 3 + c] as f32;
                let mut rows = [0.0f32; 4];
                for (ri, dy) in (-1i64..=2).enumerate() {
                    let yy = cy(dy);
                    rows[ri] = cr(px(cx(-1), yy), px(cx(0), yy), px(cx(1), yy), px(cx(2), yy), fx);
                }
                let v = cr(rows[0], rows[1], rows[2], rows[3], fy);
                row[x * 3 + c] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    });
    RgbImage::from_raw(w, h, out).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_quad() {
        let q = order_quad([[100.0, 0.0], [0.0, 0.0], [0.0, 100.0], [100.0, 100.0]]);
        assert_eq!(q[0], [0.0, 0.0]);
        assert_eq!(q[1], [100.0, 0.0]);
        assert_eq!(q[2], [100.0, 100.0]);
        assert_eq!(q[3], [0.0, 100.0]);
    }

    #[test]
    fn test_homography_identity() {
        let r: [Pt; 4] = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let h = homography(r, r).unwrap();
        let (x, y) = apply_h(&h, 3.0, 7.0);
        assert!((x - 3.0).abs() < 1e-6 && (y - 7.0).abs() < 1e-6);
    }

    #[test]
    fn test_convex_hull_area() {
        let pts: Vec<Pt> = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [5.0, 5.0]];
        let hull = convex_hull(&pts);
        assert_eq!(hull.len(), 4);
        assert!((polygon_area(&hull) - 100.0).abs() < 1e-3);
    }
}
