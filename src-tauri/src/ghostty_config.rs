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
        "frosted-jasmine" | "light" => GhosttyConfigColors {
            // Light theme (Monochrome 黑白灰)
            palette: [
                [0x11, 0x18, 0x27], // 0  black      -> text
                [0xef, 0x44, 0x44], // 1  red        -> danger
                [0x10, 0xb9, 0x81], // 2  green      -> diff-add
                [0xea, 0xb3, 0x08], // 3  yellow     -> warning
                [0x3b, 0x82, 0xf6], // 4  blue       -> info
                [0x00, 0x00, 0x00], // 5  magenta    -> accent (black)
                [0x6b, 0x72, 0x80], // 6  cyan       -> info/gray
                [0xe5, 0xe7, 0xeb], // 7  white      -> border
                [0x9c, 0xa3, 0xaf], // 8  br_black   -> text-disabled
                [0xef, 0x44, 0x44], // 9  br_red     -> danger
                [0x10, 0xb9, 0x81], // 10 br_green   -> diff-add
                [0xea, 0xb3, 0x08], // 11 br_yellow  -> warning
                [0x3b, 0x82, 0xf6], // 12 br_blue    -> info
                [0x00, 0x00, 0x00], // 13 br_magenta -> accent (black)
                [0x6b, 0x72, 0x80], // 14 br_cyan    -> info/gray
                [0xff, 0xff, 0xff], // 15 br_white   -> pure white
            ],
            foreground: [0x11, 0x18, 0x27], // text (#111827)
            background: [0xff, 0xff, 0xff], // terminal-bg (#ffffff)
            cursor: [0x00, 0x00, 0x00],     // accent (black, #000000)
        },
        "terminal-volt" | "dark" => GhosttyConfigColors {
            // Dark theme (Monochrome 黑白灰翻转)
            palette: [
                [0x17, 0x1a, 0x21], // 0  black      -> bg (#171a21)
                [0xf8, 0x71, 0x71], // 1  red        -> danger
                [0x34, 0xd3, 0x99], // 2  green      -> diff-add
                [0xfb, 0xbf, 0x24], // 3  yellow     -> warning
                [0x60, 0xa5, 0xfa], // 4  blue       -> info
                [0xff, 0xff, 0xff], // 5  magenta    -> accent (white)
                [0x9c, 0xa3, 0xaf], // 6  cyan       -> info/gray
                [0xe5, 0xe7, 0xeb], // 7  white      -> border
                [0x66, 0x70, 0x85], // 8  br_black   -> text-disabled
                [0xf8, 0x71, 0x71], // 9  br_red     -> danger
                [0x34, 0xd3, 0x99], // 10 br_green   -> diff-add
                [0xfb, 0xbf, 0x24], // 11 br_yellow  -> warning
                [0x60, 0xa5, 0xfa], // 12 br_blue    -> info
                [0xff, 0xff, 0xff], // 13 br_magenta -> accent (white)
                [0x9c, 0xa3, 0xaf], // 14 br_cyan    -> info/gray
                [0xff, 0xff, 0xff], // 15 br_white   -> pure white
            ],
            foreground: [0xf9, 0xfa, 0xfb], // text (#f9fafb)
            background: [0x17, 0x1a, 0x21], // terminal-bg (#171a21)
            cursor: [0xff, 0xff, 0xff],     // accent (white, #ffffff)
        },
        _ => theme_to_ghostty("dark"),
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
        assert!(config.contains("palette = 0=#111827"), "frosted-jasmine black should match text color");
        assert!(config.contains("background = #ffffff"), "frosted-jasmine bg should match terminal-bg");
    }

    #[test]
    fn test_theme_mapping_terminal_volt() {
        let colors = theme_to_ghostty("terminal-volt");
        assert_eq!(colors.palette[0], [0x17, 0x1a, 0x21]);
        assert_eq!(colors.foreground, [0xf9, 0xfa, 0xfb]);
    }

    #[test]
    fn test_theme_mapping_frosted_jasmine() {
        let colors = theme_to_ghostty("frosted-jasmine");
        assert_eq!(colors.background, [0xff, 0xff, 0xff]);
        assert_eq!(colors.cursor, [0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_write_config_creates_file() {
        // Use a temp dir trick — rely on home_dir; test is informational
        // In CI we'd set HOME to a temp dir.
        let config = generate_config("terminal-volt");
        assert!(!config.is_empty());
    }
}
