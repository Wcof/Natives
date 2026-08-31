package host

import (
	"errors"
	"strings"
)

type SafeError struct {
	Code    string
	Message string
}

func (e *SafeError) Error() string { return e.Message }

func invalid(message string) error { return &SafeError{Code: "invalid_request", Message: message} }
func notFound() error              { return &SafeError{Code: "not_found", Message: "未找到对应配置"} }

func classify(err error) (string, string) {
	var safe *SafeError
	if errors.As(err, &safe) {
		return safe.Code, safe.Message
	}
	if strings.Contains(err.Error(), "revision_conflict") {
		return "revision_conflict", "配置已在其他页面更新，请刷新后重试"
	}
	if strings.Contains(strings.ToLower(err.Error()), "keychain") {
		return "keychain_unavailable", "系统钥匙串当前不可用"
	}
	return "internal_error", "模型设置操作失败"
}
