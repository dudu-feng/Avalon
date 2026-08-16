// 本地 bge 模型的 candle 推理实现
//
// 加载 bge-small-zh-v1.5（BertModel），CLS token pooling + L2 归一化，
// 输出与 Python sentence-transformers 对齐的向量。
// 处理链：tokenize → BERT forward → [CLS] last_hidden_state → L2 归一化。

#![allow(dead_code)] // 供 mod.rs 引用，无外部调用方，接入后移除

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config};
use tokenizers::Tokenizer;

use super::Embedder;

/// BGE 检索指令前缀（不对称检索：query 带、doc 不带）
const QUERY_INSTRUCTION: &str = "为这个句子生成表示以用于检索相关文章：";
/// bge 最大输入长度（max_position_embeddings），超长截断
const MAX_TOKENS: usize = 512;

/// 本地 bge embedding 模型（重资源，进程内只应加载一份，通过 Arc 共享）
pub struct LocalEmbedder {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
    dim: usize,
}

impl LocalEmbedder {
    /// 加载本地模型：config.json + tokenizer.json + model.safetensors（float32 原样精度）
    pub fn load(model_path: &Path, device: &str) -> Result<Self> {
        let config: Config = serde_json::from_str(
            &std::fs::read_to_string(model_path.join("config.json"))
                .with_context(|| format!("读取 config.json 失败: {}", model_path.display()))?,
        )
        .context("解析 BERT config.json 失败")?;
        let dim = config.hidden_size;

        let tokenizer = Tokenizer::from_file(model_path.join("tokenizer.json"))
            .map_err(|e| anyhow!("加载 tokenizer.json 失败: {e}"))?;

        let device = parse_device(device)?;

        // 全量加载权重（规避 Windows mmap 坑，见 05 文档 5.8）
        let tensors = candle_core::safetensors::load(model_path.join("model.safetensors"), &device)
            .with_context(|| format!("加载 model.safetensors 失败: {}", model_path.display()))?;
        let vb = VarBuilder::from_tensors(tensors, DType::F32, &device);
        let model = BertModel::load(vb, &config).context("构建 BERT 模型失败")?;

        Ok(Self {
            model,
            tokenizer,
            device,
            dim,
        })
    }

    fn encode(&self, text: &str) -> Result<Vec<f32>> {
        // 1. 分词（add_special_tokens = true，自动加 [CLS]/[SEP]）
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| anyhow!("分词失败: {e}"))?;
        let ids = encoding.get_ids();
        // 超长截断（bge max_position_embeddings = 512）
        let ids = if ids.len() > MAX_TOKENS {
            &ids[..MAX_TOKENS]
        } else {
            ids
        };

        // 2. 张量化 [1, seq_len]；token_type_ids 全 0（等价 HF 默认不传 segment）
        let input_ids = Tensor::new(ids, &self.device)?.unsqueeze(0)?;
        let token_type_ids = input_ids.zeros_like()?;

        // 3. forward → last_hidden_state [1, seq_len, hidden]
        let hidden = self.model.forward(&input_ids, &token_type_ids, None)?;

        // 4. CLS pooling：取第 0 个 token（[CLS]）→ [hidden]
        let cls = hidden.get(0)?.get(0)?;

        // 5. L2 归一化（对应 SentenceTransformer 的 Normalize 模块）
        let normalized = cls.broadcast_div(&cls.norm()?)?;

        Ok(normalized.to_vec1()?)
    }
}

impl Embedder for LocalEmbedder {
    fn dim(&self) -> usize {
        self.dim
    }

    fn doc_embedding(&self, text: &str) -> Result<Vec<f32>> {
        self.encode(text.trim())
    }

    fn query_embedding(&self, text: &str) -> Result<Vec<f32>> {
        self.encode(&format!("{QUERY_INSTRUCTION}{}", text.trim()))
    }

    fn batch_doc_embedding(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        texts
            .iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .map(|t| self.encode(t))
            .collect()
    }
}

/// 解析设备字符串：本期只认 cpu（cuda 需 candle-core/cuda feature + 本机 CUDA，后置）
fn parse_device(device: &str) -> Result<Device> {
    match device {
        "cpu" => Ok(Device::Cpu),
        other => Err(anyhow!("不支持的设备: {other}（本期仅支持 cpu）")),
    }
}
