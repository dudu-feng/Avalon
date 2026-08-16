// LLM 返回 JSON 的健壮解析（移植自 Python parse_llm_json）
//
// 模型常返回 markdown 代码块包裹的 JSON，或混有自然语言前后缀，
// 这里按四层降级依次尝试，全部失败则返回 Err。

use anyhow::{anyhow, Result};
use serde::de::DeserializeOwned;

/// 剥离 markdown 代码块标记（```` ```json ... ``` ````）
fn strip_markdown_fences(text: &str) -> String {
    let trimmed = text.trim();

    // 完整代码块：```json\n...\n``` 或 ```...```
    if let Some(rest) = trimmed.strip_prefix("```") {
        let rest = rest.trim_start();
        let rest = rest.strip_prefix("json").unwrap_or(rest);
        let rest = rest.trim_start_matches(|c: char| c == '\n' || c == '\r' || c == ' ');
        // 找结尾 ```
        if let Some(idx) = rest.find("```") {
            return rest[..idx].trim().to_string();
        }
        // 只有开头标记，无结尾
        return rest.trim().to_string();
    }

    // 只有结尾标记 ```
    if trimmed.ends_with("```") {
        return trimmed[..trimmed.len() - 3].trim().to_string();
    }

    text.to_string()
}

/// 从混有自然语言前缀/后缀的文本中提取第一个完整的 JSON 对象。
///
/// 从第一个 `{` 开始按花括号配平（跳过字符串字面量），返回首个
/// 平衡的 `{...}` 片段；找不到则返回 None。
fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;

    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    let mut end_byte = None;

    for (offset, ch) in text[start..].char_indices() {
        if in_str {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_str = false;
            }
        } else if ch == '"' {
            in_str = true;
        } else if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth -= 1;
            if depth == 0 {
                end_byte = Some(offset + ch.len_utf8());
                break;
            }
        }
    }

    end_byte.map(|e| &text[start..start + e])
}

/// 四层降级解析：直接 → 剥 markdown → 提取首个 JSON 对象 → Err
pub fn parse_llm_json<T: DeserializeOwned>(content: &str) -> Result<T> {
    let content = content.trim();
    if content.is_empty() {
        return Err(anyhow!("LLM 返回内容为空"));
    }

    // ① 直接解析
    if let Ok(v) = serde_json::from_str::<T>(content) {
        return Ok(v);
    }

    // ② 剥离 markdown 代码块后重试
    let stripped = strip_markdown_fences(content);
    if stripped != content {
        if let Ok(v) = serde_json::from_str::<T>(&stripped) {
            return Ok(v);
        }
    }

    // ③ 从混合文本中提取首个完整 JSON 对象后重试
    if let Some(extracted) = extract_json_object(content) {
        if let Ok(v) = serde_json::from_str::<T>(extracted) {
            return Ok(v);
        }
    }

    // ④ 无法解析
    Err(anyhow!("无法解析 LLM 返回的 JSON"))
}
