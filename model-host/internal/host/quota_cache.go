package host

import (
	"encoding/json"
	"os"
	"path/filepath"
	"sync"

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/quota"
)

// 额度缓存：扩展侧「刷新额度」成功后写入真实结果（model-host 是凭证的
// Keychain 持有者）；顶栏面板读取该缓存展示最近一次真实余额。
// 只写 success 结果，零假数据；仅跟随用户手动刷新，不做定时上游请求。

var quotaCacheMu sync.Mutex

func quotaCachePath() string {
	statePath, err := domain.DefaultStatePath()
	if err != nil {
		return ""
	}
	return filepath.Join(filepath.Dir(statePath), "quota-cache.json")
}

func saveQuotaCacheEntry(result *quota.QuotaResult) {
	if result == nil || result.Status != "success" {
		return
	}
	path := quotaCachePath()
	if path == "" {
		return
	}
	quotaCacheMu.Lock()
	defer quotaCacheMu.Unlock()

	cache := map[string]json.RawMessage{}
	if data, err := os.ReadFile(path); err == nil {
		_ = json.Unmarshal(data, &cache)
	}
	entry, err := json.Marshal(result)
	if err != nil {
		return
	}
	cache[result.Provider+"|"+result.Account] = entry
	data, err := json.Marshal(cache)
	if err != nil {
		return
	}
	_ = os.WriteFile(path, data, 0600)
}
