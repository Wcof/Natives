# Long Task 06: Integration and Completion Audit

## Mission

Integrate Tasks 1-5, fix cross-task regressions, and prove the original Assistant objective requirement by requirement. This task is not a superficial “run tests” pass.

## Dependency

Run only after Tasks 1-5 have produced handoff reports. Read every report before editing.

## Original Requirements To Prove

1. Natives Assistant incorporates useful CodePilot behavior without transplanting incompatible architecture.
2. Assistant and file manager use the same underlying Git implementation.
3. Users can safely list, create, and switch real project branches.
4. Users can switch real configured providers and discovered models.
5. Provider/model selection reaches the actual conversation runtime.
6. Conversation processing is structured and streaming.
7. Content, reasoning, tools, results, files, citations, diffs, errors, completion, cancellation, and retry display coherently.
8. Project/session workflows are complete and desktop startup is reliable.
9. No unnecessary debug storage, duplicate transcript archive, fake data, recent-model localStorage, hidden Git repository, or automatic stash was added.
10. Chinese and English UI remain synchronized.

## Required Reading

- `AGENTS.md`
- Entire `docs/standards/` map relevant to touched files
- `docs/superpowers/specs/2026-07-12-assistant-workspace-integration-design.md`
- All six files in `docs/agent-tasks/assistant/`
- Handoff reports from Tasks 1-5
- Relevant CodePilot reference files for behavior comparison only

## Write Scope

Any Assistant-related file may be changed to resolve verified integration defects, but do not perform unrelated cleanup. Preserve user changes and current architecture boundaries.

## Detailed Audit Procedure

### 1. Create a Requirement Evidence Table

For each original requirement above, record:

- authoritative implementation files;
- runtime/data flow;
- covering test or smoke step;
- status: proven, contradicted, incomplete, or missing;
- exact remediation if not proven.

Do not mark “proven” based only on file existence or a narrow unit test.

### 2. Audit Shared Git

- Search every production `git` subprocess invocation.
- Prove Assistant/file manager call the same `window.nativesAPI.git` and Rust module.
- Prove events refresh both surfaces.
- Test dirty and linked-worktree blocking.
- Confirm no automatic stash/reset/force path.

### 3. Audit Provider/Model Runtime Identity

Trace one selected provider/model from picker click through conversation persistence to runtime request construction. Confirm every identifier matches. Test stale selection correction and unavailable model disabling.

### 4. Audit Streaming Lifecycle

Trace one run from send through events, reducer, blocks, persistence, reopen, cancel, and retry. Confirm no duplicate listeners/reducers and no live/stored visual contract mismatch.

### 5. Audit Project and Startup

Trace active directory through conversation create, terminal/file/Git context, restart restoration, and sidebar grouping. Verify daemon startup and readiness retry behavior.

### 6. Audit Storage and Fake Data

Search for:

```bash
rtk grep "localStorage|debug|replay|transcript|recent-model|mock|fake|placeholder" src src-tauri crates
```

Classify each match. Remove newly introduced debug/recent/fake persistence. Do not remove legitimate existing logging blindly; verify scope and ownership.

Inspect database migrations and tables to prove no duplicate Assistant/Git store was introduced.

### 7. Audit i18n and Errors

- Compare locale key structures.
- Find hard-coded visible strings in touched Assistant components.
- Confirm caught errors use `classifyError` and do not expose raw secrets/stderr.

### 8. Fix Every Unproven Requirement

Implement focused fixes with tests. Do not redefine the objective around what currently passes. If external credentials prevent one live provider check, document that residual verification gap accurately while still completing all locally provable work.

## Required Verification Suite

Run at minimum:

```bash
rtk test npm run test -- src/lib/git-branch-state.test.ts src/lib/provider-model-selection.test.ts src/lib/assistant-stream-state.test.ts src/lib/assistant-project-groups.test.ts src/lib/active-project.test.ts
rtk test npm run test -- src/components/assistant/blocks/blocks.test.tsx src/components/assistant/blocks/markdown-content.test.tsx
rtk cargo test --manifest-path src-tauri/Cargo.toml git -- --nocapture
rtk cargo test --manifest-path src-tauri/Cargo.toml provider -- --nocapture
rtk cargo test --manifest-path src-tauri/Cargo.toml conversation -- --nocapture
rtk cargo test -p assistant-protocol -- --nocapture
rtk cargo test -p agent-core -- --nocapture
rtk cargo test -p provider-adapters -- --nocapture
rtk node scripts/assert-assistant-dev-contract.mjs
rtk tsc --noEmit
rtk git diff --check
```

Then run the repository's broader relevant test commands from `AGENTS.md` if time/resources permit. Report pre-existing failures separately from regressions, with evidence.

## Required Desktop Smoke Scenario

Using a real local project and real configured provider where available:

1. Launch the desktop app through the supported development command.
2. Select a project directory.
3. Verify file manager and Assistant show the same branch.
4. Create a branch and switch back.
5. Confirm dirty changes block switching.
6. Create a conversation.
7. Switch provider/model and verify persistence after reopening.
8. Send a prompt and observe structured streaming.
9. Exercise at least one tool or record why unavailable.
10. Stop a run and confirm partial output remains.
11. Retry and confirm a new run is created.
12. Restart and verify project/conversation/model restoration.

Capture concise evidence, not debug archives. Screenshots are acceptable; persistent raw event dumps are not.

## Completion Gate

The task is complete only when the requirement evidence table marks every locally verifiable item proven, required tests pass, smoke behavior is demonstrated, and remaining external-only gaps are explicitly identified. “No obvious issue found” is not completion evidence.

## Final Report

Provide:

- requirement evidence table;
- user-visible outcomes;
- tests with pass/fail counts;
- desktop smoke evidence;
- exact residual risks or unavailable external checks;
- confirmation that no duplicate Git/debug persistence was introduced;
- safe commit/staging status without swallowing unrelated working-tree changes.

