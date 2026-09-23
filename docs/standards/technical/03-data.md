# 技术 03 · 数据、迁移与恢复

> 版本：4.0.0 · 日期：2026-09-14

## Authority 表

| 数据 | 唯一 writer | 页面可见形式 |
|---|---|---|
| Files roots、收藏、最近、Workspace、布局、App 登记/偏好 | Files Host | Native query/event projection |
| 文件内容与目录 | Files Host 经 `file-manager-core` | 分页/窗口化结果 |
| Provider、Connection、非秘密配置 | Model Host | snapshot/query |
| Credential 与模块 Secret | OS Keychain | 引用、状态、掩码 |
| Usage、价格、账单、会话聚合 | Model Host Usage Store | 分页聚合结果 |
| 内置模块业务数据 | 对应 App Runtime Process | 模块业务 API |
| Extension 本地偏好 | Chrome storage，仅限展示偏好 | 当前页面派生状态 |

#### R-D1 · 用户数据只在约定根

- **等级**：MUST
- 用户数据位于当前用户的 Natives 私有根；模块数据位于 `~/.natives/apps/<appId>/data/`。
- 浏览器规定的 Native Messaging 注册目录只放最小 manifest，不放业务数据。
- 系统级产品目录只放签名代码和固定资源，不写用户 DB、activation 或业务迁移。

#### R-D2 · SQLite 初始化一致

- **等级**：MUST
- 每个 DB 在连接初始化时启用 foreign keys；需要并发读写的 DB 使用 WAL 和明确 busy timeout。
- 事务边界覆盖完整不变量；错误时回滚，不提交半状态。
- 同一 non-reentrant Mutex guard 作用域内不得再次调用会获取同一锁的公共方法。

#### R-D3 · 迁移只向前且幂等

- **等级**：MUST
- schema 迁移有单调版本、事务、重复执行测试和坏状态恢复。
- 禁止为加列直接 DROP/重建用户表。
- 产品隐式降级拒绝；代码回退不得覆盖已接受的新数据。

#### R-D4 · 重要文件原子写

- **等级**：MUST
- 配置、收据、activation、journal 和用户文档采用同目录临时文件 → flush/fsync → rename。
- 并发编辑使用 revision、mtime 或内容摘要检测冲突。
- 权限设置在公开路径前完成；失败清理临时文件。

#### R-D5 · 产品代码与用户数据分离

- **等级**：MUST
- 完整产品安装/更新只更换受验签代码和固定模块文件；所有可执行代码位于系统产品源目录。
- 用户应用根目录（`~/.natives/apps/<appId>/`）严格只用于保存 `activation.json`、`data/`、`imports/`、`cache/`、`logs/`，严禁在用户目录下存放可执行文件（彻底废弃 `runtime/<version>/app` 模式）。
- 首次打开模块才以当前用户权限初始化或迁移业务数据。
- 重装、修复和更新保留模块显示偏好、用户数据和 Keychain。

#### R-D6 · 清除数据是独立危险操作

- **等级**：MUST
- 隐藏模块、停止运行、更新产品与清除数据是不同动作。
- 清数据前显示范围并二次确认；失败保存 `cleanup_pending` 或等价可重试状态。
- 不删除产品外路径、其他用户数据或不属于该 appId 的 Keychain 项。

#### R-D7 · 数据增长有界

- **等级**：MUST
- 长列表、Usage events、历史、缓存和日志必须分页、限额、轮转或按保留策略清理。
- UI 不一次性承载完整历史；导出可以流式读取权威数据。
- 删除/压缩策略必须可解释，不能静默丢失计费或用户文件。

#### R-D8 · 迁移与导入可取消

- **等级**：MUST
- 长任务有 progress、cancel 和 crash recovery；取消后不留下半注册、半写文件或持锁状态。
- 模块迁移先备份、验证后提交；已接受新写入后不得自动恢复旧备份。

## 合规自检

- [ ] 每类数据只有一个 writer。
- [ ] schema 与文件更新原子、幂等、可恢复。
- [ ] 产品更新不以 root 写用户数据。
- [ ] 清数据和代码操作分离。
- [ ] 长期增长数据有界。
