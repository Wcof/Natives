# Natives Windows installer assets

These assets use only Windows PowerShell, the registry, and Chrome's external
extension policy. They do not install a service, startup entry, tray process,
runtime, or updater.

## Build/package layout

Put the signed release host beside these scripts as
`natives-native-host.exe`. Sign `*.ps1`, `*.cmd`, and the host with the
release certificate before distribution. The package is deterministic; the
only deployment input is the real Chrome Web Store extension ID.

On Windows, create the single self-extracting `Setup.exe` with the inbox
IExpress tool:

```powershell
.\build-installer.ps1 -ExtensionId <real-32-character-id>
```

IExpress is intentionally required; if it is unavailable the build fails
instead of silently producing a different installer. On macOS use
`-DryRun`/`self-check.ps1` for static validation.

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\install.ps1 -ExtensionId <real-32-character-id>
```

`-DryRun` performs validation and prints the exact file/registry plan without
writing files or the registry. `self-check.ps1` is safe to run on any OS and
only checks the assets and arguments.

Uninstall with the same real ID: `.\uninstall.ps1 -ExtensionId <real-32-character-id>`.

The Web Store ID is intentionally not included in this repository. A release
pipeline must supply the ID issued by Chrome Web Store; placeholders are
rejected.
