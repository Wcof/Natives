# Natives 本机安装与测试指南（local 候选）

> 适用：macOS arm64 本机隔离候选（contract §4.2 本地模式：开发信任根 +
> 隔离系统源 `/Library/Application Support/Natives-Local/`）。
> 这不是正式 Release：正式分发要求 Developer ID/公证与生产信任根
> （ADR-0029 §8/§9），本指南只覆盖本机安装验证。

## 1. 构建候选

```sh
cd /path/to/Natives
node scripts/installer-package.mjs --mode local
```

前提：相邻目录存在 `../Natives-App-Fund/dist/fund-*-aarch64-apple-darwin.nap`
（基金内置模块载荷）；Go 与 Rust 工具链可用。产物：

```
dist/installer/Natives-0.1.0-macOS-arm64-local.pkg   # sha256 见同目录 SHA256SUMS
```

安装器为唯一引擎 `installers/macos/build-pkg.sh`（无 seeds、无
External Extensions、无 `.app`），组装器只负责输入：debug 构建的
native-file-host（本地信任根语义）、Go 构建的 model-host、解压扩展
目录（manifest 注入稳定 key，Extension ID 固定）、fund 单载荷解压 +
哈希核对、Ed25519 签名的产品组合清单。

## 2. 安装（写 /Library，需要管理员）

```sh
sudo installer -pkg dist/installer/Natives-0.1.0-macOS-arm64-local.pkg -target /
```

或双击分发外壳 **`Natives-0.1.0-macOS-arm64-local.dmg`** → 双击其中的
`.pkg` → 继续 → 输入管理员密码（DMG 只是把同一个安装器包成一层的分发
外壳；写 /Library 仍由 pkg 完成）。安装内容（root-owned，只写系统源 +
浏览器最小注册）：

```
/Library/Application Support/Natives-Local/
  native-file-host  model-host  ChromeExtension/  modules/fund/0.1.0/app
  product-manifest.json  product-manifest.sig  extension-id  uninstall.sh
/Library/Google/Chrome/NativeMessagingHosts/com.natives.file_manager.json
/Library/Google/Chrome/NativeMessagingHosts/com.natives.model_host.json
（Chromium 同理）
```

不触碰：`/Applications`、用户 DB/activation、`~/.natives`。

## 3. 双击 Natives，按引导加载扩展（§1.1 主入口流程）

1. 安装完成后打开 Finder"应用程序"：应看到 **Natives**（名称、图标、版本正常）。
2. **双击 Natives**：自动打开 Chrome，显示随包离线引导页
   （`onboarding/index.html`），同时打开扩展管理页并在 Finder 定位
   扩展目录（目录路径已自动复制到剪贴板）。Chrome 未安装时给出原生提示。
3. 按引导页三步操作：`chrome://extensions` → 开启"开发者模式" →
   "加载已解压的扩展程序"选择被定位的目录（选目录，不是 ZIP）。
4. 核对扩展 ID 必须是 `gehmgcnlpdepnpmcbbdaijabcjdnbfmh`
   （manifest 内置 key 派生，与 Native Messaging `allowed_origins` 一致；
   若不一致说明加载了错误目录）。扩展就绪后再次双击 Natives 直接进入产品。

手动路径（等价）：`chrome://extensions` → 开发者模式 → 加载已解压 →
选择 `/Library/Application Support/Natives-Local/ChromeExtension`。

## 4. 完成产品配置并测试（§3.3 / §6.3 本机简化清单）

1. 从扩展打开 Natives（空间或文件页）。
2. 设置 → 应用中心（应用列表）：未配置时顶部出现
   **"完成 Natives 配置"**；点击后基金卡片变为"就绪"。
   （该操作校验签名产品清单并为当前用户准备全部固定模块：
   载荷复制到 `~/.natives/apps/fund/runtime/`、注册 fund Host、
   写 activation，绑定同一 productGeneration。）
3. **断网**打开基金，录入/保存一笔账，重开验证数据保留、无任何代码下载。
4. 隐藏基金侧栏入口，再从中心打开——均无安装步骤。
5. 中心对基金执行"清数据"（二次确认，凭据默认保留）后仍可直接打开
   基金从空数据开始——不需要重新安装。
6. 完全退出 Chrome 再重开：配置与数据保留。

## 5. 卸载（用户数据保留）

```sh
sudo "/Library/Application Support/Natives-Local/uninstall.sh"
```

只删除系统源目录与浏览器 NM 注册；`~/.natives`（含基金数据）保留。

## 6. 已知边界

- 本地候选的 Host 是 debug 构建（本地信任根语义所需）；性能门禁按
  A-Local 规则另行度量。
- 换载/重装后扩展 ID 不变（key 固定）；重新安装 pkg 幂等覆盖系统源。
- 正式发布链（签名/公证/生产信任根/实机 N→N+1）见实施方案 §6/§8，
  不在本指南范围。
