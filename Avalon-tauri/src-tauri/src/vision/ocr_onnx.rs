// OCR 引擎实现：ort + PP-OCRv4（det + cls + rec）
//
// 只做水平/近水平文本。det 的后处理用「二值化 → 连通域 → 轴对齐外接矩形」
// 代替 DB 的轮廓检测与透视变换，换取不引入 opencv 一类重依赖。
// cls 逐文本行判断是否倒置，倒置的先旋转 180° 再送 rec。
// 模型与字典的放置见 doc/v0.7/03-vision-module.md 10.6。
//
// 重资源全部在 OnnxOcr::load 里创建；调用方（OcrHandle）负责懒加载与卸载。

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;

use anyhow::{anyhow, bail, Context, Result};
use image::{imageops, imageops::FilterType, RgbImage};
use ort::environment::Environment;
use ort::session::Session;
use ort::value::Tensor;

use super::{OcrEngine, OcrLine, OcrResult, PreparedImage};

pub const DET_MODEL_FILE: &str = "det.onnx";
pub const CLS_MODEL_FILE: &str = "cls.onnx";
pub const REC_MODEL_FILE: &str = "rec.onnx";
pub const DICT_FILE: &str = "ppocr_keys_v1.txt";

/// det 输入长边上限。PP-OCR 训练分辨率多为 960，再大收益有限、耗时线性上涨
const DET_MAX_SIDE: u32 = 960;
/// det 输入宽高必须是 32 的倍数（下采样 5 次）
const DET_ALIGN: u32 = 32;
/// 概率图二值化阈值
const DET_BINARY_THRESHOLD: f32 = 0.3;
/// 文本框平均分阈值，低于此值视为背景噪点
const DET_BOX_THRESHOLD: f32 = 0.6;
/// DB 外扩比例：d = area * ratio / perimeter
const DET_UNCLIP_RATIO: f32 = 1.5;
/// 单图最多保留的文本框，防止极端图片把识别耗时拖到不可控
const MAX_BOXES: usize = 200;
/// cls 输入固定 48x192（PP-OCR 方向分类模型训练分辨率）
const CLS_HEIGHT: u32 = 48;
const CLS_WIDTH: u32 = 192;
/// rec 输入固定高度
const REC_HEIGHT: u32 = 48;
/// rec 输入宽度范围；过宽会显著增加耗时
const REC_MIN_WIDTH: u32 = 16;
const REC_MAX_WIDTH: u32 = 320;
/// 每个会话的 intra-op 线程数。OCR 是后台辅助能力，不值得抢占全部核心
const OCR_THREADS: usize = 2;

/// det 归一化参数（PP-OCR 标准）
const DET_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const DET_STD: [f32; 3] = [0.229, 0.224, 0.225];

/// 检测框，原图坐标系 [x1, y1, x2, y2)
pub type OcrBox = [u32; 4];

pub struct OnnxOcr {
    /// Session 的创建依赖 Environment，保持它存活
    _env: Environment,
    /// Session::run 需要 &mut self，用 Mutex 串行化同一引擎的推理
    det: Mutex<Session>,
    /// 方向分类器。use_angle_cls=false 或模型缺失时为 None
    cls: Option<Mutex<Session>>,
    rec: Mutex<Session>,
    /// 判定倒置的概率阈值
    cls_threshold: f32,
    /// 识别字符表，不含 blank 与末尾空格
    dict: Vec<String>,
}

impl OnnxOcr {
    /// 从模型目录加载 det/rec，按需加载 cls。调用方负责在 spawn_blocking 里执行
    pub fn load(dir: &Path, use_angle_cls: bool, cls_threshold: f32) -> Result<Self> {
        let det_path = dir.join(DET_MODEL_FILE);
        let rec_path = dir.join(REC_MODEL_FILE);
        for path in [&det_path, &rec_path] {
            if !path.is_file() {
                bail!("OCR 模型文件缺失: {}", path.display());
            }
        }

        let env = ort::init()
            .with_name("avalon-ocr")
            .build()
            .map_err(|e| anyhow!("初始化 ONNX Runtime 失败: {e}"))?;
        let det = build_session(&env, &det_path)?;
        let rec = build_session(&env, &rec_path)?;

        // 方向分类是增强项：配置要了但文件缺失时降级，不让整个 OCR 不可用。
        // 文件存在却加载失败则视为真实错误，直接报出来
        let cls = if use_angle_cls {
            let cls_path = dir.join(CLS_MODEL_FILE);
            if cls_path.is_file() {
                Some(Mutex::new(build_session(&env, &cls_path)?))
            } else {
                log::warn!(
                    target: "vision",
                    "OCR 方向分类模型缺失，倒置文本将不做校正: {}",
                    cls_path.display()
                );
                None
            }
        } else {
            None
        };

        // RapidOCR 导出的 rec 模型把字符表放在 custom metadata 里，优先用它；
        // 其他导出可能没有，回退到同目录的 ppocr_keys_v1.txt
        let dict = match rec.metadata().ok().and_then(|meta| meta.custom("character")) {
            Some(raw) => {
                let list = parse_character_list(&raw);
                if list.is_empty() {
                    bail!("OCR rec 模型内置字符表为空");
                }
                list
            }
            None => {
                let dict_path = dir.join(DICT_FILE);
                if !dict_path.is_file() {
                    bail!(
                        "OCR rec 模型未内置字符表，且缺少字典文件: {}",
                        dict_path.display()
                    );
                }
                load_dict(&dict_path)?
            }
        };

        Ok(Self {
            _env: env,
            det: Mutex::new(det),
            cls,
            rec: Mutex::new(rec),
            cls_threshold,
            dict,
        })
    }

    fn detect(&self, image: &RgbImage) -> Result<Vec<OcrBox>> {
        let (input, det_w, det_h) = det_input(image);
        let tensor = Tensor::from_array((
            vec![1i64, 3, det_h as i64, det_w as i64],
            input,
        ))
        .map_err(|e| anyhow!("构建 det 输入张量失败: {e}"))?;

        let mut session = self
            .det
            .lock()
            .map_err(|_| anyhow!("OCR det 会话锁已中毒"))?;
        let outputs = session
            .run(ort::inputs![tensor])
            .map_err(|e| anyhow!("OCR 文本检测推理失败: {e}"))?;
        let prob = outputs[0]
            .try_extract_array::<f32>()
            .map_err(|e| anyhow!("读取 det 输出失败: {e}"))?;
        // 输出借用 session，转成自有 Vec 后再释放锁
        let prob: Vec<f32> = prob.iter().copied().collect();
        drop(outputs);
        drop(session);

        if prob.len() < det_w as usize * det_h as usize {
            bail!(
                "det 输出尺寸异常: 期望 {} 个像素，实际 {}",
                det_w as usize * det_h as usize,
                prob.len()
            );
        }

        Ok(postprocess_det(
            &prob,
            det_w as usize,
            det_h as usize,
            image.width() as usize,
            image.height() as usize,
        ))
    }

    /// 文本行是否整体倒置（180°）。cls 未启用时永远返回 false
    fn is_upside_down(&self, image: &RgbImage) -> Result<bool> {
        let Some(cls) = &self.cls else {
            return Ok(false);
        };

        let input = cls_input(image);
        let tensor = Tensor::from_array((
            vec![1i64, 3, CLS_HEIGHT as i64, CLS_WIDTH as i64],
            input,
        ))
        .map_err(|e| anyhow!("构建 cls 输入张量失败: {e}"))?;

        let mut session = cls
            .lock()
            .map_err(|_| anyhow!("OCR cls 会话锁已中毒"))?;
        let outputs = session
            .run(ort::inputs![tensor])
            .map_err(|e| anyhow!("OCR 方向分类推理失败: {e}"))?;
        let probs = outputs[0]
            .try_extract_array::<f32>()
            .map_err(|e| anyhow!("读取 cls 输出失败: {e}"))?;
        let shape = probs.shape();
        if shape.len() != 2 || shape[0] != 1 || shape[1] < 2 {
            bail!("cls 输出维度异常: {shape:?}");
        }
        // 类别 0 = 0°，类别 1 = 180°
        let p_flipped = probs[[0, 1]];
        drop(outputs);
        drop(session);

        Ok(p_flipped >= self.cls_threshold)
    }

    /// 返回识别结果与该行是否被判为倒置 —— 整图倒置时行顺序也要反过来
    fn recognize_box(&self, image: &RgbImage, b: OcrBox) -> Result<Option<(OcrLine, bool)>> {
        let [x1, y1, x2, y2] = b;
        let crop = imageops::crop_imm(image, x1, y1, x2 - x1, y2 - y1).to_image();
        // cls 判为倒置时先转正，再走与正向文本相同的识别路径
        let flipped = self.is_upside_down(&crop)?;
        let crop = if flipped {
            imageops::rotate180(&crop)
        } else {
            crop
        };
        let (input, rec_w) = rec_input(&crop)?;
        let tensor = Tensor::from_array((
            vec![1i64, 3, REC_HEIGHT as i64, rec_w as i64],
            input,
        ))
        .map_err(|e| anyhow!("构建 rec 输入张量失败: {e}"))?;

        let mut session = self
            .rec
            .lock()
            .map_err(|_| anyhow!("OCR rec 会话锁已中毒"))?;
        let outputs = session
            .run(ort::inputs![tensor])
            .map_err(|e| anyhow!("OCR 文字识别推理失败: {e}"))?;
        let logits = outputs[0]
            .try_extract_array::<f32>()
            .map_err(|e| anyhow!("读取 rec 输出失败: {e}"))?;
        let shape = logits.shape();
        if shape.len() != 3 || shape[0] != 1 {
            bail!("rec 输出维度异常: {shape:?}");
        }
        let steps = shape[1];
        let classes = shape[2];
        let mut probs = Vec::with_capacity(steps * classes);
        for t in 0..steps {
            for c in 0..classes {
                probs.push(logits[[0, t, c]]);
            }
        }
        drop(outputs);
        drop(session);

        Ok(decode_ctc(&probs, steps, classes, &self.dict).map(|line| (line, flipped)))
    }
}

impl OcrEngine for OnnxOcr {
    fn name(&self) -> &'static str {
        // 启用方向分类后结果可能不同，纳入引擎名以失效旧的 OCR 缓存
        if self.cls.is_some() {
            "ppocr-v4-cls"
        } else {
            "ppocr-v4"
        }
    }

    fn recognize(&self, image: &PreparedImage) -> Result<OcrResult> {
        let decoded = image::load_from_memory(&image.bytes).context("OCR 解码图片失败")?;
        let rgb = decoded.to_rgb8();
        let boxes = self.detect(&rgb)?;

        let mut lines = Vec::new();
        let mut all_flipped = true;
        for b in boxes {
            if let Some((line, flipped)) = self.recognize_box(&rgb, b)? {
                if !line.text.trim().is_empty() {
                    all_flipped &= flipped;
                    lines.push(line);
                }
            }
        }
        // 整图 180° 倒置时，det 仍在原图坐标系里按 y 排序，行顺序是反的。
        // 只有所有行都被判为倒置才反转，避免误伤正置与倒置混排的图片
        let lines = reorder_lines(lines, all_flipped);

        Ok(build_result(lines))
    }
}

fn build_session(env: &Environment, path: &Path) -> Result<Session> {
    Session::builder(env)
        .map_err(|e| anyhow!("创建 ONNX Runtime 会话失败: {e}"))?
        .with_intra_threads(OCR_THREADS)
        .map_err(|e| anyhow!("配置 ONNX 线程数失败: {e}"))?
        .commit_from_file(path)
        .map_err(|e| anyhow!("加载 OCR 模型失败 {}: {e}", path.display()))
}

/// 读取外部字典。保留空行与空格行：索引必须与模型输出严格对应
pub(crate) fn load_dict(path: &Path) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("读取 OCR 字典失败: {}", path.display()))?;
    Ok(parse_character_list(&text))
}

/// 解析一行一个字符的字符表；只去掉末尾换行产生的空项，
/// 中间空行原样保留 —— 少一项就会让后续所有字符整体错位
pub(crate) fn parse_character_list(raw: &str) -> Vec<String> {
    let mut lines: Vec<String> = raw
        .split('\n')
        .map(|line| line.trim_end_matches('\r').to_string())
        .collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

/// det 预处理：等比缩放到长边 ≤ DET_MAX_SIDE，并对齐到 32 的倍数。
/// 返回 NCHW 浮点数据与输入宽高
pub(crate) fn det_input(image: &RgbImage) -> (Vec<f32>, u32, u32) {
    let (src_w, src_h) = image.dimensions();
    let max_side = src_w.max(src_h).max(1);
    let scale = if max_side > DET_MAX_SIDE {
        DET_MAX_SIDE as f32 / max_side as f32
    } else {
        1.0
    };
    let w = align_up((src_w as f32 * scale).round().max(1.0) as u32, DET_ALIGN);
    let h = align_up((src_h as f32 * scale).round().max(1.0) as u32, DET_ALIGN);

    let resized = imageops::resize(image, w, h, FilterType::Triangle);
    let mut out = vec![0f32; 3 * w as usize * h as usize];
    for (x, y, px) in resized.enumerate_pixels() {
        let idx = y as usize * w as usize + x as usize;
        for c in 0..3 {
            out[c * w as usize * h as usize + idx] =
                (px[c] as f32 / 255.0 - DET_MEAN[c]) / DET_STD[c];
        }
    }
    (out, w, h)
}

/// cls 预处理：直接缩放到 48x192，mean/std 均为 0.5。
/// 与 det/rec 不同，这里不保持长宽比 —— cls 模型按固定分辨率训练
pub(crate) fn cls_input(image: &RgbImage) -> Vec<f32> {
    let resized = imageops::resize(image, CLS_WIDTH, CLS_HEIGHT, FilterType::Triangle);
    let plane = CLS_WIDTH as usize * CLS_HEIGHT as usize;
    let mut out = vec![0f32; 3 * plane];
    for (x, y, px) in resized.enumerate_pixels() {
        let idx = y as usize * CLS_WIDTH as usize + x as usize;
        for c in 0..3 {
            out[c * plane + idx] = (px[c] as f32 / 255.0 - 0.5) / 0.5;
        }
    }
    out
}

/// rec 预处理：高度固定 48，宽度按比例缩放并限制在 [16, 320]
pub(crate) fn rec_input(image: &RgbImage) -> Result<(Vec<f32>, u32)> {
    let (src_w, src_h) = image.dimensions();
    if src_w == 0 || src_h == 0 {
        bail!("OCR 文本框为空");
    }
    let target_w = ((REC_HEIGHT as f32 * src_w as f32 / src_h as f32).round() as u32)
        .clamp(REC_MIN_WIDTH, REC_MAX_WIDTH);
    let resized = imageops::resize(image, target_w, REC_HEIGHT, FilterType::Triangle);

    let mut out = vec![0f32; 3 * target_w as usize * REC_HEIGHT as usize];
    let plane = target_w as usize * REC_HEIGHT as usize;
    for (x, y, px) in resized.enumerate_pixels() {
        let idx = y as usize * target_w as usize + x as usize;
        for c in 0..3 {
            out[c * plane + idx] = (px[c] as f32 / 255.0 - 0.5) / 0.5;
        }
    }
    Ok((out, target_w))
}

/// det 概率图后处理：二值化 → 连通域 → 外接矩形 → 分数过滤 → 外扩 → 映射回原图
pub(crate) fn postprocess_det(
    prob: &[f32],
    det_w: usize,
    det_h: usize,
    orig_w: usize,
    orig_h: usize,
) -> Vec<OcrBox> {
    let mut visited = vec![false; det_w * det_h];
    let mut boxes: Vec<(OcrBox, f32)> = Vec::new();
    let mut queue = VecDeque::new();

    for start in 0..det_w * det_h {
        if visited[start] || prob[start] <= DET_BINARY_THRESHOLD {
            continue;
        }
        visited[start] = true;
        queue.clear();
        queue.push_back(start);

        let (mut min_x, mut min_y, mut max_x, mut max_y) = (start % det_w, start / det_w, start % det_w, start / det_w);
        let mut count = 0usize;
        let mut sum = 0f32;

        while let Some(idx) = queue.pop_front() {
            let x = idx % det_w;
            let y = idx / det_w;
            count += 1;
            sum += prob[idx];
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);

            for (nx, ny) in [
                (x.wrapping_sub(1), y),
                (x + 1, y),
                (x, y.wrapping_sub(1)),
                (x, y + 1),
            ] {
                if nx >= det_w || ny >= det_h {
                    continue;
                }
                let n = ny * det_w + nx;
                if visited[n] || prob[n] <= DET_BINARY_THRESHOLD {
                    continue;
                }
                visited[n] = true;
                queue.push_back(n);
            }
        }

        let bw = max_x - min_x + 1;
        let bh = max_y - min_y + 1;
        // 过滤噪点：面积太小或长宽比过于极端（单像素线）
        if count < 4 || bw < 3 || bh < 3 {
            continue;
        }
        let score = sum / count as f32;
        if score < DET_BOX_THRESHOLD {
            continue;
        }

        // DB 的 unclip：按面积/周长估算外扩距离
        let perimeter = 2 * (bw + bh);
        let expand = ((count as f32 * DET_UNCLIP_RATIO) / perimeter as f32).ceil() as usize;
        let x1 = min_x.saturating_sub(expand);
        let y1 = min_y.saturating_sub(expand);
        let x2 = (max_x + 1 + expand).min(det_w);
        let y2 = (max_y + 1 + expand).min(det_h);

        // 映射回原图坐标
        let sx = orig_w as f32 / det_w as f32;
        let sy = orig_h as f32 / det_h as f32;
        let ox1 = ((x1 as f32 * sx).floor() as usize).min(orig_w.saturating_sub(1));
        let oy1 = ((y1 as f32 * sy).floor() as usize).min(orig_h.saturating_sub(1));
        let ox2 = ((x2 as f32 * sx).ceil() as usize).min(orig_w);
        let oy2 = ((y2 as f32 * sy).ceil() as usize).min(orig_h);
        if ox2.saturating_sub(ox1) < 8 || oy2.saturating_sub(oy1) < 8 {
            continue;
        }

        boxes.push(([ox1 as u32, oy1 as u32, ox2 as u32, oy2 as u32], score));
    }

    // 阅读顺序：先按行（y 中心），再按列（x 起点）
    boxes.sort_by(|a, b| {
        let ay = (a.0[1] + a.0[3]) / 2;
        let by = (b.0[1] + b.0[3]) / 2;
        ay.cmp(&by).then(a.0[0].cmp(&b.0[0]))
    });
    boxes.truncate(MAX_BOXES);
    boxes.into_iter().map(|(b, _)| b).collect()
}

/// CTC 解码：逐帧 argmax → 去连续重复 → 去 blank → 映射字典。
/// 返回 None 表示整段没有有效字符
pub(crate) fn decode_ctc(
    probs: &[f32],
    steps: usize,
    classes: usize,
    dict: &[String],
) -> Option<OcrLine> {
    if steps == 0 || classes == 0 || probs.len() < steps * classes {
        return None;
    }

    let mut text = String::new();
    let mut conf_sum = 0f32;
    let mut conf_count = 0usize;
    let mut prev = usize::MAX;

    for t in 0..steps {
        let row = &probs[t * classes..(t + 1) * classes];
        let mut best = 0usize;
        let mut best_p = row[0];
        for (c, &p) in row.iter().enumerate().skip(1) {
            if p > best_p {
                best = c;
                best_p = p;
            }
        }

        // blank = 0；连续相同索引只保留一次
        if best != 0 && best != prev {
            let ch = char_for_index(best, dict);
            if !ch.is_empty() {
                text.push_str(ch);
                conf_sum += best_p;
                conf_count += 1;
            }
        }
        prev = best;
    }

    if text.is_empty() {
        return None;
    }
    let confidence = if conf_count == 0 {
        0.0
    } else {
        conf_sum / conf_count as f32
    };
    Some(OcrLine { text, confidence })
}

/// 模型输出索引 → 字符。约定：0 = blank，1..=n = 字典，n+1 = 空格
fn char_for_index(idx: usize, dict: &[String]) -> &str {
    if idx == 0 {
        ""
    } else if idx <= dict.len() {
        &dict[idx - 1]
    } else if idx == dict.len() + 1 {
        " "
    } else {
        ""
    }
}

/// 整图倒置时逆转行序；部分行倒置或只有一行时保持 det 给出的顺序
fn reorder_lines(mut lines: Vec<OcrLine>, all_flipped: bool) -> Vec<OcrLine> {
    if all_flipped && lines.len() > 1 {
        lines.reverse();
    }
    lines
}

fn build_result(lines: Vec<OcrLine>) -> OcrResult {
    let text = lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let total: usize = lines.iter().map(|l| l.text.chars().count()).sum();
    let confidence = if total == 0 {
        0.0
    } else {
        lines
            .iter()
            .map(|l| l.confidence * l.text.chars().count() as f32)
            .sum::<f32>()
            / total as f32
    };
    OcrResult::new(text, lines, confidence)
}

fn align_up(value: u32, align: u32) -> u32 {
    value.div_ceil(align) * align
}

/// 模型目录下 det/rec 是否齐全。字典不是必需：RapidOCR 导出的模型自带字符表。
/// cls 也不强制：缺失时只是不做 180° 校正。
/// 工具注册前只做文件探测，不加载模型
pub(crate) fn models_present(dir: &Path) -> bool {
    [DET_MODEL_FILE, REC_MODEL_FILE]
        .iter()
        .all(|name| dir.join(name).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prob_map(w: usize, h: usize, rects: &[(usize, usize, usize, usize)]) -> Vec<f32> {
        let mut map = vec![0f32; w * h];
        for &(x1, y1, x2, y2) in rects {
            for y in y1..y2 {
                for x in x1..x2 {
                    map[y * w + x] = 0.9;
                }
            }
        }
        map
    }

    #[test]
    fn det_postprocess_finds_and_orders_boxes() {
        // 两个 40x20 的块，右下那个先写入，验证排序按阅读顺序
        let map = prob_map(200, 100, &[(100, 60, 140, 80), (20, 10, 60, 30)]);
        let boxes = postprocess_det(&map, 200, 100, 200, 100);
        assert_eq!(boxes.len(), 2);
        assert!(boxes[0][1] < boxes[1][1], "上方文本框应排在前面");
        assert!(boxes[0][2] > boxes[0][0] && boxes[0][3] > boxes[0][1]);
    }

    #[test]
    fn det_postprocess_ignores_noise_and_low_scores() {
        // 单像素噪点、以及一整块低分区域都不应产出框
        let mut map = vec![0f32; 100 * 100];
        map[50 * 100 + 50] = 0.9;
        for y in 0..40 {
            for x in 0..40 {
                map[y * 100 + x] = 0.4;
            }
        }
        assert!(postprocess_det(&map, 100, 100, 100, 100).is_empty());
    }

    #[test]
    fn det_postprocess_expands_and_clamps() {
        // 贴着边界的块外扩后不能越界
        let map = prob_map(64, 64, &[(0, 0, 20, 20)]);
        let boxes = postprocess_det(&map, 64, 64, 64, 64);
        assert_eq!(boxes[0][0], 0);
        assert_eq!(boxes[0][1], 0);
        assert!(boxes[0][2] <= 64 && boxes[0][3] <= 64);
    }

    #[test]
    fn det_input_aligns_to_32_and_caps_long_side() {
        let img = RgbImage::new(1000, 500);
        let (data, w, h) = det_input(&img);
        assert_eq!(w % DET_ALIGN, 0);
        assert_eq!(h % DET_ALIGN, 0);
        assert!(w <= DET_MAX_SIDE && h <= DET_MAX_SIDE);
        assert_eq!(data.len(), 3 * w as usize * h as usize);

        // 小图不放大，只对齐
        let small = RgbImage::new(100, 50);
        let (_, w, h) = det_input(&small);
        assert_eq!((w, h), (128, 64));
    }

    #[test]
    fn ctc_decode_dedups_and_maps_dictionary() {
        let dict: Vec<String> = ["你", "好", "世", "界"].iter().map(|s| s.to_string()).collect();
        // 帧序列：你 你 blank 好 好 | 世 blank 界 界
        let ids = [1usize, 1, 0, 2, 2, 3, 0, 4, 4];
        let classes = dict.len() + 2;
        let mut probs = vec![0f32; ids.len() * classes];
        for (t, &id) in ids.iter().enumerate() {
            probs[t * classes + id] = 0.9;
        }
        let line = decode_ctc(&probs, ids.len(), classes, &dict).unwrap();
        assert_eq!(line.text, "你好世界");
        assert!((line.confidence - 0.9).abs() < 1e-6);
    }

    #[test]
    fn ctc_decode_handles_trailing_space() {
        let dict: Vec<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let ids = [1usize, 3]; // a + 空格（n+1）
        let classes = dict.len() + 2;
        let mut probs = vec![0f32; ids.len() * classes];
        for (t, &id) in ids.iter().enumerate() {
            probs[t * classes + id] = 0.8;
        }
        assert_eq!(decode_ctc(&probs, ids.len(), classes, &dict).unwrap().text, "a ");
    }

    #[test]
    fn ctc_decode_returns_none_for_blank_only() {
        let dict: Vec<String> = ["a"].iter().map(|s| s.to_string()).collect();
        let classes = dict.len() + 2;
        let probs = vec![0f32; 3 * classes];
        assert!(decode_ctc(&probs, 3, classes, &dict).is_none());
    }

    #[test]
    fn reorder_lines_reverses_only_when_all_flipped() {
        let line = |text: &str| OcrLine {
            text: text.to_string(),
            confidence: 0.9,
        };
        let lines = || vec![line("A"), line("B"), line("C")];

        let reordered = reorder_lines(lines(), true);
        assert_eq!(
            reordered.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            vec!["C", "B", "A"]
        );

        let kept = reorder_lines(lines(), false);
        assert_eq!(
            kept.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            vec!["A", "B", "C"]
        );

        assert_eq!(reorder_lines(vec![line("A")], true)[0].text, "A");
    }

    #[test]
    fn cls_input_has_fixed_shape_and_normalization() {
        let img = RgbImage::new(300, 40);
        let data = cls_input(&img);
        assert_eq!(data.len(), 3 * CLS_WIDTH as usize * CLS_HEIGHT as usize);
        // 全黑像素归一化后为 -1
        assert!(data.iter().all(|&v| (v + 1.0).abs() < 1e-6));
    }

    #[test]
    fn rec_input_keeps_aspect_and_clamps_width() {
        let wide = RgbImage::new(1000, 20);
        let (_, w) = rec_input(&wide).unwrap();
        assert_eq!(w, REC_MAX_WIDTH);

        let tall = RgbImage::new(5, 100);
        let (_, w) = rec_input(&tall).unwrap();
        assert_eq!(w, REC_MIN_WIDTH);
    }

    #[test]
    fn dict_roundtrip_keeps_blank_line_and_space() {
        let dir = std::env::temp_dir().join(format!("avalon-ocr-dict-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dict.txt");
        std::fs::write(&path, "你\r\n好\r\n \r\n").unwrap();
        let dict = load_dict(&path).unwrap();
        assert_eq!(dict, vec!["你", "好", " "]);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 手工验收：需要 data/models/ocr 下的真实模型与 data/test_data/ocr_e2e.png
    #[test]
    #[ignore = "需要本地模型与测试图，仅用于手工验收"]
    fn e2e_recognizes_chinese_image() {
        let engine = OnnxOcr::load(Path::new("f:/Avalon/data/models/ocr"), true, 0.9).unwrap();
        let bytes = std::fs::read("f:/Avalon/data/test_data/ocr_e2e.png").unwrap();
        let prepared = PreparedImage {
            bytes,
            mime: "image/png",
            width: 900,
            height: 420,
            sha256: "e2e".into(),
            phash: "0".repeat(16),
            profile: crate::vision::ImageProfile {
                width: 900,
                height: 420,
                format: "png".into(),
                has_alpha: false,
                animated: false,
                frames: 1,
                source_bytes: 0,
            },
            thumbnail: crate::vision::Thumbnail {
                bytes: vec![],
                mime: "image/png",
                ext: "png",
            },
        };
        let result = engine.recognize(&prepared).unwrap();
        println!("confidence={:.3}\n{}", result.confidence, result.text);
        assert!(result.text.contains("Avalon"), "未识别到标题: {}", result.text);
        assert!(result.text.contains("连接超时"), "未识别到中文: {}", result.text);
        assert!(result.text.contains("2026"), "未识别到数字: {}", result.text);
    }

    /// 手工验收：整图旋转 180° 后，cls 应逐行纠正方向再识别。
    /// 需要 data/models/ocr 下的真实模型（含 cls.onnx）与 data/test_data/ocr_e2e.png
    #[test]
    #[ignore = "需要本地模型与测试图，仅用于手工验收"]
    fn e2e_recognizes_upside_down_image() {
        let engine = OnnxOcr::load(Path::new("f:/Avalon/data/models/ocr"), true, 0.9).unwrap();
        let bytes = std::fs::read("f:/Avalon/data/test_data/ocr_e2e.png").unwrap();
        let rgb = image::load_from_memory(&bytes).unwrap().to_rgb8();
        let (width, height) = rgb.dimensions();
        let rotated = imageops::rotate180(&rgb);

        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(rotated)
            .write_to(
                &mut std::io::Cursor::new(&mut png),
                image::ImageFormat::Png,
            )
            .unwrap();

        let prepared = PreparedImage {
            bytes: png,
            mime: "image/png",
            width,
            height,
            sha256: "e2e-rotated".into(),
            phash: "0".repeat(16),
            profile: crate::vision::ImageProfile {
                width,
                height,
                format: "png".into(),
                has_alpha: false,
                animated: false,
                frames: 1,
                source_bytes: 0,
            },
            thumbnail: crate::vision::Thumbnail {
                bytes: vec![],
                mime: "image/png",
                ext: "png",
            },
        };
        let result = engine.recognize(&prepared).unwrap();
        println!("confidence={:.3}\n{}", result.confidence, result.text);
        assert!(result.text.contains("Avalon"), "倒置图片未正确校正: {}", result.text);
    }

    #[test]
    fn models_present_requires_det_and_rec() {
        let dir = std::env::temp_dir().join(format!("avalon-ocr-models-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!models_present(&dir));

        std::fs::write(dir.join(DET_MODEL_FILE), b"x").unwrap();
        assert!(!models_present(&dir));

        std::fs::write(dir.join(REC_MODEL_FILE), b"x").unwrap();
        assert!(models_present(&dir));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parse_character_list_keeps_order_and_trailing_space() {
        // 末尾换行不产生空字符；中间的空行（空格字符）必须保留
        let list = parse_character_list("!\r\n你\r\n \r\n好\r\n");
        assert_eq!(list, vec!["!", "你", " ", "好"]);
    }
}
