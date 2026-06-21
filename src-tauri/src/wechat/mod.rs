//! WeChat ClawBot — 微信远程遥控本机 Claude Code / Codex
//!
//! 三段式架构:
//! - `ilink`: 腾讯官方 iLink HTTP JSON 协议客户端
//! - `bridge`: 核心编排（扫码登录 / 消息收发 / 状态机）
//! - `driver`: 本机 CLI 驱动（claude / codex 无头模式）
//!
//! Reference: fanbox/electron/wechat/ (bridge.js 467L + ilink.js 213L + driver.js 187L)

pub mod ilink;
pub mod bridge;
pub mod driver;
