//go:build windows

package singleinstance

import (
	"crypto/sha256"
	"errors"
	"fmt"
	"net"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/Microsoft/go-winio"
)

type Instance struct{ Listener net.Listener }

// 管道名由「当前用户身份 + 规范化配置命名空间」派生（方案 L2.2）：
//   - 用户隔离：不同 Windows 账户互不可见对方管道；
//   - 配置命名空间隔离：开发（~/.natives-local）与正式（~/.natives）各自独立；
//   - 旧固定管道 \\.\pipe\natives-model-host 不再静默兼容连接——升级时若存在
//     合法旧 worker，返回可操作的重启提示，不强杀（F7）。
const legacyPipeName = `\\.\pipe\natives-model-host`

func controlPipeName(dir string) (string, error) {
	userToken, err := userTokenName()
	if err != nil {
		return "", err
	}
	namespace := strings.TrimSpace(dir)
	if namespace == "" {
		home, homeErr := os.UserHomeDir()
		if homeErr != nil {
			return "", homeErr
		}
		namespace = filepath.Join(home, ".natives")
	}
	// 规范化路径等价形式：大小写不敏感文件系统下统一小写，消除长短/等价路径漂移。
	normalized := strings.ToLower(filepath.Clean(namespace))
	digest := sha256.Sum256([]byte("natives-model-host-pipe|" + userToken + "|" + normalized))
	return fmt.Sprintf(`\\.\pipe\natives-model-host-%x`, digest[:12]), nil
}

// userTokenName 返回当前用户身份标识（DOMAIN\USER），作为管道命名空间的
// 用户隔离维度。
func userTokenName() (string, error) {
	if domain, user := os.Getenv("USERDOMAIN"), os.Getenv("USERNAME"); domain != "" && user != "" {
		return domain + `\` + user, nil
	}
	user := os.Getenv("USERNAME")
	if user == "" {
		return "", errors.New("cannot determine current user identity for pipe namespace")
	}
	return user, nil
}

func Connect(dir string) (net.Conn, error) {
	pipeName, err := controlPipeName(dir)
	if err != nil {
		return nil, err
	}
	connection, dialErr := winio.DialPipe(pipeName, ptr(150*time.Millisecond))
	if dialErr == nil {
		return connection, nil
	}
	// 旧固定管道不可静默兼容：给出可操作的重启提示而非连接未知旧实例。
	if _, legacyErr := winio.DialPipe(legacyPipeName, ptr(150*time.Millisecond)); legacyErr == nil {
		return nil, errors.New("检测到旧版 Natives Model Host（固定管道）仍在运行，请退出该实例或重启 Natives 以完成升级")
	}
	return nil, dialErr
}

func Acquire(dir string) (*Instance, net.Conn, error) {
	pipeName, err := controlPipeName(dir)
	if err != nil {
		return nil, nil, err
	}
	if connection, dialErr := Connect(dir); dialErr == nil {
		return nil, connection, nil
	}
	listener, listenErr := winio.ListenPipe(pipeName, &winio.PipeConfig{
		// 仅授予管道属主完全控制：与按用户派生的管道名共同构成隔离（L2.2）。
		SecurityDescriptor: "D:P(A;;GA;;;OW)",
		MessageMode:        false,
		InputBufferSize:    1 << 20,
		OutputBufferSize:   1 << 20,
	})
	if listenErr == nil {
		return &Instance{Listener: listener}, nil, nil
	}
	// 竞争启动：短暂等待后重试连接既有 worker（与 Unix 行为一致）。
	connection, dialErr := winio.DialPipe(pipeName, ptr(time.Second))
	if dialErr == nil {
		return nil, connection, nil
	}
	return nil, nil, listenErr
}

func (i *Instance) Close() error {
	if i == nil || i.Listener == nil {
		return nil
	}
	return i.Listener.Close()
}

func ptr(value time.Duration) *time.Duration { return &value }
