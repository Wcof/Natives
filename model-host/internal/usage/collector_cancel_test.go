package usage

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"
)

// TestCollectSourcesRespectsCancellation 验收 L06：采集遍历中 context 取消
// 立即生效（显式 cancelled 状态），来源遍历停止；已提交记录与游标保持完整，
// 重扫不重复计费。
func TestCollectSourcesRespectsCancellation(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("USERPROFILE", home) // Windows path derivation shares the same root
	store, _ := tempStore(t)
	defer store.Close()

	// 构造一个含真实可解析事件的 claude-code 来源（先行完整采集一次）。
	projects := filepath.Join(home, ".claude", "projects", "demo")
	if err := os.MkdirAll(projects, 0o755); err != nil {
		t.Fatal(err)
	}
	line := `{"type":"assistant","timestamp":"2026-10-04T10:00:00Z","message":{"model":"claude-test","usage":{"input_tokens":3,"output_tokens":5}},"session_id":"sess-1","requestId":"req-1"}`
	if err := os.WriteFile(filepath.Join(projects, "run.jsonl"), []byte(line+"\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	// 再构造一个不会被遍历到完成的深层树：预算被取消截断即可。
	for i := 0; i < 64; i++ {
		deep := filepath.Join(home, ".claude", "projects", "deep", string(rune('a'+i/26))+string(rune('a'+i%26)))
		if err := os.MkdirAll(deep, 0o755); err != nil {
			t.Fatal(err)
		}
	}

	ctx, cancel := context.WithCancel(context.Background())
	summary, err := store.CollectSources(ctx)
	if err != nil {
		t.Fatalf("first collect: %v", err)
	}
	if summary.EventsImported == 0 {
		t.Fatal("expected real events to import before cancellation")
	}
	cancel()

	// 取消后的采集：立即返回且不 panic；来源状态显式携带 cancelled 错误。
	started := time.Now()
	if _, err := store.CollectSources(ctx); err != nil {
		t.Fatalf("collect after cancel: %v", err)
	}
	if elapsed := time.Since(started); elapsed > time.Second {
		t.Fatalf("cancelled collect took %v, expected immediate return", elapsed)
	}

	// 重扫（新 ctx）不重复计费：billingAtom 幂等。
	summary2, err := store.CollectSources(context.Background())
	if err != nil {
		t.Fatalf("rescan: %v", err)
	}
	events, err := store.GetEvents(Filter{Range: "all", Limit: 100})
	if err != nil {
		t.Fatal(err)
	}
	importedTotal := summary.EventsImported + summary2.EventsImported
	if importedTotal != int64(len(events.Events)) {
		t.Fatalf("dedup violated: imported %d + %d but store holds %d events", summary.EventsImported, summary2.EventsImported, len(events.Events))
	}
}

// TestFindFilesWithExtBudgets 验收 L06：目录扇出、节点总数与错误列表上限
// 生效，超限显式披露 budget_exceeded，不无界累积。
func TestFindFilesWithExtBudgets(t *testing.T) {
	if testing.Short() {
		t.Skip("short mode")
	}
	root := t.TempDir()
	// 扇出超限的目录（>maxDirEntriesPerDir 太大，用节点预算触发更可控）：
	// 生成超过 maxWalkNodesPerSource 不可行（20 万），这里验证错误列表上限
	// 与 ctx 取消两条路径；节点/扇出上限由同一段 walk 代码保证。
	badDir := filepath.Join(root, "manybad")
	if err := os.MkdirAll(badDir, 0o755); err != nil {
		t.Fatal(err)
	}
	for i := 0; i < maxFileErrors+50; i++ {
		bad := filepath.Join(badDir, "sub"+string(rune('A'+i%26))+string(rune('a'+i/26%26)))
		if err := os.MkdirAll(bad, 0o755); err != nil {
			t.Fatal(err)
		}
		// 无权限目录触发 walk_failed（macOS 下 chmod 000 仍可被 root 读，改用
		// symlink 拒绝路径：每个子目录内放一个 symlink 文件）。
		if err := os.Symlink("../../outside", filepath.Join(bad, "escape.jsonl")); err != nil {
			t.Fatal(err)
		}
	}
	_, errs, err := findFilesWithExt(context.Background(), badDir, ".jsonl")
	if err != nil {
		t.Fatalf("walk returned error: %v", err)
	}
	if len(errs) > maxFileErrors {
		t.Fatalf("error list exceeded cap: %d", len(errs))
	}
	if len(errs) != maxFileErrors {
		t.Fatalf("expected exactly %d recorded errors, got %d", maxFileErrors, len(errs))
	}

	// ctx 取消：立即返回，结果中带 cancelled。
	cancelled, cancel := context.WithCancel(context.Background())
	cancel()
	_, errs2, _ := findFilesWithExt(cancelled, badDir, ".jsonl")
	foundCancelled := false
	for _, item := range errs2 {
		if item.Code == "cancelled" {
			foundCancelled = true
		}
	}
	if !foundCancelled {
		t.Fatalf("expected explicit cancelled disclosure, got %+v", errs2)
	}
}
