package main

// 开发工具：真实 Host 导出 fixture（§5.5「真实工具数据的默认深/浅截图」数据源）。
//
// 走产品真实采集链路 CollectSources（白名单元数据，§6.2 边界）写入临时 DB
// （绝不触碰生产用量库），再用与 Native Messaging 相同的聚合方法
// GetOverview/GetEvents/GetSessions 导出 JSON fixture。
// 产物仅用于浏览器截图验收，不进生产数据；不持久化 Prompt/正文。

import (
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"

	"github.com/ldh/natives/model-host/internal/usage"
)

func main() {
	out := flag.String("out", "", "输出 fixture JSON 路径（必填）")
	rangeFlag := flag.String("range", "30d", "聚合范围（默认 30d）")
	flag.Parse()
	if *out == "" {
		fmt.Fprintln(os.Stderr, "usage: export-usage-fixture -out <path> [-range 30d]")
		os.Exit(2)
	}

	tmpDir, err := os.MkdirTemp("", "natives-usage-fixture")
	if err != nil {
		fatal("temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	store, err := usage.NewStore(filepath.Join(tmpDir, "fixture.db"))
	if err != nil {
		fatal("open temp store: %v", err)
	}
	defer store.Close()

	summary, err := store.CollectSources()
	if err != nil {
		fatal("CollectSources: %v", err)
	}

	f := usage.Filter{Range: *rangeFlag}
	overview, err := store.GetOverview(f)
	if err != nil {
		fatal("GetOverview: %v", err)
	}
	events, err := store.GetEvents(usage.Filter{Range: *rangeFlag, Limit: 200, SortBy: "requested_at", SortDir: "desc"})
	if err != nil {
		fatal("GetEvents: %v", err)
	}
	sessions, err := store.GetSessions(f)
	if err != nil {
		fatal("GetSessions: %v", err)
	}

	fixture := map[string]any{
		"range":          *rangeFlag,
		"collectSummary": summary,
		"overview":       overview,
		"events":         events,
		"sessions":       sessions,
	}
	data, err := json.MarshalIndent(fixture, "", "  ")
	if err != nil {
		fatal("marshal: %v", err)
	}
	if err := os.MkdirAll(filepath.Dir(*out), 0o755); err != nil {
		fatal("mkdir: %v", err)
	}
	if err := os.WriteFile(*out, data, 0o644); err != nil {
		fatal("write: %v", err)
	}

	fmt.Printf("REAL FIXTURE EXPORTED: %s\n", *out)
	fmt.Printf("sourcesScanned=%d filesProcessed=%d eventsImported=%d bytes=%d\n",
		summary.SourcesScanned, summary.FilesProcessed, summary.EventsImported, summary.BytesRead)
}

func fatal(format string, args ...any) {
	fmt.Fprintf(os.Stderr, "export-usage-fixture: "+format+"\n", args...)
	os.Exit(1)
}
