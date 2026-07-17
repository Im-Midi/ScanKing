//! 离线 OCR：PaddleOCR（DBNet 检测 + CRNN/SVTR 识别 + 可选方向分类）ONNX 模型，
//! 由纯 Rust 的 tract 推理，无需任何原生动态库，可直接交叉编译到 Android。
//!
//! 模型目录需包含：det.onnx、rec.onnx、keys.txt（字典），可选 cls.onnx。

#![cfg(feature = "ocr")]

use crate::geometry::{convex_hull, dist, min_area_rect, warp_quad, Pt};
use anyhow::{anyhow, Context, Result};
use image::{imageops, RgbImage};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tract_onnx::prelude::*;

type Plan = TypedRunnableModel<TypedModel>;

const DET_SIZE: u32 = 960;
const DET_THRESH: f32 = 0.30;
const BOX_SCORE_THRESH: f32 = 0.45;
const UNCLIP: f32 = 1.9;
const REC_H: u32 = 48;
const REC_BUCKETS: [u32; 6] = [80, 160, 240, 320, 480, 640];
const CLS_W: u32 = 192;

#[derive(Debug, Clone, Serialize)]
pub struct OcrLine {
    pub text: String,
    pub score: f32,
    /// 原图坐标系四角
    pub quad: [[f32; 2]; 4],
}

pub struct OcrEngine {
    det: Plan,
    rec_path: PathBuf,
    rec_plans: Mutex<HashMap<u32, Plan>>,
    cls: Option<Plan>,
    keys: Vec<String>,
}

/// 在字节流中做等长替换（长度相同才不会破坏 protobuf 结构）
fn replace_bytes_inplace(data: &mut [u8], from: &[u8], to: &[u8]) {
    debug_assert_eq!(from.len(), to.len());
    if from.is_empty() || data.len() < from.len() {
        return;
    }
    let mut i = 0;
    while i + from.len() <= data.len() {
        if &data[i..i + from.len()] == from {
            data[i..i + from.len()].copy_from_slice(to);
            i += from.len();
        } else {
            i += 1;
        }
    }
}

fn load_plan(path: &Path, shape: [usize; 4]) -> Result<Plan> {
    let mut bytes =
        std::fs::read(path).with_context(|| format!("读取模型失败: {}", path.display()))?;
    // paddle2onnx 导出的动态维度符号名形如 "p2o.DynamicDimension.0"，
    // tract 的维度符号解析器不接受点号，做等长替换绕开（随后会用固定形状覆盖）。
    replace_bytes_inplace(&mut bytes, b"p2o.DynamicDimension.", b"p2oxDynamicDimensionx");
    let model = tract_onnx::onnx()
        .model_for_read(&mut std::io::Cursor::new(&bytes))
        .with_context(|| format!("加载模型失败: {}", path.display()))?
        .with_input_fact(
            0,
            InferenceFact::dt_shape(f32::datum_type(), tvec!(shape[0], shape[1], shape[2], shape[3])),
        )?
        .into_optimized()?
        .into_runnable()?;
    Ok(model)
}

impl OcrEngine {
    /// 检查模型文件是否齐全且大小合理（阈值贴近真实体积，截断残缺文件会被判定为未安装）
    pub fn ready(dir: &Path) -> bool {
        let ok = |name: &str, min: u64| {
            dir.join(name).metadata().map(|m| m.len() >= min).unwrap_or(false)
        };
        ok("det.onnx", 4_500_000) && ok("rec.onnx", 10_500_000) && ok("keys.txt", 20_000)
    }

    /// 删除模型文件（加载失败时自愈：清掉残缺文件，下次自动重新安装）
    pub fn purge(dir: &Path) {
        for f in ["det.onnx", "rec.onnx", "cls.onnx", "keys.txt"] {
            let _ = std::fs::remove_file(dir.join(f));
        }
    }

    pub fn load(dir: &Path) -> Result<OcrEngine> {
        let det = load_plan(&dir.join("det.onnx"), [1, 3, DET_SIZE as usize, DET_SIZE as usize])?;
        let keys_raw = std::fs::read_to_string(dir.join("keys.txt")).context("读取字典失败")?;
        let keys: Vec<String> = keys_raw.lines().map(|l| l.to_string()).collect();
        if keys.is_empty() {
            return Err(anyhow!("字典为空"));
        }
        let cls_path = dir.join("cls.onnx");
        let cls = if cls_path.exists() {
            Some(load_plan(&cls_path, [1, 3, REC_H as usize, CLS_W as usize])?)
        } else {
            None
        };
        Ok(OcrEngine {
            det,
            rec_path: dir.join("rec.onnx"),
            rec_plans: Mutex::new(HashMap::new()),
            cls,
            keys,
        })
    }

    /// 对整幅图执行 OCR，返回按阅读顺序排列的文本行
    pub fn ocr(&self, img: &RgbImage) -> Result<Vec<OcrLine>> {
        // 控制识别分辨率上限
        let img = if img.width().max(img.height()) > 2000 {
            let s = 2000.0 / img.width().max(img.height()) as f32;
            imageops::resize(
                img,
                (img.width() as f32 * s) as u32,
                (img.height() as f32 * s) as u32,
                imageops::FilterType::Triangle,
            )
        } else {
            img.clone()
        };

        let boxes = self.detect(&img)?;
        let mut lines: Vec<OcrLine> = Vec::new();
        for b in boxes {
            let crop = warp_quad(&img, b);
            let crop = self.maybe_flip(crop)?;
            if let Some((text, score)) = self.recognize(&crop)? {
                if !text.trim().is_empty() {
                    lines.push(OcrLine {
                        text,
                        score,
                        quad: b,
                    });
                }
            }
        }
        Ok(sort_reading_order(lines))
    }

    pub fn ocr_to_text(&self, img: &RgbImage) -> Result<String> {
        let lines = self.ocr(img)?;
        Ok(join_lines(&lines))
    }

    // ---------- 检测 ----------

    fn detect(&self, img: &RgbImage) -> Result<Vec<[Pt; 4]>> {
        let (ow, oh) = (img.width(), img.height());
        let scale = DET_SIZE as f32 / ow.max(oh) as f32;
        let rw = ((ow as f32 * scale).round() as u32).clamp(8, DET_SIZE);
        let rh = ((oh as f32 * scale).round() as u32).clamp(8, DET_SIZE);
        let resized = imageops::resize(img, rw, rh, imageops::FilterType::Triangle);

        // letterbox 到 960x960，归一化 (x/255 - mean) / std
        let mean = [0.485f32, 0.456, 0.406];
        let std = [0.229f32, 0.224, 0.225];
        let n = (DET_SIZE * DET_SIZE) as usize;
        let mut data = vec![0.0f32; 3 * n];
        for c in 0..3 {
            let base = c * n;
            let fill = (0.0 - mean[c]) / std[c];
            data[base..base + n].fill(fill);
        }
        for (x, y, p) in resized.enumerate_pixels() {
            let idx = y as usize * DET_SIZE as usize + x as usize;
            for c in 0..3 {
                data[c * n + idx] = (p[c] as f32 / 255.0 - mean[c]) / std[c];
            }
        }
        let tensor = Tensor::from(
            tract_ndarray::Array4::from_shape_vec(
                (1, 3, DET_SIZE as usize, DET_SIZE as usize),
                data,
            )?,
        );
        let result = self.det.run(tvec!(tensor.into()))?;
        let out = result[0].to_array_view::<f32>()?;
        let flat: Vec<f32> = out.iter().copied().collect();
        if flat.len() < n {
            return Err(anyhow!("检测输出尺寸异常"));
        }

        // 二值化 + 连通域
        let mask: Vec<bool> = flat[..n].iter().map(|&v| v > DET_THRESH).collect();
        let comps = connected_components(&mask, DET_SIZE as usize, DET_SIZE as usize);

        let mut boxes = Vec::new();
        for comp in comps {
            if comp.len() < 24 {
                continue;
            }
            let score: f32 = comp.iter().map(|&i| flat[i]).sum::<f32>() / comp.len() as f32;
            if score < BOX_SCORE_THRESH {
                continue;
            }
            let pts: Vec<Pt> = comp
                .iter()
                .map(|&i| [(i % DET_SIZE as usize) as f32, (i / DET_SIZE as usize) as f32])
                .collect();
            let hull = convex_hull(&pts);
            if hull.len() < 3 {
                continue;
            }
            let rect = min_area_rect(&hull);
            let w = dist(rect[0], rect[1]);
            let h = dist(rect[0], rect[3]);
            if w.min(h) < 3.0 {
                continue;
            }
            // DB unclip：按 面积*系数/周长 外扩
            let area = w * h;
            let perim = 2.0 * (w + h);
            let d = area * UNCLIP / perim.max(1.0);
            let rect = expand_rect(rect, d);
            // 映回原图坐标
            let mut q = rect;
            for p in q.iter_mut() {
                p[0] = (p[0] / scale).clamp(0.0, ow as f32 - 1.0);
                p[1] = (p[1] / scale).clamp(0.0, oh as f32 - 1.0);
            }
            boxes.push(q);
        }
        Ok(boxes)
    }

    // ---------- 方向分类（180°） ----------

    fn maybe_flip(&self, crop: RgbImage) -> Result<RgbImage> {
        let Some(cls) = &self.cls else { return Ok(crop) };
        let tensor = to_rec_tensor(&crop, CLS_W);
        let result = cls.run(tvec!(tensor.into()))?;
        let out = result[0].to_array_view::<f32>()?;
        let v: Vec<f32> = out.iter().copied().collect();
        if v.len() >= 2 && v[1] > v[0] && v[1] > 0.9 {
            Ok(imageops::rotate180(&crop))
        } else {
            Ok(crop)
        }
    }

    // ---------- 识别 ----------

    fn recognize(&self, crop: &RgbImage) -> Result<Option<(String, f32)>> {
        let (w, h) = (crop.width(), crop.height());
        if w < 4 || h < 4 {
            return Ok(None);
        }
        let target_w = (w as f32 * REC_H as f32 / h as f32).round() as u32;
        let bucket = *REC_BUCKETS
            .iter()
            .find(|&&b| b >= target_w)
            .unwrap_or(&REC_BUCKETS[REC_BUCKETS.len() - 1]);

        self.ensure_rec_plan(bucket)?;
        let tensor = to_rec_tensor(crop, bucket);
        let result = {
            let plans = self.rec_plans.lock().unwrap();
            let plan = plans.get(&bucket).ok_or_else(|| anyhow!("识别模型未加载"))?;
            plan.run(tvec!(tensor.into()))?
        };
        let out = result[0].to_array_view::<f32>()?;
        let shape = out.shape().to_vec();
        if shape.len() != 3 {
            return Err(anyhow!("识别输出维度异常: {:?}", shape));
        }
        let (t, c) = (shape[1], shape[2]);
        let flat: Vec<f32> = out.iter().copied().collect();

        // CTC 解码：逐帧 argmax → 去重 → 去 blank(0)
        let mut text = String::new();
        let mut scores = Vec::new();
        let mut last_idx = 0usize;
        for ti in 0..t {
            let row = &flat[ti * c..(ti + 1) * c];
            let (mut bi, mut bv) = (0usize, f32::MIN);
            for (i, &v) in row.iter().enumerate() {
                if v > bv {
                    bv = v;
                    bi = i;
                }
            }
            if bi != 0 && bi != last_idx {
                if bi - 1 < self.keys.len() {
                    text.push_str(&self.keys[bi - 1]);
                } else {
                    text.push(' ');
                }
                scores.push(bv);
            }
            last_idx = bi;
        }
        if text.is_empty() {
            return Ok(None);
        }
        let score = scores.iter().sum::<f32>() / scores.len() as f32;
        Ok(Some((text, score)))
    }

    fn ensure_rec_plan(&self, bucket: u32) -> Result<()> {
        let mut plans = self.rec_plans.lock().unwrap();
        if !plans.contains_key(&bucket) {
            let plan = load_plan(&self.rec_path, [1, 3, REC_H as usize, bucket as usize])?;
            plans.insert(bucket, plan);
        }
        Ok(())
    }
}

/// 缩放到高 48、宽 bucket（不足右侧留灰），归一化 (x/127.5 - 1)
fn to_rec_tensor(crop: &RgbImage, bucket: u32) -> Tensor {
    let (w, h) = (crop.width(), crop.height());
    let target_w = ((w as f32 * REC_H as f32 / h as f32).round() as u32).clamp(1, bucket);
    let resized = imageops::resize(crop, target_w, REC_H, imageops::FilterType::Triangle);
    let n = (REC_H * bucket) as usize;
    let mut data = vec![0.0f32; 3 * n];
    for (x, y, p) in resized.enumerate_pixels() {
        let idx = y as usize * bucket as usize + x as usize;
        for c in 0..3 {
            data[c * n + idx] = p[c] as f32 / 127.5 - 1.0;
        }
    }
    Tensor::from(
        tract_ndarray::Array4::from_shape_vec((1, 3, REC_H as usize, bucket as usize), data)
            .unwrap(),
    )
}

/// 二值图连通域（4 邻接，非递归洪泛）
fn connected_components(mask: &[bool], w: usize, h: usize) -> Vec<Vec<usize>> {
    let mut visited = vec![false; mask.len()];
    let mut comps = Vec::new();
    let mut stack = Vec::new();
    for start in 0..mask.len() {
        if !mask[start] || visited[start] {
            continue;
        }
        let mut comp = Vec::new();
        visited[start] = true;
        stack.push(start);
        while let Some(i) = stack.pop() {
            comp.push(i);
            let (x, y) = (i % w, i / w);
            let mut push = |nx: usize, ny: usize| {
                let ni = ny * w + nx;
                if mask[ni] && !visited[ni] {
                    visited[ni] = true;
                    stack.push(ni);
                }
            };
            if x > 0 {
                push(x - 1, y);
            }
            if x + 1 < w {
                push(x + 1, y);
            }
            if y > 0 {
                push(x, y - 1);
            }
            if y + 1 < h {
                push(x, y + 1);
            }
        }
        comps.push(comp);
    }
    comps
}

/// 沿矩形自身两条轴向外扩 d
fn expand_rect(rect: [Pt; 4], d: f32) -> [Pt; 4] {
    let ux = rect[1][0] - rect[0][0];
    let uy = rect[1][1] - rect[0][1];
    let ul = (ux * ux + uy * uy).sqrt().max(1e-6);
    let (ux, uy) = (ux / ul, uy / ul);
    let vx = rect[3][0] - rect[0][0];
    let vy = rect[3][1] - rect[0][1];
    let vl = (vx * vx + vy * vy).sqrt().max(1e-6);
    let (vx, vy) = (vx / vl, vy / vl);
    [
        [rect[0][0] - ux * d - vx * d, rect[0][1] - uy * d - vy * d],
        [rect[1][0] + ux * d - vx * d, rect[1][1] + uy * d - vy * d],
        [rect[2][0] + ux * d + vx * d, rect[2][1] + uy * d + vy * d],
        [rect[3][0] - ux * d + vx * d, rect[3][1] - uy * d + vy * d],
    ]
}

/// 按阅读顺序排序：先按行分组（垂直中心接近），行内按 x 排序
fn sort_reading_order(mut lines: Vec<OcrLine>) -> Vec<OcrLine> {
    lines.sort_by(|a, b| {
        center_y(a).partial_cmp(&center_y(b)).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut rows: Vec<Vec<OcrLine>> = Vec::new();
    for l in lines {
        let cy = center_y(&l);
        let hh = height_of(&l);
        if let Some(row) = rows.last_mut() {
            let rcy = row.iter().map(center_y).sum::<f32>() / row.len() as f32;
            let rh = row.iter().map(height_of).sum::<f32>() / row.len() as f32;
            if (cy - rcy).abs() < rh.max(hh) * 0.6 {
                row.push(l);
                continue;
            }
        }
        rows.push(vec![l]);
    }
    let mut out = Vec::new();
    for mut row in rows {
        row.sort_by(|a, b| {
            a.quad[0][0].partial_cmp(&b.quad[0][0]).unwrap_or(std::cmp::Ordering::Equal)
        });
        out.extend(row);
    }
    out
}

fn center_y(l: &OcrLine) -> f32 {
    (l.quad[0][1] + l.quad[2][1]) / 2.0
}

fn height_of(l: &OcrLine) -> f32 {
    dist(l.quad[0], l.quad[3])
}

/// 同一视觉行内的片段智能连接：中英文之间不加空格，英文单词间加空格
fn join_lines(lines: &[OcrLine]) -> String {
    let mut out = String::new();
    let mut prev_cy: Option<f32> = None;
    for l in lines {
        let cy = center_y(l);
        if let Some(p) = prev_cy {
            if (cy - p).abs() < height_of(l) * 0.6 {
                let prev_ascii = out.chars().last().map(|c| c.is_ascii()).unwrap_or(false);
                let next_ascii = l.text.chars().next().map(|c| c.is_ascii()).unwrap_or(false);
                if prev_ascii && next_ascii {
                    out.push(' ');
                }
            } else {
                out.push('\n');
            }
        }
        out.push_str(&l.text);
        prev_cy = Some(cy);
    }
    out
}
