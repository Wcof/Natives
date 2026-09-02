# Natives CLIProxyAPI fork

Source: `github.com/router-for-me/CLIProxyAPI/v7` at commit
`f0de1d008fe8881dcb7431cf97b147295874c2b2` (MIT).

Only `sdk/`, `internal/`, module metadata, embedded runtime assets, and the
license are retained. Natives changes only the usage manager so its queue is
bounded and its dispatcher can restart after a gateway restart. The upstream
CLI, management UI, examples, and product runtime are intentionally excluded.
