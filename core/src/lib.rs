//! scanking-core：扫描 App 的图像处理与文档管理核心
//!
//! - `detect`    文档边缘自动检测
//! - `geometry`  四边形几何与透视矫正
//! - `filters`   增强滤镜
//! - `pdf`       PDF 生成
//! - `idcard`    证件拼页
//! - `store`     文档库存储
//! - `ocr`       离线 OCR（feature = "ocr"）

pub mod detect;
pub mod filters;
pub mod geometry;
pub mod idcard;
pub mod pdf;
pub mod store;

#[cfg(feature = "ocr")]
pub mod ocr;
