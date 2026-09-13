package quota

import (
	"context"
	"net/http"
	"testing"
)

// R6（方案 §4.3）：额度诚实状态——查询失败必须返回 status=error 且
// windows 为空，不得伪造剩余百分比（不能因 HTTP 成功就显示 100%）。

type stubTransport struct {
	status int
	body   string
	err    error
}

func (s *stubTransport) RoundTrip(*http.Request) (*http.Response, error) {
	if s.err != nil {
		return nil, s.err
	}
	return &http.Response{
		StatusCode: s.status,
		Body:       http.NoBody,
		Header:     make(http.Header),
	}, nil
}

func stubClient(t *testing.T, tr *stubTransport) *Client {
	t.Helper()
	c := NewClient()
	c.httpClient = &http.Client{Transport: tr}
	return c
}

// 不支持的 provider：明确 error，windows 空，无百分比。
func TestQueryUnsupportedProviderHonestError(t *testing.T) {
	res, err := NewClient().Query(context.Background(), "unknown-provider", "acct", "tok", "")
	if err != nil {
		t.Fatalf("Query 返回 transport 错误: %v", err)
	}
	if res.Status != "error" {
		t.Fatalf("Status = %q, want error", res.Status)
	}
	if res.Error == "" {
		t.Fatal("Error 必须说明 unsupported 原因")
	}
	if len(res.Windows) != 0 {
		t.Fatalf("windows = %+v, want 空（不得伪造窗口）", res.Windows)
	}
}

// HTTP 4xx：error 且 windows 空，不把失败冒充 100% 用尽。
func TestQueryHTTPErrorKeepsWindowsEmpty(t *testing.T) {
	c := stubClient(t, &stubTransport{status: 401})
	res, err := c.Query(context.Background(), "codex", "acct", "tok", "")
	if err != nil {
		t.Fatalf("Query transport 错误: %v", err)
	}
	if res.Status != "error" {
		t.Fatalf("Status = %q, want error", res.Status)
	}
	if res.Error == "" {
		t.Fatal("Error 必须携带 HTTP 状态说明")
	}
	if len(res.Windows) != 0 {
		t.Fatalf("windows = %+v, want 空", res.Windows)
	}
}

// 网络失败：error 且 windows 空。
func TestQueryNetworkErrorKeepsWindowsEmpty(t *testing.T) {
	c := stubClient(t, &stubTransport{err: context.DeadlineExceeded})
	res, err := c.Query(context.Background(), "claude", "acct", "tok", "")
	if err != nil {
		t.Fatalf("Query transport 错误: %v", err)
	}
	if res.Status != "error" || len(res.Windows) != 0 {
		t.Fatalf("Status=%q windows=%+v, want error+空", res.Status, res.Windows)
	}
}
