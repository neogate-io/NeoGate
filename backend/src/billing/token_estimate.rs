//! 启发式 token 估算器，移植自 new-api 的 token_estimator.go。
//!
//! 用途：当上游不支持 Anthropic 的 `/v1/messages/count_tokens` 端点时（例如
//! new-api 实例只实现了 `/v1/messages`），本地估算请求的输入 token 数并返回一个
//! 合法的 count_tokens 响应，避免把上游的 404 透传给 Claude Code，从而恢复其
//! 上下文占比追踪与自动压缩。
//!
//! 估算器按字符类别加权累加，针对 Claude 单独调过权重表（见 `CLAUDE`）。它不追求
//! 与官方 tokenizer 完全一致，但对中英文混排、代码、URL 的偏差远小于
//! `body.len() / 4` 这类粗估。

use serde_json::Value;

/// 不同字符类别的计费权重。
struct Multipliers {
    /// 英文单词（每词）
    word: f64,
    /// 数字（每连续数字串）
    number: f64,
    /// 中日韩字符（每字）
    cjk: f64,
    /// 普通标点符号（每个）
    symbol: f64,
    /// 数学符号（∑ ∫ ∂ √ 等，每个）
    math_symbol: f64,
    /// URL 分隔符（/ : ? & = # % 等，tokenizer 优化较好）
    url_delim: f64,
    /// @ 符号（会导致单词切分，消耗较高）
    at_sign: f64,
    /// Emoji 表情（每个）
    emoji: f64,
    /// 换行符/制表符（每个）
    newline: f64,
    /// 空格（每个）
    space: f64,
}

/// Claude 系列模型的权重表，与 new-api `multipliersMap[Claude]` 保持一致。
const CLAUDE: Multipliers = Multipliers {
    word: 1.13,
    number: 1.63,
    cjk: 1.21,
    symbol: 0.4,
    math_symbol: 4.52,
    url_delim: 1.26,
    at_sign: 2.82,
    emoji: 2.6,
    newline: 0.89,
    space: 0.39,
};

#[derive(Clone, Copy, PartialEq)]
enum WordType {
    None,
    Latin,
    Number,
}

/// 用 Claude 权重表估算一段文本的 token 数。移植自 new-api 的 `EstimateToken`。
pub fn estimate_claude_text_tokens(text: &str) -> i64 {
    if text.is_empty() {
        return 0;
    }
    let m = &CLAUDE;
    let mut count = 0.0f64;
    let mut current = WordType::None;

    for r in text.chars() {
        // 1. 空格与换行
        if r.is_whitespace() {
            current = WordType::None;
            if r == '\n' || r == '\t' {
                count += m.newline;
            } else {
                count += m.space;
            }
            continue;
        }
        // 2. CJK：按字符计
        if is_cjk(r) {
            current = WordType::None;
            count += m.cjk;
            continue;
        }
        // 3. Emoji
        if is_emoji(r) {
            current = WordType::None;
            count += m.emoji;
            continue;
        }
        // 4. 拉丁字母/数字（英文单词）
        if r.is_alphanumeric() {
            let new_type = if r.is_numeric() {
                WordType::Number
            } else {
                WordType::Latin
            };
            // 进入新单词，或字母<->数字切换时才计一次
            if current == WordType::None || current != new_type {
                if new_type == WordType::Number {
                    count += m.number;
                } else {
                    count += m.word;
                }
                current = new_type;
            }
            continue;
        }
        // 5. 标点/特殊字符
        current = WordType::None;
        if is_math_symbol(r) {
            count += m.math_symbol;
        } else if r == '@' {
            count += m.at_sign;
        } else if is_url_delim(r) {
            count += m.url_delim;
        } else {
            count += m.symbol;
        }
    }

    count.ceil() as i64
}

fn is_cjk(r: char) -> bool {
    let c = r as u32;
    // 中文（含扩展 A/B 常用区由 is_han 覆盖不到，这里保持与 new-api 一致的主区间）
    matches!(c, 0x4E00..=0x9FFF | 0x3400..=0x4DBF)
        || matches!(c, 0x3040..=0x30FF) // 日文
        || matches!(c, 0xAC00..=0xD7A3) // 韩文
}

fn is_emoji(r: char) -> bool {
    let c = r as u32;
    // 0x1F600..=0x1F64F 与 0x1F900..=0x1F9FF 已被 0x1F300..=0x1F9FF 覆盖，
    // 故不重复列出（new-api 的 Go 版是冗余枚举的）。
    matches!(c,
        0x1F300..=0x1F9FF
        | 0x2600..=0x26FF
        | 0x2700..=0x27BF
        | 0x1FA00..=0x1FAFF
    )
}

fn is_math_symbol(r: char) -> bool {
    const MATH_SYMBOLS: &str =
        "∑∫∂√∞≤≥≠≈±×÷∈∉∋∌⊂⊃⊆⊇∪∩∧∨¬∀∃∄∅∆∇∝∟∠∡∢°′″‴⁺⁻⁼⁽⁾ⁿ₀₁₂₃₄₅₆₇₈₉₊₋₌₍₎²³¹⁴⁵⁶⁷⁸⁹⁰";
    if MATH_SYMBOLS.contains(r) {
        return true;
    }
    let c = r as u32;
    matches!(c, 0x2200..=0x22FF | 0x2A00..=0x2AFF | 0x1D400..=0x1D7FF)
}

fn is_url_delim(r: char) -> bool {
    matches!(r, '/' | ':' | '?' | '&' | '=' | ';' | '#' | '%')
}

/// 从 Anthropic count_tokens 请求体中抽取所有可计数文本，估算输入 token 数。
///
/// 覆盖 `system`（字符串或 content-block 数组）与 `messages[].content`
/// （字符串或 content-block 数组，逐块累加其中的 `text` 字段）。非文本块
/// （image、tool_result 的二进制等）不计入——与 new-api 对 Claude 的处理一致，
/// 估算值用于上下文占比判断而非计费。
pub fn estimate_anthropic_input_tokens(body: &[u8]) -> i64 {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        // body 不是合法 JSON：退回到字节粗估，避免返回 0 误导 Claude Code。
        return super::estimate_input_tokens(body);
    };

    let mut text = String::new();

    if let Some(system) = value.get("system") {
        collect_text(system, &mut text);
    }
    if let Some(messages) = value.get("messages").and_then(Value::as_array) {
        for message in messages {
            if let Some(content) = message.get("content") {
                collect_text(content, &mut text);
            }
        }
    }
    // tools 定义（名称/描述/schema）也占用输入 token，一并计入。
    if let Some(tools) = value.get("tools") {
        collect_text(tools, &mut text);
    }

    estimate_claude_text_tokens(&text)
}

/// 递归收集 JSON 中的文本：字符串直接取；content-block 数组取每块的 `text`；
/// 对象兜底取其 `text` 字段。块之间以换行分隔，避免相邻单词被错误粘连。
fn collect_text(value: &Value, out: &mut String) {
    match value {
        Value::String(s) => {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(s);
        }
        Value::Array(items) => {
            for item in items {
                collect_text(item, out);
            }
        }
        Value::Object(map) => {
            // content-block：{"type":"text","text":"..."} 或工具定义里的文本字段。
            if let Some(Value::String(s)) = map.get("text") {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(s);
            }
            // 工具定义常见字段：name / description。
            for key in ["name", "description"] {
                if let Some(Value::String(s)) = map.get(key) {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(s);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_text_is_zero() {
        assert_eq!(estimate_claude_text_tokens(""), 0);
    }

    #[test]
    fn english_words_counted_per_word() {
        // "hello world" -> 两个单词 * 1.13 + 一个空格 * 0.39 = 2.65 -> ceil 3
        assert_eq!(estimate_claude_text_tokens("hello world"), 3);
    }

    #[test]
    fn cjk_counted_per_char() {
        // 4 个汉字 * 1.21 = 4.84 -> ceil 5
        assert_eq!(estimate_claude_text_tokens("你好世界"), 5);
    }

    #[test]
    fn extracts_system_string_and_message_blocks() {
        let body = json!({
            "model": "claude-opus-4-8",
            "system": "You are a helpful assistant.",
            "messages": [
                {"role": "user", "content": "hello world"},
                {"role": "assistant", "content": [
                    {"type": "text", "text": "hi there"}
                ]}
            ]
        });
        let tokens = estimate_anthropic_input_tokens(body.to_string().as_bytes());
        assert!(tokens > 0, "expected positive estimate, got {tokens}");
    }

    #[test]
    fn non_text_blocks_ignored_but_text_blocks_counted() {
        let with_image = json!({
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "describe this"},
                {"type": "image", "source": {"type": "base64", "data": "AAAABBBBCCCC"}}
            ]}]
        });
        let text_only = json!({
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "describe this"}
            ]}]
        });
        // 图片块不计入文本，两者估算应一致（image 的 base64 不进 collect_text）。
        assert_eq!(
            estimate_anthropic_input_tokens(with_image.to_string().as_bytes()),
            estimate_anthropic_input_tokens(text_only.to_string().as_bytes())
        );
    }

    #[test]
    fn invalid_json_falls_back_to_byte_estimate() {
        let garbage = b"not json at all";
        assert_eq!(
            estimate_anthropic_input_tokens(garbage),
            super::super::estimate_input_tokens(garbage)
        );
    }
}
