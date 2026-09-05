# Natives CLIProxyAPI fork

Source: `github.com/router-for-me/CLIProxyAPI/v7` at commit
`5208aec7` (`v7.2.151`, MIT).

Only `sdk/`, `internal/`, module metadata, embedded runtime assets, and the
license are retained. Natives changes only the usage manager so its queue is
bounded and its dispatcher can restart after a gateway restart. The upstream
CLI, management UI, examples, and product runtime are intentionally excluded.
