# 技术 05 · Native Host 工程规范

> 版本：4.0.0 · 日期：2026-09-14
> 适用：Rust Files/App Core 与 Go Model Host

#### R-B1 · 外部请求返回结构化结果

- **等级**：MUST
- Native 方法返回稳定 success/result 或 error code/message/action。
- 可预期的用户输入、文件、DB、网络和权限错误不得 panic、`unwrap` 或 `expect`。
- 测试和真正不可达不变量可以例外，但必须局部且有说明。

#### R-B2 · 协议解析先于副作用

- **等级**：MUST
- 帧、方法、字段、长度、revision、appId 和路径在统一 dispatch 边界验证。
- 类型与枚举只有一个生产定义；页面 fixture 用同步检查防漂移。
- 未支持方法明确返回 unsupported/invalid request，不静默降级。

#### R-B3 · 日志统一脱敏

- **等级**：MUST
- API key、token、Authorization、用户主目录、请求体和业务敏感字段写出前经统一 sanitizer。
- 禁止直接 Debug 打印外部请求、环境变量、Keychain 内容或 Provider 响应。

#### R-B4 · 共享代码必须有真实复用

- **等级**：MUST
- Files 领域复用 `file-manager-core`；App Runtime 基础设施与安全协议复用 `app-runtime-core`。
- 只有两个以上生产调用方共享的稳定逻辑才进入 shared crate/package。
- 禁止为未来可能复用建立 runtime、factory、interface 或第二协议层。

#### R-B5 · 阻塞工作不占用请求线程

- **等级**：MUST
- Rust async 中的文件、SQLite、压缩、哈希和进程等待进入 `spawn_blocking`、专用线程或现有有界池。
- Go 长 IO 接受 `context.Context`，设置 deadline/cancel，不持锁等待网络或子进程。
- 并发有固定上限；队列满返回明确 backpressure。

#### R-B6 · 锁与事务不重入

- **等级**：MUST
- 持有 non-reentrant Mutex guard 时不得调用会获取同一锁的公共方法。
- transaction commit 后先显式释放 connection/guard，再调用 snapshot/query。
- 修锁问题必须检查所有 sibling `commit → snapshot/session/query` 路径。

#### R-B7 · 子进程和端口必须回收

- **等级**：MUST
- 每个 spawn 点记录 owner、PID/identity、stdin/EOF、cancel、wait 和失败清理。
- 禁止裸 spawn 后丢弃 handle；短命令也必须 wait 或显式 reap。
- loopback 绑定 `127.0.0.1:0`；关闭顺序为取消新请求 → 关闭 listener → 等待任务/进程。
- 只终止由当前 owner 创建且 identity 匹配的进程。

#### R-B8 · Native Messaging 主循环有界

- **等级**：MUST
- 单个 reader 解析帧；pending request 按 id 管理并在完成、超时、disconnect 时删除。
- EOF 进入统一 shutdown；不得由第二个 goroutine/thread 重复读取 stdin。
- 大响应分页或流式，不拼接无界 buffer。

#### R-B9 · 数据遵守唯一 authority

- **等级**：MUST
- DB、迁移、原子文件和清理遵守 `03-data.md`。
- App Host 不连接 Core DB 写业务表；Core 不创建模块业务表。
- Model Host 不拥有 Files 能力；Files Host 不读取 Provider Secret。

#### R-B10 · 文件和函数保持可审查

- **等级**：SHOULD
- 生产文件推荐 ≤500 行；≥700 行登记原因；≥1000 行必须拆分或有明确书面例外。
- 函数推荐 ≤60 行；>120 行应拆职责；不要用 `utils`/`helpers` 隐藏复杂度。
- 拆分按数据 owner、生命周期或稳定接口，不按行数机械切片。

#### R-B11 · 测试卡住按死锁处理

- **等级**：MUST
- Rust/Go 测试停止输出先限时运行一个精确测试，检查进程和栈；不得并发叠加重复尝试。
- 不把活动测试管道接到只在 EOF 输出的命令。

## 合规自检

- [ ] 请求边界先验证，错误结构化。
- [ ] 日志无 Secret 和用户敏感明文。
- [ ] 阻塞、并发、缓存和队列有界。
- [ ] 锁/事务没有重入。
- [ ] 子进程、端口和 pending request 可回收。
- [ ] Host 领域 authority 未越界。
