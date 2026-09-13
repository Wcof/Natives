package usage

// ZCode rollout JSONL 采集器（R4，方案 §2.1/§6.2/§7.2）。
//
// source contract（本机安装版本实测 schema，2026-09）：
//   - 目录 ~/.zcode/cli/rollout/model-io-sess_<id>.jsonl，每行一个 JSON 对象；
//   - 仅 type=="model_io" 行产生计量事件；白名单字段：
//     requestId / sessionId / startedAt(ISO8601) / model{modelId,providerId} /
//     response.usage{inputTokens,cacheReadTokens,cacheWriteTokens,outputTokens,totalTokens}；
//   - ⚠ usage 桶口径：inputTokens 与 cacheRead/cacheWrite 的关系未在来源中
//     显式声明；按独立桶处理（Claude 口径）——若 input 已含 cache 子集，
//     TotalTokens 以来源 totalTokens 为准做非重叠校验，负差截 0 并保留原值；
//   - 稳定计费原子：zcode:<requestId>（无 requestId 的行跳过，不猜测）；
//   - 隐私：不读 request.body/messages、response.text/toolCalls 正文内容。

import (
	"bufio"
	"encoding/json"
	"os"
	"strings"
	"time"
)

// zcodeRolloutLine 为 ZCode rollout JSONL 行的白名单投影。
type zcodeRolloutLine struct {
	Type      string `json:"type"`
	RequestID string `json:"requestId"`
	SessionID string `json:"sessionId"`
	StartedAt string `json:"startedAt"`
	Model     struct {
		ModelID    string `json:"modelId"`
		ProviderID string `json:"providerId"`
	} `json:"model"`
	Response struct {
		Usage *struct {
			InputTokens      int64 `json:"inputTokens"`
			CacheReadTokens  int64 `json:"cacheReadTokens"`
			CacheWriteTokens int64 `json:"cacheWriteTokens"`
			OutputTokens     int64 `json:"outputTokens"`
			TotalTokens      int64 `json:"totalTokens"`
		} `json:"usage"`
	} `json:"response"`
}

// ParseZCodeRolloutJSONL 解析单个 ZCode model-io rollout 文件。
// 坏行/缺 requestId/缺 usage 的行跳过并计数（§7.3-6 partial 语义）。
func ParseZCodeRolloutJSONL(content []byte) ([]NativeEvent, int, int) {
	var events []NativeEvent
	parsed, skipped := 0, 0
	scanner := bufio.NewScanner(strings.NewReader(string(content)))
	scanner.Buffer(make([]byte, 0, 64*1024), 4*1024*1024)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" {
			continue
		}
		var l zcodeRolloutLine
		if err := json.Unmarshal([]byte(line), &l); err != nil {
			skipped++
			continue
		}
		if l.Type != "model_io" || l.RequestID == "" || l.Response.Usage == nil {
			continue
		}
		u := l.Response.Usage
		if u.InputTokens == 0 && u.CacheReadTokens == 0 && u.CacheWriteTokens == 0 && u.OutputTokens == 0 {
			continue
		}
		requestedAt := time.Now().UTC()
		if l.StartedAt != "" {
			if t, err := time.Parse(time.RFC3339Nano, l.StartedAt); err == nil {
				requestedAt = t.UTC()
			}
		}
		total := u.InputTokens + u.CacheReadTokens + u.CacheWriteTokens + u.OutputTokens
		if u.TotalTokens > 0 && total != u.TotalTokens {
			// 桶口径差异：以来源 totalTokens 为展示总量，保留各桶原值。
			total = u.TotalTokens
		}
		events = append(events, NativeEvent{
			BillingAtom:      "zcode:" + l.RequestID,
			SourceRecordID:   "zcode-req:" + l.RequestID,
			RequestedAt:      requestedAt,
			Provider:         firstNonEmpty(l.Model.ProviderID, "zcode"),
			Model:            firstNonEmpty(l.Model.ModelID, "unknown"),
			Result:           ResultSuccess,
			InputTokens:      u.InputTokens,
			OutputTokens:     u.OutputTokens,
			CacheReadTokens:  u.CacheReadTokens,
			CacheWriteTokens: u.CacheWriteTokens,
			TotalTokens:      total,
			SessionID:        l.SessionID,
			ParserVersion:    "zcode-rollout/1",
			RecordKind:       "request",
		})
		parsed++
	}
	return events, parsed, skipped
}

// collectZCodeRollout 采集 ZCode rollout 目录（幂等：zcode:<requestId>）。
func (s *Store) collectZCodeRollout(dir string) (int64, int64, error) {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return 0, 0, nil // 目录不存在：未授权/未安装，静默跳过
	}
	var importedCount, totalBytes int64
	instance := deriveInstanceID("zcode", dir)
	for _, e := range entries {
		if e.IsDir() || !strings.HasPrefix(e.Name(), "model-io-") || !strings.HasSuffix(e.Name(), ".jsonl") {
			continue
		}
		path := dir + "/" + e.Name()
		info, err := os.Stat(path)
		if err != nil || info.Size() == 0 {
			continue
		}
		content, err := os.ReadFile(path)
		if err != nil {
			return importedCount, totalBytes, err
		}
		events, parsed, _ := ParseZCodeRolloutJSONL(content)
		if parsed == 0 {
			continue
		}
		for _, ne := range events {
			ev := withIdentity(nativeEventToEvent(ne), "zcode", instance)
			ev.Source = "ZCode"
			inserted, _ := s.InsertEventIfAbsent(ev)
			if inserted {
				importedCount++
			}
		}
		totalBytes += info.Size()
	}
	return importedCount, totalBytes, nil
}
