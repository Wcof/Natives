# Natives CLIProxyAPI fork

Source: `github.com/router-for-me/CLIProxyAPI/v7` at tag
`v7.3.17` (MIT). Synced from the local upstream checkout by exporting
`internal/`, `sdk/`, `go.mod`, `go.sum` via `git archive v7.3.17`.

Only `sdk/`, `internal/`, module metadata, embedded runtime assets, and the
license are retained. The upstream CLI, management UI, examples, and product
runtime are intentionally excluded.

Natives-specific changes on top of upstream (captured as replayable diffs in
`natives-patches/*.patch`; the in-app kernel updater re-applies them after
exporting an upstream tag — keep that directory in sync with any change):

- `sdk/cliproxy/usage/manager.go`: the usage dispatcher uses a `running`
  flag instead of `sync.Once` so it can restart after a gateway restart.
- `internal/runtime/executor/antigravity_executor_credits.go` (+ test):
  antigravity 400 responses whose body contains "user location is not
  supported" are surfaced as retryable 503 errors instead of permanent
  failures.
- `sdk/cliproxy/static_models.go` (fork-added): bridge exposing
  `registry.GetStaticModelDefinitionsByChannel` to SDK consumers.
- `internal/runtime/executor/helps/payload_mutations_test.go`:
  `TestConfigExampleDocumentsCodexAdditionalToolsPayloadFilter` is removed —
  it reads the upstream repo-root `config.example.yaml` which the trimmed
  layout excludes.

Upstream OAuth client credentials (e.g. antigravity `constants.go`) are
kept as-is from upstream; do not blank them — blanked client ids break
Google authorization with "Missing required parameter: client_id".
