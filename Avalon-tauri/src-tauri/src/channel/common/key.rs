// 渠道会话 key 的安全化
//
// channel 名会直接当作 data/memory/session/current/<key>.json 的文件名，
// 外部渠道给的 openid / group_id 不能原样落进去 —— 其中可能带 /、:、空格等
// 在 Windows 上非法或会被解释成路径分隔符的字符。

/// 只保留 ASCII 字母数字、下划线与连字符，其余统一替换成下划线。
///
/// 不做去重或哈希：openid 本身足够长且稳定，替换后的碰撞概率可以忽略；
/// 保持可读性比追求绝对唯一更重要，排查问题时能一眼看出是哪个群/谁。
pub fn sanitize_channel(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_safe_characters() {
        assert_eq!(
            sanitize_channel("qq_group_ABC-123_xyz"),
            "qq_group_ABC-123_xyz"
        );
    }

    #[test]
    fn replaces_path_and_separator_characters() {
        assert_eq!(sanitize_channel("a/b:c d"), "a_b_c_d");
        assert_eq!(sanitize_channel("中文"), "__");
    }
}
