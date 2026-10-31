# @natives/dsh-space

Natives 空间模块，以 DeepSeek Harness（DSH）独立插件运行（ADR-0048）：侧栏
1 个入口，点击后主面板全屏打开 `space.html`，不切换会话、不出现 Natives
产品级导航或其他模块入口。

## 安装

```bash
install_bundle target=/绝对路径/@natives-dsh-space-0.0.1.tgz
remove_bundle target=@natives/dsh-space   # 卸载（不删用户数据）
```

可与 `@natives/dsh-files`、`@natives/dsh-tokenusage`、`@natives/dsh-fund`
任意组合安装；四者共用同一份 Natives Core（127.0.0.1:8770）与同一份本地数据。

## 运行前提（按顺序解析）

1. 本机已运行的 Natives Core（attach，端口取锁文件）；
2. 源码 checkout 构建（仅当插件位于 checkout 内）；
3. 本机已安装 Natives 产品源；
4. 随包 `payload/`（完整签名产品源快照；仅生产签名 payload 可对外分发）。

没有以上任一来源时入口会提示「未找到 Natives 产品源」。

## 验证

1. 侧栏出现「空间」入口；
2. 点击后主面板全屏加载工作区卡片，主题跟随 DSH 明暗（不改写用户偏好）；
3. 侧栏只保留空间工作区树，无文档快捷方式／应用菜单／设置入口；
4. 数据 schema 高于插件运行时时明确拒绝，不降级写。
