# Windows 本地候选包

Windows 当前只保留与产品一致的免费本地加载路径：扩展目录和 Native
Hosts 一起打进 ZIP，首次运行注册 Native Messaging 后打开
`chrome://extensions`，用户开启开发者模式并选择包内 `Extension` 目录。
不写 Chrome 扩展商店注册表，不使用 `update_url`，也不依赖 Chrome Web
Store 账号。

从仓库根目录生成候选包：

```powershell
npm run package:windows:local
```

输出 `dist/Natives-Windows-x64-Local.zip`。在 Windows 上解压后双击
`安装并注册 Host.cmd`；向导会自动打开扩展管理页和扩展目录。加载成功后
双击 `启动 Natives.cmd` 进入空间，应用中心中的 Fund 等模块随同一套
Natives 文件提供，不存在模块级下载或安装。

这只是本地候选包。Windows 正式签名、实机生命周期和卸载验证通过后，才
能进入 release 门禁；旧的 IExpress/Web Store 脚本已归档到
`docs/archive/legacy-development/windows-webstore-installer/`，不得恢复为
生产入口。
