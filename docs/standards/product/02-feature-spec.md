# 产品架构 02 · 功能与诚实状态

> **版本**: 3.0.0 · **日期**: 2026-08-19
> **关联 ADR**: [ADR-0020](../../adr/0020-ai-native-personal-workspace-rearchitecture.md)
> **说明**: 本篇取代旧 Assistant / Agent / Workshop / Capability 功能清单。

## 一、状态规则

#### R-F1 · 功能状态必须可验证
- **等级**：MUST
- **分类**：无假数据、错误处理
- **规则**：功能只能声明为 `implemented`、`partial`、`unsupported` 或 `unavailable`；`implemented` **必须**有真实 source、调用链和测试证据。错误不得转换成空数组、零值或成功 toast。

#### R-F2 · 用户可见数据必须有真实来源
- **等级**：MUST
- **分类**：无假数据
- **规则**：文件、应用状态、Provider health、quota、usage、cost、进程、端口和工具检测结果**必须**来自真实领域 query。无来源显示 Unknown/Unavailable；估算值必须标注估算。

#### R-F3 · 空态、加载态、错误态分离
- **等级**：MUST
- **分类**：交互、错误处理
- **规则**：UI **必须**区分 loading、empty、error、unsupported；不得用 skeleton 永久遮蔽失败，不得把 error 当 empty。

## 二、目标功能域

| 域 | P0/P1 目标 | 禁止扩展 |
|---|---|---|
| Home | 多 Workspace（创建/打开/切换/关闭/重开/置顶/排序/重命名/复制/模板/删除，Close 只关会话、Delete 才删资源）、Grid/Canvas 双布局、增删移缩复制配隐重置、Widget Catalog（搜索/分类/添加/选择/配置）、Data View 四模式（List/Table/Board/Calendar） | Widget Runtime/市场、Plugin 平台、无限画布 |
| Files | CRUD、Trash、Watch、Search、Recent/Favorite、预览入口 | 为 Home 重写文件系统 authority |
| Apps | App / RuntimeSpec / RuntimeInstance / Surface、受监督启动/停止/探测 | 把 App 当 Widget 或裸 spawn |
| AI Resources | Provider / Connection / Credential、多 Key、OAuth、模型目录与健康 | 把 protocol 当 Provider、明文 Secret |
| Local Proxy | Messages / Chat Completions / Responses、tools/stream/reasoning、Key Pool、usage | 企业 Gateway、多租户计费、第二 Event Platform |
| AI Tool Integration | Detect/Inspect/Backup/Plan/Apply/Verify/Rollback | 直接覆盖用户配置、无回滚写入 |
| Data & Usage | 现有 source parser、归一、聚合、完整 Usage 页 | 新统一 Event Platform、假 quota/cost |
| Settings | 通用/外观/AI/Proxy/工具/个人摘要 | 把完整 Usage Dashboard 当 Home |

## 三、硬 Gate

#### R-F4 · Protocol 与 Secret Gate
- **等级**：MUST
- **分类**：安全、错误处理
- **规则**：Proxy 上线前**必须**通过三协议真实 fixture、tool/reasoning/usage/stop reason、异常 EOF、取消回收、Key Pool 与 OAuth refresh 测试。Secret 迁移上线前**必须**通过 Keychain write/read/verify/rollback/locked 与 disk/log scan。

#### R-F5 · AI Tool 配置写入 Gate
- **等级**：MUST
- **分类**：数据、安全
- **规则**：任何 AI 工具配置修改**必须**先 inspect，生成 plan，原子 backup，apply 后 verify；失败必须 rollback。不得在未识别格式上盲写。

#### R-F6 · Home 性能 Gate
- **等级**：MUST
- **分类**：性能
- **规则**：Home 不得按 Widget 数量重复相同 query/timer；drag/resize pointer move 写 DB 次数必须为 0；hidden/background 资源必须停止；20 Widget、循环 resize/sidebar、soak 与 packaged Tauri/WebKit 必须验收。

## 四、Legacy Death Proof

最终发布前必须证明：

- 新 IA 是唯一生产入口；
- Assistant/Jobs/Capabilities/Agent Daemon/Agent crates/Plugin Runtime 无生产引用；
- 旧 schema 迁移与回滚可验证，secret 不残留明文或同盘主密钥；
- 无孤儿进程、端口、PTY、Watcher、Timer、Listener、WebView、Socket、Task；
- 完整 typecheck/lint/test/perf、Rust workspace、协议/迁移/构建 Gate 通过。
