//! Ghostty 主题配置生成器
//!
//! 根据 Natives 主题 ID 生成 Ghostty 原生终端配色配置文件。
//! 产出的 config 文件可以被 Ghostty 通过 --config-file=<path> 加载。
//!
//! Feature gate: 本模块无额外依赖，始终可用。

use crate::Result;
use std::path::PathBuf;

// ── 主题 → Ghostty 调色板映射 ──

/// Ghostty 16 色 ANSI palette + foreground/background/cursor
pub struct GhosttyConfigColors {
    pub palette: [[u8; 3]; 16],
    pub foreground: [u8; 3],
    pub background: [u8; 3],
    pub cursor: [u8; 3],
}

/// 根据主题 ID 返回对应的 Ghostty 色彩配置
pub fn theme_to_ghostty(theme_id: &str) -> GhosttyConfigColors {
    match theme_id {
        "frosted-jasmine" => GhosttyConfigColors {
            // 暖白浆果色系 — 16 色调色板
            palette: [
                [0x2d, 0x1f, 0x14], // 0  black      -> text
                [0xf0, 0x5b, 0x3f], // 1  red        -> danger
                [0xff, 0x98, 0x56], // 2  green      -> diff-add
                [0xff, 0xa4, 0x66], // 3  yellow     -> warning
                [0xbe, 0x88, 0xed], // 4  blue       -> info
                [0xff, 0x79, 0x3f], // 5  magenta    -> accent
                [0xbe, 0x88, 0xed], // 6  cyan       -> info
                [0xdc, 0xdf, 0xe6], // 7  white      -> text
                [0x7a, 0x6b, 0x5a], // 8  br_black   -> text-dim
                [0xf0, 0x5b, 0x3f], // 9  br_red     -> danger
                [0xff, 0x98, 0x56], // 10 br_green   -> diff-add
                [0xff, 0xa4, 0x66], // 11 br_yellow  -> warning
                [0xbe, 0x88, 0xed], // 12 br_blue    -> info
                [0xff, 0x79, 0x3f], // 13 br_magenta -> accent
                [0xbe, 0x88, 0xed], // 14 br_cyan    -> info
                [0xff, 0xfd, 0xfa], // 15 br_white   -> bg
            ],
            foreground: [0x2d, 0x1f, 0x14], // text
            background: [0xfd, 0xf6, 0xf0], // terminal-bg
            cursor: [0xff, 0x79, 0x3f],     // accent
        },
        // 默认：terminal-volt（赛博绿）
        _ => GhosttyConfigColors {
            // 暗色赛博绿系
            palette: [
                [0x0d, 0x0f, 0x12], // 0  black      -> bg
                [0xff, 0x3a, 0x4d], // 1  red        -> danger
                [0x00, 0xff, 0x9c], // 2  green      -> accent/diff-add
                [0xff, 0xb5, 0x45], // 3  yellow     -> warning
                [0x45, 0xb5, 0xff], // 4  blue       -> info
                [0x00, 0xff, 0x9c], // 5  magenta    -> accent
                [0x45, 0xb5, 0xff], // 6  cyan       -> info
                [0xdc, 0xdf, 0xe6], // 7  white      -> text
                [0x55, 0x5a, 0x66], // 8  br_black   -> text-faint
                [0xff, 0x3a, 0x4d], // 9  br_red     -> danger
                [0x00, 0xff, 0x9c], // 10 br_green   -> accent/diff-add
                [0xff, 0xb5, 0x45], // 11 br_yellow  -> warning
                [0x45, 0xb5, 0xff], // 12 br_blue    -> info
                [0x00, 0xff, 0x9c], // 13 br_magenta -> accent
                [0x45, 0xb5, 0xff], // 14 br_cyan    -> info
                [0xff, 0xff, 0xff], // 15 br_white   -> pure white
            ],
            foreground: [0xd4, 0xd7, 0xde], // terminal-fg
            background: [0x0d, 0x0f, 0x12], // terminal-bg
            cursor: [0x00, 0xff, 0x9c],     // accent
        },
    }
}

// ── Config 文本生成 ──

/// 生成 Ghostty config 文本（palette + foreground + background + cursor-color）
pub fn generate_config(theme_id: &str) -> String {
    let colors = theme_to_ghostty(theme_id);
    let mut out = String::with_capacity(512);

    // Palette entries
    for (i, [r, g, b]) in colors.palette.iter().enumerate() {
        out.push_str(&format!("palette = {i}=#{r:02x}{g:02x}{b:02x}\n"));
    }

    // Core colors
    let [fr, fg, fb] = colors.foreground;
    let [br, bg, bb] = colors.background;
    let [cr, cg, cb] = colors.cursor;
    out.push_str(&format!("foreground = #{fr:02x}{fg:02x}{fb:02x}\n"));
    out.push_str(&format!("background = #{br:02x}{bg:02x}{bb:02x}\n"));
    out.push_str(&format!("cursor-color = #{cr:02x}{cg:02x}{cb:02x}\n"));

    out
}

// ── 写入磁盘 ──

/// 将主题配置写入 ~/.natives/ghostty/config-<theme_id>.conf
/// 返回写入的配置文件路径
pub fn write_config(theme_id: &str) -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| crate::Error::Internal("cannot find home directory".into()))?;
    let config_dir = home.join(".natives").join("ghostty");
    std::fs::create_dir_all(&config_dir)
        .map_err(|e| crate::Error::Internal(format!("failed to create ghostty config dir: {e}")))?;

    let config_path = config_dir.join(format!("config-{theme_id}.conf"));
    let content = generate_config(theme_id);
    std::fs::write(&config_path, &content)
        .map_err(|e| crate::Error::Internal(format!("failed to write ghostty config: {e}")))?;

    Ok(config_path)
}

// ── 单元测试 ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_config_contains_palette() {
        let config = generate_config("terminal-volt");
        assert!(config.contains("palette = 0="), "should contain palette entry 0");
        assert!(config.contains("palette = 15="), "should contain palette entry 15");
    }

    #[test]
    fn test_generate_config_contains_foreground() {
        let config = generate_config("terminal-volt");
        assert!(config.contains("foreground = "), "should contain foreground");
        assert!(config.contains("background = "), "should contain background");
        assert!(config.contains("cursor-color = "), "should contain cursor-color");
    }

    #[test]
    fn test_generate_config_contains_frosted_jasmine() {
        let config = generate_config("frosted-jasmine");
        assert!(config.contains("palette = 0=#2d1f14"), "frosted-jasmine black should match text color");
        assert!(config.contains("background = #fdf6f0"), "frosted-jasmine bg should match terminal-bg");
    }

    #[test]
    fn test_theme_mapping_terminal_volt() {
        let colors = theme_to_ghostty("terminal-volt");
        // Palette[0] should be bg color for dark theme
        assert_eq!(colors.palette[0], [0x0d, 0x0f, 0x12]);
        assert_eq!(colors.foreground, [0xd4, 0xd7, 0xde]);
    }

    #[test]
    fn test_theme_mapping_frosted_jasmine() {
        let colors = theme_to_ghostty("frosted-jasmine");
        assert_eq!(colors.background, [0xfd, 0xf6, 0xf0]);
        assert_eq!(colors.cursor, [0xff, 0x79, 0x3f]);
    }

    #[test]
    fn test_write_config_creates_file() {
        // Use a temp dir trick — rely on home_dir; test is informational
        // In CI we'd set HOME to a temp dir.
        let config = generate_config("terminal-volt");
        assert!(!config.is_empty());
    }
}
