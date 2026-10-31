// 本地 OCR 接口
//
// P0 只保留 trait 与空实现；P2 再接 ONNX/PP-OCR。
// 这样视觉主链路可以先跑通，OCR 只在真正配置了引擎时才参与。

use anyhow::Result;

use super::types::PreparedImage;

#[derive(Debug, Clone)]
pub struct OcrLine {
    pub text: String,
    pub confidence: f32,
}

#[derive(Debug, Clone)]
pub struct OcrResult {
    pub text: String,
    pub lines: Vec<OcrLine>,
    pub confidence: f32,
}

pub trait OcrEngine: Send + Sync {
    fn recognize(&self, image: &PreparedImage) -> Result<OcrResult>;
}

/// 未启用 OCR 时的空实现：永远返回「没有结果」
pub struct NoopOcr;

impl OcrEngine for NoopOcr {
    fn recognize(&self, _image: &PreparedImage) -> Result<OcrResult> {
        anyhow::bail!("OCR 未启用")
    }
}
