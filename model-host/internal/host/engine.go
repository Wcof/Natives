package host

import (
	"context"
	"crypto/rand"
	"crypto/subtle"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	log "github.com/sirupsen/logrus"

	"github.com/ldh/natives/model-host/internal/authfiles"
	clipadapter "github.com/ldh/natives/model-host/internal/cliproxy"
	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/lifecycle"
	"github.com/ldh/natives/model-host/internal/nativeio"
	"github.com/ldh/natives/model-host/internal/quota"
	"github.com/ldh/natives/model-host/internal/secrets"
	"github.com/ldh/natives/model-host/internal/usage"
)

type Engine struct {
	inferenceMu         sync.Mutex
	inferenceJobs       map[string]*inferenceJob
	repo                *domain.Repository
	secrets             secrets.Store
	authStore           *clipadapter.AuthStore
	auth                clipadapter.LoginManager
	runtime             clipadapter.Runtime
	runtimeConfigPath   string
	emit                func(nativeio.Response)
	usageStore          *usage.Store
	usageCalculator     *usage.Calculator
	usageImporter       *usage.Importer
	usagePlugin         *usage.NativesUsagePlugin
	authFiles           *authfiles.Manager
	quotaClient         *quota.Client
	mu                  sync.Mutex
	gatewayMu           sync.Mutex
	secretMu            sync.Mutex
	kernelMu            sync.Mutex
	kernelJob           *kernelUpdateJob
	kernelFetchLatest   func(ctx context.Context) (string, error)
	kernelRunUpdate     func(job *kernelUpdateJob, target string)
	kernelIdentityCheck func() error
	kernelCtx           context.Context
	kernelCancel        context.CancelFunc
	sessions            map[string]context.CancelFunc
	oauthTimeout        time.Duration
	collectMu           sync.Mutex
	lastCollectAt       time.Time
	isCollecting        bool
	aiQuery             *aiQueryServer // ADR-0033 只读 AI 数据查询端口（绑定失败时为 nil）
	// tasks 登记采集等 Engine 后台任务：取消、共同 deadline 等待与 stopping
	// 判定原子（方案 L1）。
	tasks *lifecycle.Registry
	// closed 原子进入 stopping：Close 开始后 Handle 拒绝新请求，
	// 保证关闭存储后不再被请求路径访问（L4）。
	closed atomic.Bool
}

func NewEngine(repo *domain.Repository, secretStore secrets.Store, emit func(nativeio.Response)) (*Engine, error) {
	snapshot, err := repo.Load()
	if err != nil {
		return nil, err
	}
	ids := make([]string, 0, len(snapshot.Accounts))
	for _, account := range snapshot.Accounts {
		ids = append(ids, account.ID)
	}
	authStore := clipadapter.NewAuthStore(secretStore, ids, nil)
	authStore.OnAccountSaved(maskSecret, func(view clipadapter.AccountView) error { return syncAccountFromView(repo, emit, view) })
	authManager := clipadapter.NewLoginFlow(authStore)
	runtimeConfigPath, err := domain.DefaultRuntimeConfigPath()
	if err != nil {
		return nil, err
	}

	usageDBPath, err := usage.DefaultDBPath()
	if err != nil {
		return nil, err
	}
	usageStore, err := usage.NewStore(usageDBPath)
	if err != nil {
		return nil, err
	}
	usageCalc := usage.NewCalculator(usageStore)
	usageImp := usage.NewImporter(usageStore, usageCalc)

	type usageKey struct{ id, name, secret string }
	var usageKeyMu sync.RWMutex
	usageKeys := make([]usageKey, 0, len(snapshot.Gateway.AccessKeys)+1)
	reloadUsageKeys := func() {
		snap, loadErr := repo.Load()
		if loadErr != nil {
			return
		}
		keys := make([]usageKey, 0, len(snap.Gateway.AccessKeys)+1)
		for _, key := range snap.Gateway.AccessKeys {
			if secret, getErr := secretStore.Get(key.SecretRef); getErr == nil && secret != "" {
				keys = append(keys, usageKey{key.ID, key.Name, secret})
			}
		}
		if secret, getErr := secretStore.Get(gatewaySecretRef); getErr == nil && secret != "" {
			keys = append(keys, usageKey{"default", "默认密钥", secret})
		}
		usageKeyMu.Lock()
		usageKeys = keys
		usageKeyMu.Unlock()
	}
	resolveUsageKey := func(apiKey string) (string, string) {
		usageKeyMu.RLock()
		defer usageKeyMu.RUnlock()
		for _, key := range usageKeys {
			if subtle.ConstantTimeCompare([]byte(apiKey), []byte(key.secret)) == 1 {
				return key.id, key.name
			}
		}
		return "", ""
	}
	reloadUsageKeys()
	keyResolver := func(apiKey string) (string, string) {
		if id, name := resolveUsageKey(apiKey); id != "" {
			return id, name
		}
		reloadUsageKeys() // A rotated or newly-created key misses once, then remains memory-only.
		return resolveUsageKey(apiKey)
	}

	broadcastFunc := func(eventType string, data interface{}) {
		if emit != nil {
			emit(nativeio.Response{OK: true, Event: eventType, Result: data})
		}
	}

	usagePlg := usage.NewNativesUsagePlugin(usageStore, usageCalc, keyResolver, broadcastFunc)
	clipadapter.RegisterUsagePlugin("natives-usage", usagePlg)

	afManager, _ := authfiles.NewManager("")
	qClient := quota.NewClient()

	engine := &Engine{
		repo:              repo,
		secrets:           secretStore,
		authStore:         authStore,
		auth:              authManager,
		emit:              emit,
		usageStore:        usageStore,
		usageCalculator:   usageCalc,
		usageImporter:     usageImp,
		usagePlugin:       usagePlg,
		authFiles:         afManager,
		quotaClient:       qClient,
		sessions:          make(map[string]context.CancelFunc),
		runtimeConfigPath: runtimeConfigPath,
		oauthTimeout:      6 * time.Minute,
	}
	engine.kernelCtx, engine.kernelCancel = context.WithCancel(context.Background())
	engine.tasks = lifecycle.NewRegistry()
	clipadapter.SetModelRegistryHook(func(accountID string, models []clipadapter.ModelInfo) {
		catalogUpdate(engine, accountID, models)
	})
	// §7.5：不做启动隐式扫描、无常驻定时器（ADR-0030）。采集有两个事件驱动入口：
	// 显式 model_usage_collect 方法，与 Native Messaging 连接建立时的
	// TriggerCollectAsync（内置 10s 防抖），见收口方案 Phase 0.4。
	// ADR-0033：启动只读 AI 数据查询端口（127.0.0.1:8766），随 Engine 生命周期启停。
	engine.aiQuery = startAIQueryServer(engine)
	return engine, nil
}

func (e *Engine) Handle(ctx context.Context, request nativeio.Request) nativeio.Response {
	// Close 开始后拒绝新请求：不触碰即将关闭的存储（L4 stopping 语义）。
	if e.closed.Load() {
		return nativeio.Response{ID: request.ID, OK: false, Error: "模型服务正在退出", ErrorCode: "host_stopping"}
	}
	result, err := e.dispatch(ctx, request.Method, request.Params)
	if err == nil {
		switch request.Method {
		case "model_usage_budget_upsert", "model_usage_attention_ack", "model_usage_insight_dismiss":
			if e.emit != nil {
				e.emit(nativeio.Response{OK: true, Event: "model_usage_updated"})
			}
		}
		return nativeio.Response{ID: request.ID, OK: true, Result: result}
	}
	code, message := classify(err)
	return nativeio.Response{ID: request.ID, OK: false, Error: message, ErrorCode: code}
}

// Close 按 L4 统一顺序收口，全程共享一个绝对 deadline（非驻留 worker 从最后
// 合法连接关闭起 ≤1s 退出；本函数是其中最后一段，预算 800ms 留出连接收口）：
// 原子进入 stopping → 取消 Engine 任务并等待 → 撤销查询端口 → 取消 owner 任务
// → 停网关 → 停用量 producer 并排空已接收记录 → 关闭存储。
func (e *Engine) Close() {
	// 第一步：原子拒绝新请求（Handle 与重复 Close 快速返回）。
	if e.closed.Swap(true) {
		return
	}
	shutdown := lifecycle.NewShutdown(800 * time.Millisecond)
	if e.kernelCancel != nil {
		e.kernelCancel()
	}
	tasksCtx, tasksCancel := shutdown.Context()
	defer tasksCancel()
	stopResult := e.tasks.Stop(tasksCtx)
	if len(stopResult.TimedOut) > 0 {
		log.Warnf("engine close: tasks did not finish in deadline: %v", stopResult.TimedOut)
	}
	e.cancelInferenceAndSessions()
	if e.aiQuery != nil {
		e.aiQuery.Close()
	}
	clipadapter.ClearModelRegistryHook()
	gatewayCtx, gatewayCancel := shutdown.Context()
	stopErr := e.stopRuntime(gatewayCtx)
	gatewayCancel()
	pluginCtx, pluginCancel := shutdown.Context()
	if e.usagePlugin != nil {
		e.usagePlugin.Close(pluginCtx)
	}
	pluginCancel()
	if e.usageStore != nil {
		_ = e.usageStore.Close()
	}
	if stopErr != nil {
		log.Warnf("engine close: gateway stop did not finish within deadline: %v", stopErr)
	}
	snapshot, err := e.repo.Load()
	if err == nil && !snapshot.Gateway.Resident && snapshot.Gateway.State != "stopped" {
		_, _ = e.repo.Update(nil, resetGateway)
	}
}

// stopRuntime 在 gatewayMu 保护下停网关；持锁时间由 ctx 期限约束，
// 不在锁内做无限等待。Runtime.Stop 自身对未启动状态幂等返回 nil。
func (e *Engine) stopRuntime(ctx context.Context) error {
	e.gatewayMu.Lock()
	defer e.gatewayMu.Unlock()
	return e.runtime.Stop(ctx)
}

// cancelInferenceAndSessions 取消全部推理任务与 OAuth 会话（Engine 收口路径）。
func (e *Engine) cancelInferenceAndSessions() {
	e.inferenceMu.Lock()
	for id, j := range e.inferenceJobs {
		j.cancel()
		delete(e.inferenceJobs, id)
	}
	e.inferenceMu.Unlock()
	e.mu.Lock()
	for id, cancel := range e.sessions {
		cancel()
		delete(e.sessions, id)
	}
	e.mu.Unlock()
}

// CancelInferenceForOwners 只取消给定 owner（发起客户端）的推理任务；
// 其他客户端与 Engine 任务不受影响（L7）。
func (e *Engine) CancelInferenceForOwners(owners []string) {
	if len(owners) == 0 {
		return
	}
	set := make(map[string]struct{}, len(owners))
	for _, owner := range owners {
		set[owner] = struct{}{}
	}
	e.inferenceMu.Lock()
	defer e.inferenceMu.Unlock()
	for id, j := range e.inferenceJobs {
		if _, match := set[j.owner]; match {
			j.cancel()
			delete(e.inferenceJobs, id)
		}
	}
}

func (e *Engine) dispatch(ctx context.Context, method string, raw json.RawMessage) (any, error) {
	switch method {
	case "model_inference_options":
		return e.inferenceOptions()
	case "model_inference_start":
		return e.startInference(ctx, raw)
	case "model_inference_status":
		return e.inferenceStatus(raw, false)
	case "model_inference_cancel":
		return e.inferenceStatus(raw, true)
	case "model_snapshot", "model_gateway_status":
		snapshot, err := e.repo.Load()
		snapshot.Gateway.KernelVersion = getLocalKernelVersion()
		return snapshot, err
	case "model_collect_status":
		return e.collectStatus(), nil
	case "model_provider_create":
		return e.reconfigureGateway(e.createProvider(raw))
	case "model_provider_update":
		return e.reconfigureGateway(e.updateProvider(raw))
	case "model_provider_delete":
		return e.reconfigureGateway(e.deleteProvider(raw))
	case "model_provider_set_enabled":
		return e.reconfigureGateway(e.setProviderEnabled(raw))
	case "model_provider_test":
		return e.testProvider(ctx, raw)
	case "model_models_upsert":
		return e.reconfigureGateway(e.upsertModel(raw))
	case "model_models_delete":
		return e.reconfigureGateway(e.deleteModel(raw))
	case "model_models_set_enabled":
		return e.reconfigureGateway(e.setModelEnabled(raw))
	case "model_models_refresh":
		return e.reconfigureGateway(e.refreshModels(ctx, raw))
	case "model_default_model_set":
		return e.setDefaultModel(raw)
	case "model_gateway_set_resident":
		return e.setResident(raw)
	case "model_gateway_restart":
		return e.restartGateway(ctx, raw)
	case "model_gateway_settings_update":
		return e.updateGatewaySettings(raw)
	case "model_gateway_key_create":
		return e.createGatewayKey(raw)
	case "model_gateway_key_update":
		return e.updateGatewayKey(raw)
	case "model_gateway_key_delete":
		return e.deleteGatewayKey(raw)
	case "model_gateway_key_rotate", "model_gateway_rotate_access_key":
		return e.rotateGatewayKey(raw)
	case "model_gateway_key_reveal", "model_gateway_reveal_access_key":
		return e.revealGatewayKeyByID(raw)
	case "model_gateway_start":
		return e.startGateway(ctx, raw)
	case "model_gateway_stop":
		return e.stopGateway(ctx, raw)
	case "model_kernel_check_update":
		return e.checkKernelUpdate()
	case "model_kernel_update":
		return e.updateKernel(ctx)
	case "model_kernel_update_status":
		return e.kernelUpdateStatus()
	case "model_oauth_start", "model_account_reauth":
		return e.startOAuth(ctx, raw)
	case "model_oauth_cancel":
		return e.cancelOAuth(raw)
	case "model_account_set_enabled":
		return e.setAccountEnabled(raw)
	case "model_account_delete":
		return e.deleteAccount(raw)

	// Usage & Analytics methods
	case "model_usage_status":
		return e.getUsageStatus()
	case "model_usage_overview":
		return e.getUsageOverview(raw)
	case "model_usage_analysis":
		return e.getUsageAnalysis(raw)
	case "model_usage_sessions":
		return e.getUsageSessions(raw)
	case "model_usage_events":
		return e.getUsageEvents(raw)
	case "model_usage_pricing":
		return e.getUsagePricing()
	case "model_usage_sources":
		return e.getUsageSources()
	case "model_usage_budgets":
		return e.getUsageBudgets()
	case "model_usage_budget_upsert":
		return e.upsertUsageBudget(raw)
	case "model_usage_insights":
		return e.getUsageInsights()
	case "model_usage_insight_dismiss":
		return e.dismissUsageInsight(raw)
	case "model_tool_event_ingest":
		return e.ingestToolEvent(raw)
	case "model_usage_attention":
		return e.getUsageAttention(raw)
	case "model_usage_attention_ack":
		return e.ackUsageAttention(raw)
	case "model_usage_subjects":
		return e.getUsageSubjects(raw)
	case "model_usage_billing":
		return e.getUsageBilling(raw)
	case "model_usage_billing_entry_upsert":
		return e.upsertUsageBillingEntry(raw)
	case "model_usage_collect":
		return e.collectUsage(ctx)
	case "model_usage_price_upsert":
		return e.upsertUsagePrice(raw)
	case "model_usage_price_delete":
		return e.deleteUsagePrice(raw)
	case "model_usage_price_sync":
		return e.syncUsagePrice(ctx)
	case "model_usage_import_begin":
		return e.beginUsageImport(raw)
	case "model_usage_import_chunk":
		return e.chunkUsageImport(raw)
	case "model_usage_import_preview":
		return e.previewUsageImport(raw)
	case "model_usage_import_commit":
		return e.commitUsageImport(raw)
	case "model_usage_import_cancel":
		return e.cancelUsageImport(raw)

	// Auth Files and Quota Inquiry methods
	case "model_auth_files_list":
		return e.listAuthFiles()
	case "model_auth_files_import":
		return e.importAuthFile(raw)
	case "model_auth_files_update":
		return e.updateAuthFile(raw)
	case "model_auth_files_delete":
		return e.deleteAuthFile(raw)
	case "model_auth_files_open_dir":
		return e.openAuthDir()
	case "model_account_models":
		return e.accountModels(raw)
	case "model_account_models_update":
		return e.updateAccountModels(raw)
	case "model_quota_query":
		return e.queryQuota(ctx, raw)
	case "model_quota_cached":
		var params map[string]json.RawMessage
		if len(raw) > 0 && (json.Unmarshal(raw, &params) != nil || len(params) != 0) {
			return nil, invalid("额度缓存查询不接受参数")
		}
		return cachedQuotaResult(), nil

	// Agent clients (智能体配置)
	case "model_agent_clients_list":
		return e.listAgentClients()
	case "model_agent_clients_sync":
		return e.syncAgentClientConfigs(ctx)
	case "model_agent_client_models":
		return e.agentClientModels(ctx, raw)
	case "model_agent_client_apply":
		return e.applyAgentClientConfig(ctx, raw, "apply")
	case "model_agent_client_default":
		return e.applyAgentClientConfig(ctx, raw, "default")
	case "model_agent_client_close":
		return e.applyAgentClientConfig(ctx, raw, "close")
	case "model_agent_client_launch":
		return e.launchAgentClient(raw)
	case "model_agent_codex_auth_check", "model_agent_codex_clear",
		"model_agent_codex_sessions_list", "model_agent_codex_sessions_delete",
		"model_agent_pi_status", "model_agent_pi_action":
		return e.agentClientExtras(ctx, raw, method)

	default:
		return nil, invalid("不支持的模型设置操作")
	}
}

func expected(raw json.RawMessage) (*int64, error) {
	var value struct {
		ExpectedRevision *int64 `json:"expectedRevision"`
	}
	if len(raw) > 0 && string(raw) != "null" {
		if err := json.Unmarshal(raw, &value); err != nil {
			return nil, invalid("请求参数无效")
		}
	}
	if value.ExpectedRevision == nil {
		return nil, invalid("写操作缺少配置版本")
	}
	return value.ExpectedRevision, nil
}

func requireRevision(value *int64) error {
	if value == nil {
		return invalid("写操作缺少配置版本")
	}
	return nil
}

func randomID(prefix string) (string, error) {
	bytes := make([]byte, 16)
	if _, err := rand.Read(bytes); err != nil {
		return "", fmt.Errorf("generate id: %w", err)
	}
	return prefix + hex.EncodeToString(bytes), nil
}

func randomSecret() (string, error) {
	bytes := make([]byte, 32)
	if _, err := rand.Read(bytes); err != nil {
		return "", err
	}
	return "natives_" + hex.EncodeToString(bytes), nil
}

func maskSecret(value string) string {
	value = strings.TrimSpace(value)
	if len(value) <= 4 {
		return "••••"
	}
	return "••••" + value[len(value)-4:]
}

func findProvider(snapshot *domain.Snapshot, id string) (*domain.Provider, error) {
	for index := range snapshot.Providers {
		if snapshot.Providers[index].ID == id {
			return &snapshot.Providers[index], nil
		}
	}
	return nil, notFound()
}

func findAccount(snapshot *domain.Snapshot, id string) (*domain.Account, error) {
	for index := range snapshot.Accounts {
		if snapshot.Accounts[index].ID == id {
			return &snapshot.Accounts[index], nil
		}
	}
	return nil, notFound()
}

func (e *Engine) emitSnapshot(event string, snapshot domain.Snapshot) {
	if e.emit != nil {
		e.emit(nativeio.Response{OK: true, Event: event, Result: map[string]any{"snapshot": snapshot}})
	}
}

var errSecretMissing = errors.New("secret is not configured")

// collectStatus 返回事件驱动采集的只读状态，供性能门禁与诊断区分
// “采集进行中”与“采集完成后的稳态空闲”两个阶段（不触发任何采集）。
func (e *Engine) collectStatus() map[string]any {
	e.collectMu.Lock()
	defer e.collectMu.Unlock()
	status := map[string]any{"collecting": e.isCollecting}
	if !e.lastCollectAt.IsZero() {
		status["lastCollectAt"] = e.lastCollectAt.UTC().Format(time.RFC3339Nano)
	}
	return status
}

// TriggerCollectAsync 在后台异步触发一次本地多工具增量日志采集（事件驱动：
// Native Messaging 连接建立、设置打开等时机）。
// 内置防抖（进行中或距上次完成 <10s 跳过），不做常驻定时器（遵循 ADR-0030）。
// 任务经 lifecycle.Registry 登记：Engine 收口时取消并等待，不会在存储关闭后
// 继续访问。若有新事件导入，向所有客户端广播 model_usage_updated。
func (e *Engine) TriggerCollectAsync() {
	if e.usageStore == nil {
		return
	}
	e.collectMu.Lock()
	if e.isCollecting || time.Since(e.lastCollectAt) < 10*time.Second {
		e.collectMu.Unlock()
		return
	}
	e.isCollecting = true
	e.collectMu.Unlock()

	task := e.tasks.Go("usage-collect", func(ctx context.Context) {
		defer func() {
			e.collectMu.Lock()
			e.isCollecting = false
			e.lastCollectAt = time.Now()
			e.collectMu.Unlock()
		}()
		summary, err := e.usageStore.CollectSources(ctx)
		if err != nil || summary == nil || summary.EventsImported <= 0 || e.emit == nil {
			return
		}
		e.emit(nativeio.Response{
			OK:    true,
			Event: "model_usage_updated",
			Result: map[string]interface{}{
				"source":         "collector",
				"eventsImported": summary.EventsImported,
			},
		})
	})
	if task == nil {
		// Engine 正在收口：回滚防抖状态，保证状态诚实。
		e.collectMu.Lock()
		e.isCollecting = false
		e.collectMu.Unlock()
	}
}
