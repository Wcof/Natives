# ADR-0043：Files PDF 使用 Chrome 内置阅读器

- 状态：Accepted（2026-10-20，用户明确授权限定范围）
- 范围：ADR-0042 两入口的 Files PDF 预览；不改变业务模块和 HTML 沙箱。

## 依据

真实扩展和无扩展 Chrome 返回有效 PDF 字节，但带 iframe sandbox 的阅读器显示错误页。隔离诊断仅移除 PDF iframe sandbox 后可以加载 Chrome 阅读器。证据见双入口施工包 `evidence/20261018-full-acceptance/pdf-preview-repair.md`。

## 决策

1. 仅用户选择且经 Host 路径、类型、尺寸与取消校验的 PDF，按固定 `application/pdf` MIME 和 `%PDF-` 校验构造 Blob。PDF iframe 不设 sandbox，内容解析与活动内容隔离由 Chrome 内置阅读器承担；不声称仍有额外 iframe 沙箱。
2. 扩展 CSP 仅在 `frame-src` 增加 `blob:`；`script-src 'self'`、`object-src 'none'` 和 loopback 范围不变。不允许任意外部文档 URL、业务 JS 或凭据进入 PDF。
3. 页码、实际总页数、翻页、搜索和缩放使用内置阅读器的工具栏。删除文本正则推算页数及与原生阅读器不同步的重复控件；压缩对象中的页数不能由文本正则推断。
   初始宽度使用 `view=FitH`；原 `zoom=page-width` 不是有效数字，Chrome 会忽略它。依据为 [Chromium 参数解析器](https://github.com/chromium/chromium/blob/main/chrome/browser/resources/pdf/open_pdf_params_parser.ts) 的 `parseZoomParam_` 与 `parseViewParam_`，并须用当前 Chrome 实测适宽行为。
4. 浏览器报告 PDF 阅读器不可用时显示现有失败、重试及系统打开操作。iframe error 同样显示失败，不将外壳 load 事件作为内容成功证据。预览切换／关闭继续撤销 Object URL，取消尚未结束的 Host 请求，拒绝迟到结果。
5. HTML 只读预览仍为已清理 srcdoc 加 `allow-same-origin`（无脚本）；App iframe 仍精确 `allow-scripts allow-forms`。不得复用 PDF 例外放宽其他内容。

## 验证

两入口实际导入单页／多页及压缩页树 PDF，核对阅读器内容、真实页数、翻页、搜索、缩放及关闭释放。校验无阅读器、坏 magic、取消／迟到、Host 超限拒绝，复核 CSP 与其他 iframe 沙箱。保留首次失败和修复后截图；源码门禁不能代替阅读器内容验证。
