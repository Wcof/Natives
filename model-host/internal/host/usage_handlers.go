package host

import (
	"context"
	"encoding/json"
	"os"
	"strings"
	"time"

	"github.com/ldh/natives/model-host/internal/usage"
)

func (e *Engine) getUsageStatus() (map[string]any, error) {
	if e.usageStore == nil {
		return nil, invalid("使用记录存储未就绪")
	}
	dbPath := e.usageStore.DBPath()
	count, oldest, newest, err := e.usageStore.GetStatus()
	if err != nil {
		return nil, err
	}
	var sizeBytes int64
	if info, statErr := os.Stat(dbPath); statErr == nil {
		sizeBytes = info.Size()
	}
	return map[string]any{
		"recordCount":    count,
		"dbSizeBytes":    sizeBytes,
		"oldestRecordAt": oldest,
		"newestRecordAt": newest,
	}, nil
}

func (e *Engine) getUsageOverview(raw json.RawMessage) (*usage.OverviewResult, error) {
	if e.usageStore == nil {
		return nil, invalid("使用记录存储未就绪")
	}
	var filter usage.Filter
	if len(raw) > 0 && string(raw) != "null" {
		if err := json.Unmarshal(raw, &filter); err != nil {
			return nil, invalid("查询过滤参数无效")
		}
	}
	if err := validateUsageFilter(filter); err != nil {
		return nil, err
	}
	return e.usageStore.GetOverview(filter)
}

func (e *Engine) getUsageAnalysis(raw json.RawMessage) (*usage.AnalyticsResult, error) {
	if e.usageStore == nil {
		return nil, invalid("使用记录存储未就绪")
	}
	var filter usage.Filter
	if len(raw) > 0 && string(raw) != "null" {
		if err := json.Unmarshal(raw, &filter); err != nil {
			return nil, invalid("透视分析参数无效")
		}
	}
	if err := validateUsageFilter(filter); err != nil {
		return nil, err
	}
	return e.usageStore.GetAnalytics(filter)
}

func (e *Engine) getUsageSessions(raw json.RawMessage) (*usage.SessionsResult, error) {
	if e.usageStore == nil {
		return nil, invalid("使用记录存储未就绪")
	}
	var filter usage.Filter
	if len(raw) > 0 && string(raw) != "null" {
		if err := json.Unmarshal(raw, &filter); err != nil {
			return nil, invalid("会话统计参数无效")
		}
	}
	if err := validateUsageFilter(filter); err != nil {
		return nil, err
	}
	return e.usageStore.GetSessions(filter)
}

func (e *Engine) getUsageEvents(raw json.RawMessage) (*usage.EventsResult, error) {
	if e.usageStore == nil {
		return nil, invalid("使用记录存储未就绪")
	}
	var filter usage.Filter
	if len(raw) > 0 && string(raw) != "null" {
		if err := json.Unmarshal(raw, &filter); err != nil {
			return nil, invalid("明细记录参数无效")
		}
	}
	if err := validateUsageFilter(filter); err != nil {
		return nil, err
	}
	result, err := e.usageStore.GetEvents(filter)
	if err != nil {
		return nil, err
	}
	// costStatus 读时判定：价格命中（含 0 价）为 priced，目录缺价为 unpriced，
	// 不让"未计价"与"合法零价"在 UI 中混淆（T0 契约修复）。
	if e.usageCalculator != nil {
		for i := range result.Events {
			if e.usageCalculator.FindPrice(result.Events[i].Provider, result.Events[i].Model) != nil {
				result.Events[i].CostStatus = "priced"
			} else {
				result.Events[i].CostStatus = "unpriced"
			}
		}
	}
	return result, nil
}

func validateUsageFilter(filter usage.Filter) error {
	switch filter.Range {
	case "", "4h", "24h", "72h", "today", "7d", "30d", "all":
	case "custom":
		if filter.StartTime == nil || filter.EndTime == nil || !filter.StartTime.Before(*filter.EndTime) {
			return invalid("自定义时间范围无效")
		}
	default:
		return invalid("使用记录时间范围无效")
	}
	// 整改 E2 §4.2：IANA 时区必须在白名单内核验；不识别的时区返回参数
	// 错误，不静默退回 UTC。
	if _, err := usage.ResolveTimezone(filter.Timezone); err != nil {
		return invalid("用户时区参数无效")
	}
	for _, values := range [][]string{filter.Models, filter.Providers, filter.Sources, filter.AccessKeyIDs} {
		if len(values) > 100 {
			return invalid("使用记录筛选项过多")
		}
		for _, value := range values {
			if len([]rune(value)) > 200 {
				return invalid("使用记录筛选项过长")
			}
		}
	}
	return nil
}

func (e *Engine) getUsagePricing() (*usage.PricingResult, error) {
	if e.usageCalculator == nil {
		return nil, invalid("价格计算器未就绪")
	}
	return e.usageCalculator.GetPricingCatalog()
}

func (e *Engine) upsertUsagePrice(raw json.RawMessage) (*usage.PricingResult, error) {
	if e.usageStore == nil || e.usageCalculator == nil {
		return nil, invalid("价格系统未就绪")
	}
	var price usage.Price
	if err := json.Unmarshal(raw, &price); err != nil {
		return nil, invalid("价格配置参数无效")
	}
	price.ModelID = strings.TrimSpace(price.ModelID)
	price.ProviderID = strings.TrimSpace(price.ProviderID)
	if price.ModelID == "" || len([]rune(price.ModelID)) > 160 || len([]rune(price.ProviderID)) > 80 {
		return nil, invalid("价格配置缺少模型标识")
	}
	for _, value := range []int64{price.InputPriceMicro, price.OutputPriceMicro, price.CacheReadPriceMicro, price.CacheWritePriceMicro} {
		if value < 0 || value > 1_000_000_000_000 {
			return nil, invalid("价格参数超出允许范围")
		}
	}
	price.Source = usage.PriceSourceManual
	price.UpdatedAt = time.Now().UTC().Format(time.RFC3339)
	if err := e.usageStore.UpsertCustomPrice(price); err != nil {
		return nil, err
	}
	return e.usageCalculator.GetPricingCatalog()
}

func (e *Engine) deleteUsagePrice(raw json.RawMessage) (*usage.PricingResult, error) {
	if e.usageStore == nil || e.usageCalculator == nil {
		return nil, invalid("价格系统未就绪")
	}
	var input struct {
		ProviderID string `json:"providerId"`
		ModelID    string `json:"modelId"`
		ID         string `json:"id"`
	}
	if err := json.Unmarshal(raw, &input); err != nil {
		return nil, invalid("删除价格参数无效")
	}
	if input.ID != "" {
		if err := e.usageStore.DeletePrice(input.ID); err != nil {
			return nil, err
		}
	} else if input.ModelID != "" {
		if err := e.usageStore.DeleteCustomPrice(input.ProviderID, input.ModelID); err != nil {
			return nil, err
		}
	} else {
		return nil, invalid("删除价格缺少标识")
	}
	return e.usageCalculator.GetPricingCatalog()
}

func (e *Engine) syncUsagePrice(ctx context.Context) (*usage.PricingResult, error) {
	if e.usageCalculator == nil {
		return nil, invalid("价格系统未就绪")
	}
	if err := e.usageCalculator.SyncRemoteCatalog(ctx); err != nil {
		return nil, err
	}
	return e.usageCalculator.GetPricingCatalog()
}

func (e *Engine) beginUsageImport(raw json.RawMessage) (map[string]any, error) {
	if e.usageImporter == nil {
		return nil, invalid("导入服务未就绪")
	}
	var input struct {
		FileName   string `json:"fileName"`
		FileSize   int64  `json:"fileSize"`
		ImportKind string `json:"importKind,omitempty"` // usage_events（默认）| billing_csv
		Provider   string `json:"provider,omitempty"`
		Account    string `json:"account,omitempty"`
	}
	if err := json.Unmarshal(raw, &input); err != nil || input.FileName == "" || input.FileSize <= 0 {
		return nil, invalid("导入会话参数无效")
	}
	session, err := e.usageImporter.BeginSessionWithKind(
		input.ImportKind, input.FileName, input.FileSize, input.Provider, input.Account)
	if err != nil {
		return nil, invalid(err.Error())
	}
	return map[string]any{
		"sessionId":  session.ID,
		"chunkSize":  usage.ImportChunkSize,
		"importKind": session.ImportKind(),
	}, nil
}

func (e *Engine) chunkUsageImport(raw json.RawMessage) (map[string]any, error) {
	if e.usageImporter == nil {
		return nil, invalid("导入服务未就绪")
	}
	var input struct {
		SessionID       string `json:"sessionId"`
		ChunkIndex      int    `json:"chunkIndex"`
		ChunkDataBase64 string `json:"chunkDataBase64"`
	}
	if err := json.Unmarshal(raw, &input); err != nil || input.SessionID == "" || input.ChunkDataBase64 == "" {
		return nil, invalid("分块上传参数无效")
	}
	session, err := e.usageImporter.AppendChunk(input.SessionID, input.ChunkIndex, input.ChunkDataBase64)
	if err != nil {
		return nil, err
	}
	return map[string]any{
		"receivedBytes": session.ReceivedSize,
		"totalChunks":   session.TotalChunks,
	}, nil
}

func (e *Engine) previewUsageImport(raw json.RawMessage) (*usage.ImportPreviewResult, error) {
	if e.usageImporter == nil {
		return nil, invalid("导入服务未就绪")
	}
	var input struct {
		SessionID string `json:"sessionId"`
	}
	if err := json.Unmarshal(raw, &input); err != nil || input.SessionID == "" {
		return nil, invalid("预览请求缺少会话标识")
	}
	return e.usageImporter.PreviewSession(input.SessionID)
}

func (e *Engine) commitUsageImport(raw json.RawMessage) (map[string]any, error) {
	if e.usageImporter == nil {
		return nil, invalid("导入服务未就绪")
	}
	var input struct {
		SessionID string `json:"sessionId"`
	}
	if err := json.Unmarshal(raw, &input); err != nil || input.SessionID == "" {
		return nil, invalid("提交请求缺少会话标识")
	}
	count, err := e.usageImporter.CommitSession(input.SessionID)
	if err != nil {
		return nil, err
	}
	// §7.3：账单 commit 是续费提醒的评估入口之一——导入带到期日的
	// 订阅/预付条目后立即评估 7/3/1/0 天窗口（幂等，不重复入箱）。
	if e.usageStore != nil {
		if _, evalErr := e.usageStore.EvaluateRenewals(usage.RenewalOpts{SuppressAlerts: false}); evalErr != nil {
			return map[string]any{"importedCount": count}, evalErr
		}
	}
	return map[string]any{
		"importedCount": count,
	}, nil
}

// upsertUsageBillingEntry 手动录入经用户确认的账务事实（§7.2）：订阅、
// API 扣款、充值、credits 变化、退款、折扣、税费、预付余额到期。
// evidence 强制 user_confirmed；充值不得携带服务消耗（kindImpact 校验）。
func (e *Engine) upsertUsageBillingEntry(raw json.RawMessage) (map[string]any, error) {
	if e.usageStore == nil {
		return nil, invalid("使用记录存储未就绪")
	}
	var entry usage.BillingEntry
	if err := json.Unmarshal(raw, &entry); err != nil {
		return nil, invalid("账务条目参数无效")
	}
	if err := e.usageStore.InsertManualBillingEntry(&entry); err != nil {
		return nil, invalid(err.Error())
	}
	// 手动条目可能带订阅/预付到期日：立即评估续费窗口。
	renewals, err := e.usageStore.EvaluateRenewals(usage.RenewalOpts{SuppressAlerts: false})
	if err != nil {
		return map[string]any{"ok": true, "entryId": entry.ID}, err
	}
	return map[string]any{"ok": true, "entryId": entry.ID, "renewals": renewals}, nil
}

func (e *Engine) cancelUsageImport(raw json.RawMessage) (map[string]any, error) {
	if e.usageImporter == nil {
		return nil, invalid("导入服务未就绪")
	}
	var input struct {
		SessionID string `json:"sessionId"`
	}
	if err := json.Unmarshal(raw, &input); err != nil || input.SessionID == "" {
		return nil, invalid("取消请求缺少会话标识")
	}
	e.usageImporter.CancelSession(input.SessionID)
	return map[string]any{
		"cancelled": true,
	}, nil
}
