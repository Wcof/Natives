# macOS package

Build from the repository root with a prebuilt executable and the fixed local
extension identity. The identity is needed only for Native Messaging
allowlists; the package never contacts or registers with an extension store:

```sh
sh installers/macos/build-pkg.sh \
  --host target/release/native-file-host \
  --extension-id <32-character-extension-id> \
  --output dist/Natives-macos.pkg
```

`pkgbuild` and `productbuild` are the only packaging tools. Add `--app-sign`,
`--pkg-sign`, and `--product-sign` with Developer ID identities for app,
component package, and product package signing. Notarization is an external
`notarytool`/Developer ID credential step; without signing arguments the output
is explicitly an unsigned development package. Run
`sh installers/macos/verify.sh` for static and temporary-layout checks. The
uninstaller accepts `--extension-id` when the saved install metadata is absent.
