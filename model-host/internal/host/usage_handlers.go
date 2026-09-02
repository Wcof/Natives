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
	return e.usageStore.GetEvents(filter)
}

func validateUsageFilter(filter usage.Filter) error {
	switch filter.Range {
	case "", "4h", "24h", "today", "7d", "30d", "all":
	case "custom":
		if filter.StartTime == nil || filter.EndTime == nil || !filter.StartTime.Before(*filter.EndTime) {
			return invalid("自定义时间范围无效")
		}
	default:
		return invalid("使用记录时间范围无效")
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
		FileName string `json:"fileName"`
		FileSize int64  `json:"fileSize"`
	}
	if err := json.Unmarshal(raw, &input); err != nil || input.FileName == "" || input.FileSize <= 0 {
		return nil, invalid("导入会话参数无效")
	}
	session, err := e.usageImporter.BeginSession(input.FileName, input.FileSize)
	if err != nil {
		return nil, err
	}
	return map[string]any{
		"sessionId": session.ID,
		"chunkSize": usage.ImportChunkSize,
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
	return map[string]any{
		"importedCount": count,
	}, nil
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
