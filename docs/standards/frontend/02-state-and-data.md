# 前端 02 · 状态、数据与生命周期

> 版本：4.0.0 · 日期：2026-09-14

#### R-E8 · 持久状态来自 owner

- **等级**：MUST
- 页面启动先加载 Host snapshot/query，再建立事件订阅；DOM 和内存对象只是投影。
- Chrome storage 只保存语言、外观、视图等前端偏好，不保存 Host 业务状态、Secret 或计费权威。
- 主题持久值当前为 `archive | volt`；非法值回退 `archive`。

#### R-E9 · 草稿与提交分离

- **等级**：MUST
- 输入、拖拽、缩放、平移期间使用局部 draft。
- commit 使用 expected revision/mtime；失败恢复最后确认快照并给出可执行错误。
- pointer move 不写 Host/DB；stop/flush 最多一次提交。

#### R-E10 · 异步状态诚实

- **等级**：MUST
- 每个异步视图区分 loading、ready、empty、error、unsupported。
- 请求 generation/id 防止旧响应覆盖新状态。
- 刷新失败可以保留最后成功数据，但必须显示 stale/error 和重试。

#### R-E11 · 事件与查询页面级去重

- **等级**：MUST
- 同一 Native Host 在一个页面内优先共享 client；相同查询按规范化参数与 revision 合并。
- Host event 只建立一份页面级订阅，再分发给当前 renderer。
- 缓存必须有容量、revision/TTL 和清理条件；不得用空 catch 把失败变空数据。

#### R-E12 · 所有副作用有 disposer

- **等级**：MUST
- `addEventListener`、Chrome listener、timer、observer、object URL、Native Port 和 renderer 都要有 owner。
- 重绘前调用旧 disposer；destroy/pagehide/disconnect 清除 pending、timer 和 DOM 引用。
- BFCache pageshow 只恢复一次，不重复注册监听。

#### R-E13 · 隐藏态停止非必要工作

- **等级**：MUST
- `document.hidden` 时暂停时钟、背景轮播、自动刷新和无任务 Native Port。
- 可见后先重算真实状态，再恢复调度；不能补跑隐藏期间每一次 tick。
- 用户运行中的数据迁移/写入可继续，但 UI timer 与业务任务必须分开。

#### R-E14 · 错误统一分类

- **等级**：MUST
- 页面展示稳定 code 对应的本地化信息和 action。
- 原始异常、路径、SQL、命令、堆栈和 Secret 不进入 UI。
- retryable 才显示重试；权限、安装或安全问题提供对应修复入口。

#### R-E15 · 模态与焦点使用浏览器原生能力

- **等级**：MUST
- 优先语义化 `<dialog>`、button、input、a；打开后聚焦首个有效控件，关闭后恢复触发点。
- Escape 取消当前非破坏性操作；确认结果由 dialog returnValue/明确状态读取。
- 禁止 `alert`、`prompt`、`confirm`。

## 合规自检

- [ ] Host 是业务状态 authority。
- [ ] draft/commit/revision 边界明确。
- [ ] query、event、timer 没有按 Widget 重复。
- [ ] 每个副作用可释放，BFCache 不重复注册。
- [ ] hidden 停止非必要工作。
- [ ] 错误和模态可理解、可访问。
