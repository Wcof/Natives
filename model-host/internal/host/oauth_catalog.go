package host

import (
	"slices"
	"strings"

	clipadapter "github.com/ldh/natives/model-host/internal/cliproxy"
	"github.com/ldh/natives/model-host/internal/domain"
)

// catalogUpdate 是模型目录的项目侧回调（mode/T07）：SDK hook 对象与
// ModelInfo→项目类型转换留在 cliproxy 适配层，这里只消费项目类型。
func catalogUpdate(engine *Engine, accountID string, models []clipadapter.ModelInfo) {
	current, err := engine.repo.Load()
	if err != nil {
		return
	}
	account, err := findAccount(&current, accountID)
	if err != nil || !account.Enabled {
		return
	}
	snapshot, err := engine.repo.Update(nil, func(snapshot *domain.Snapshot) error {
		account, findErr := findAccount(snapshot, accountID)
		if findErr != nil || !account.Enabled {
			return findErr
		}
		provider, findErr := findProvider(snapshot, account.Provider)
		if findErr != nil {
			return findErr
		}
		removeAccountModels(snapshot, accountID)
		for _, info := range models {
			id := strings.TrimSpace(info.ID)
			if id == "" || len(id) > 200 {
				continue
			}
			index := slices.IndexFunc(provider.Models, func(model domain.Model) bool { return model.ID == id })
			if index < 0 {
				if len(provider.Models) >= maxModelsPerProvider {
					break
				}
				display := strings.TrimSpace(info.DisplayName)
				if display == "" {
					display = id
				}
				contextLength := max(info.ContextLength, info.MaxContextLength, info.InputTokenLimit)
				provider.Models = append(provider.Models, domain.Model{ID: id, DisplayName: display, ContextLength: int64(contextLength), AccountIDs: []string{accountID}, Enabled: true})
				continue
			}
			provider.Models[index].AccountIDs = append(provider.Models[index].AccountIDs, accountID)
			slices.Sort(provider.Models[index].AccountIDs)
			provider.Models[index].AccountIDs = slices.Compact(provider.Models[index].AccountIDs)
		}
		return nil
	})
	if err == nil {
		engine.emitSnapshot("model_catalog_changed", snapshot)
	}
}

// Runtime restarts unregister and re-register the same account asynchronously.
// The next registration replaces that account's catalog; account disable/delete removes it explicitly
// （SDK 的 OnModelsUnregistered 空实现同样留在适配层）。

func removeAccountModels(snapshot *domain.Snapshot, accountID string) {
	for providerIndex := range snapshot.Providers {
		provider := &snapshot.Providers[providerIndex]
		if provider.Kind != "oauth" {
			continue
		}
		for modelIndex := range provider.Models {
			provider.Models[modelIndex].AccountIDs = slices.DeleteFunc(provider.Models[modelIndex].AccountIDs, func(id string) bool { return id == accountID })
		}
		provider.Models = slices.DeleteFunc(provider.Models, func(model domain.Model) bool { return len(model.AccountIDs) == 0 })
	}
}
