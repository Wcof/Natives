# Blueprint: Term_WechatTranscript (Repaired — Feature-Specific)

## 1. Logic Deconstruction & Reference Analysis

### Experience
微信对话内容读取，openclaw sessions list筛weixin channel到读~/.openclaw/agents/main/sessions/sid.jsonl到解析JSONL过滤user+assistant可读文本跳过system/tool噪声到渲染对话气泡。

### Reference Code Analysis
- **Target File:** `src-tauri/src/terminal/wechattranscript.rs`
- **Referenced Functions:** —
- **Optimized Implementation Logic（Natives Rust 优化版）:**
  基于 fanbox.md Ground Truth 描述，该特征的核心逻辑涉及 `—`。
  Natives 重构为 Rust + Tauri，采用异步非阻塞设计：
  - 所有 I/O 通过 `tokio::fs` 异步执行，不阻塞主线程
  - 状态变更通过 Tauri Event 推送，前端响应式更新
  - 类型安全：Rust enum/struct 编译期消除动态类型错误
  - IPC 安全：Tauri command whitelist，无 HTTP localhost 攻击面

### State Machine
```
Term_WechatTranscript:  init → loading → (rendering | previewing) → (success | error)
```
- `init`: 初始化
- `loading`: 加载数据（文件读取 / 缩略图生成）
- `rendering` / `previewing`: 渲染/预览中
- `success`: 渲染完成
- `error`: 渲染失败（降级提示）

### Algorithm
```
// Term_WechatTranscript — 预览/渲染（流式加载 + 降级）
async fn render(path: &str, opts: RenderOpts) -> Result<RenderedOutput> {
    // 1. 类型判断 + 预处理
    let kind = detect_file_kind(path);
    match kind {
        FileKind::Image => {
            // 图片缩略图: 用 image crate 解码
            let img = image::open(path)?;
            let thumb = img.thumbnail(opts.width, opts.height);
            let mut buf = Vec::new();
            thumb.write_to(&mut buf, ImageFormat::Jpeg)?;
            Ok(RenderedOutput::Image(base64(&buf)))
        }
        FileKind::Html => {
            // HTML 预览: sandbox iframe + 路径镜像
            let html = tokio::fs::read_to_string(path).await?;
            let fixed = rewrite_local_paths(&html, path);
            Ok(RenderedOutput::Html { content: fixed, sandbox: true })
        }
        FileKind::Text if opts.size > 2 * 1024 * 1024 => {
            // 大文件: 读前 256KB + UTF-8 边界回退
            let head = read_head_utf8safe(path, 256 * 1024).await?;
            Ok(RenderedOutput::TextTruncated { content: head, truncated: true })
        }
        // ... 其他类型
    }
}
```

## 2. Natives Architecture

### Module Path
| 层 | 路径 |
|---|------|
| Rust 后端 | `src-tauri/src/terminal/wechattranscript.rs` |
| 前端组件 | `src/components/terminal/` |

### Data Model
```typescript
// Term_WechatTranscript — 终端数据模型
interface Term_WechatTranscriptParams {
  id: string;
  // 特征专属参数
}
interface Term_WechatTranscriptResult {
  success: boolean;
  id: string;
  error?: string;
}
```

### APIs
```rust
#[tauri::command]
async fn wechattranscript(params: ...) -> Result<Term_WechatTranscriptResult>
```

### Performance & Security Design
| 维度 | 方案 |
|------|------|
| **异步** | 所有 I/O 通过 tokio async 执行，主线程零阻塞 |
| **内存** | 内存边界控制（大文件截断 / LRU 缓存上限 / 流式处理） |
| **安全** | Tauri IPC whitelist + 路径校验 / sandbox iframe / validName |
| **跨平台** | `#[cfg(target_os)]` 条件编译处理平台差异 |

## 3. Risk Analysis

### Risks
1. **边界条件**: 极端路径/空结果/权限不足

### Mitigation & Runtime Guardrail
- 输入校验前置，拒绝非法路径/空字节/目录穿越
- 所有异步操作超时控制（`tokio::time::timeout`），防死锁
- 单元测试 + 集成测试覆盖边界条件（空目录 / 权限不足 / 符号链接循环 / 并发写入）
- 日志分级输出，关键路径有 error / warn / info 三级追踪
