package usage

// T6：macOS 系统通知适配器（ADR-0030 决策 8）。
//
// 固定程序 + 固定脚本模板 + 独立 argv：文本只作为 osascript 的独立参数传入，
// 绝不拼进 AppleScript 源码/shell 字符串（无注入面）。返回 queued/submitted/failed
// 三态——系统接受请求不等于用户已看到（ADR-0030 §6.5）。Spike 已验证
// /usr/bin/osascript display notification 路径（ADR-0030 Spike 记录）。

import (
	"context"
	"os/exec"
	"strings"
	"time"
)

// NotificationResult 为投递记录状态。
type NotificationResult string

const (
	NotificationQueued    NotificationResult = "queued"
	NotificationSubmitted NotificationResult = "submitted"
	NotificationFailed    NotificationResult = "failed"
)

// notifier 抽象系统调用以便测试注入。
type notifier func(ctx context.Context, title, body string) error

// osascriptNotifier 用固定 argv 调用 /usr/bin/osascript。脚本结构固定，
// body/title 作为 run handler 的独立 argv 传入——不经 shell、不进入脚本源码。
func osascriptNotifier(ctx context.Context, title, body string) error {
	cmd := exec.CommandContext(ctx, "/usr/bin/osascript",
		"-e", "on run argv",
		"-e", "display notification (item 1 of argv) with title (item 2 of argv)",
		"-e", "end run",
		body, title,
	)
	return cmd.Run()
}

// SendNotification 发送一条系统通知，返回投递状态（不宣称“已看到”）。
// 超时有界（3 秒）；失败不 panic、不阻塞调用方。
func SendNotification(title, body string) NotificationResult {
	return sendNotificationWith(context.Background(), osascriptNotifier, title, body)
}

func sendNotificationWith(ctx context.Context, n notifier, title, body string) NotificationResult {
	if strings.TrimSpace(title) == "" {
		title = "Natives"
	}
	cctx, cancel := context.WithTimeout(ctx, 3*time.Second)
	defer cancel()
	if err := n(cctx, title, body); err != nil {
		return NotificationFailed
	}
	return NotificationSubmitted
}
