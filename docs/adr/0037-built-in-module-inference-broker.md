# ADR-0037 · 内置模块受授权 AI 推理与外链代理

- 日期：2026-09-30
- 状态：接受（用户明确要求完成投资首页及 AI 能力）
- 依赖：ADR-0020、ADR-0031、ADR-0033

## 决策

超时与流式反馈语义按 [ADR-0040](0040-bounded-streaming-module-inference.md) 的限定修订执行，其余边界继续有效。

应用 iframe 保持 `allow-scripts allow-forms`。通用 Shell 按已核验产品清单中的 `ai.inference.request` 能力代理 `options/start/status/cancel`，不解析投资业务。每条请求同时核对 frame source、会话 generation、challenge、requestId、长度与方法白名单。每个 Shell 至多一项推理任务；页面回收取消任务并释放共享 Model Host 客户端。

Model Host 继续唯一拥有凭据与模型配置，通过已有 CLIProxyAPI 的受鉴权 loopback 推理端点调用用户已启用且路由唯一的模型。模块不能指定 URL、headers、密钥、系统路径或管理方法。只读 8766 端口不承载推理。任务开始立即返回编号，状态查询限于随机 owner；30 秒超时、输出长度和任务数量有界。任务内容不广播，不持久化到 Host 日志。

投资模块在本地生成版本绑定的证据包，默认只含比例、规则类型、数据质量与用户勾选的摘录；最多五个摘录、总计 12000 字符。用户预览后主动发送；金额和账户信息只有显式选择披露才加入。模型结果属于解释，不能更改账本、自动下单或替代确定性规则结果。

外链由同一 Shell 的 `system.link.open` 白名单能力在主动点击后处理，仅接受无凭据的 HTTP(S) URL；用 Chrome 新标签打开，不放宽 iframe sandbox。

## 验证

覆盖未声明能力、错误来源／generation／challenge、超限、并发、取消、断连、迟到结果、未知模型、禁用模型、重定向与不含凭据的错误。真实供应商验证只用合成数据，不发送用户真实资产。工程测试不代表真实供应商已经可用。
