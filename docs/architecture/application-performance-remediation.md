# Natives 性能与内存回收验收方案

> 更新：2026-09-14。本文是当前性能门禁和剩余验证项；早期架构审计已移至
> `docs/archive/legacy-development/application-performance-remediation.md`。

实施对象是 Extension、`native-file-host`、`file-manager-core`、`model-host` 和内置模块。
性能优化只修改已证实的瓶颈，安全、数据完整性、可访问性和 Native EOF 回收规则优先级更高。

## 当前基线

| 对象 | 目标 | 当前判断 |
|---|---:|---|
| Extension 发布体积 | ≤ 360 KiB | 由 `extension:check` 复核 |
| `native-file-host` Release 体积 | ≤ 4 MiB | 由 `perf:check` 复核 |
| Host 空闲 RSS | ≤ 12 MiB | 由 `perf:check` 复核 |
| Model Host 空闲 RSS | ≤ 256 MiB | 由 `perf:check` 复核 |
| Host EOF 退出 | ≤ 2 s | 必须实测 |
| 10,000 项目录查询 p95 | ≤ 50 ms | 必须实测 |
| Extension 空闲 CPU | ≤ 5% | 必须实测 |
| Watcher 合并窗口 | 250 ms | 代码与实测一致 |

一次通过只代表该构建、该平台和该数据集通过，不能外推到未验收平台。

## 生命周期约束

```text
newtab/空间：只渲染可见 Widget
  页面隐藏 → 暂停时钟、轮播和刷新
  最后一个 AI Widget 销毁 → 断开 Model Native Port

files.html：唯一 Files Native Port
  隐藏且无任务 60 秒 → 断开
  pagehide/关闭 → 断开 → Host EOF → 2 秒内退出

内置模块：owner page → App Host → 受限 loopback iframe
  页面隐藏 60 秒 → 停止模块并释放端口
```

所有 interval、listener、Blob URL、Native Port、Watcher 和临时文件都必须有对称 disposer。
Service Worker 不创建 Port、不轮询、不保活；Model Host 常驻只有用户显式开启时才允许。

## 已完成的代码整改

- App Center 已移除模块下载、目录、种子和独立发布生产链；模块代码随完整产品交付。
- Host 的旧安装事务和启动恢复调用已删除；产品配置保留验签、staging、原子切换和恢复安全原语。
- App Store 只投影内置模块；清除数据保留代码、注册、activation 和显示/启用偏好。
- Extension 清理了重复主题监听和旧安装状态字段；关闭模块的运行锁覆盖 activation 与数据库事务。
- 文件 Host 保持有界分页、取消、并发、路径授权和 EOF 清理。

## 必须补齐的证据

### P0：稳定基线

1. `perf:check` 每次先用 Cargo 构建当前 Release Host，再测体积、RSS、CPU、EOF 和目录 p95。
2. Model Host 冷启动至少采样 5 次，分开报告编译、Keychain、Usage 初始化和首个快照耗时。
3. Extension 分别输出 `newtab`、`files`、`apps`、`model-settings` 的发布依赖体积。

### P1：真实浏览器生命周期

在干净 Chrome profile 中重复 30 分钟：打开/关闭文件页、切换空间主题、增删 AI Widget、打开/隐藏内置模块。
记录 Renderer/Host RSS、CPU、Native Port、监听器、interval、Blob URL 和 loopback 端口数量；结束时应回到基线，增长趋势超过 10% 即失败。

### P1：数据规模与取消

使用 50 万条用量事件和 10,000 项目录，验证查询 p95、峰值内存、取消后的任务数与连接数归零。
任何超时、取消或页面关闭都必须停止后续依赖请求，不能把未知结果记为成功。

### P2：平台证据

macOS arm64/x64 和 Windows 分别执行安装、升级、修复、卸载保留数据、Native EOF 与空闲 CPU 验收。
缺真实设备或签名时标为 `unknown`/`blocked`，不以模拟器或静态产物代替。

## 门禁

```sh
rtk npm run standards:check
rtk npm run extension:check
rtk npm run perf:check
rtk npm run perf:files
rtk env -u CARGO_TARGET_DIR cargo fmt --check
rtk env -u CARGO_TARGET_DIR cargo test --workspace
```

若性能变化来自业务修复，记录同一设备、同一构建、同一数据集的 before/after；没有可比证据就不宣称优化完成。
