// 项目侧授权记录视图与意图操作（mode/T07，A22 / R-M7）。
//
// SDK（coreauth.Auth、clipcore.ModelInfo 等）的类型与状态转换止于本适配
// 包；Host 业务只见 AccountView／ModelInfo／ModelDefinition 等项目类型与
// 下面声明的意图操作。明文 token 只在本包内读写 Keychain/元数据，
// 经回调给宿主的只有掩码；不建立逐字段镜像，不新增明文旁路。
package cliproxy

import (
	"context"
	"encoding/json"
	"errors"
	"strings"

	sdkauth "github.com/router-for-me/CLIProxyAPI/v8/sdk/auth"
	clipcore "github.com/router-for-me/CLIProxyAPI/v8/sdk/cliproxy"
	coreauth "github.com/router-for-me/CLIProxyAPI/v8/sdk/cliproxy/auth"
	cliproxyusage "github.com/router-for-me/CLIProxyAPI/v8/sdk/cliproxy/usage"
	clipconfig "github.com/router-for-me/CLIProxyAPI/v8/sdk/config"
)

// AccountView 是授权记录的项目侧只读视图（不含任何明文 token）。
type AccountView struct {
	ID                 string
	Provider           string
	Label              string
	Status             string
	Disabled           bool
	LastErrorRetryable bool
	ExcludedModels     []string
	Mask               string
}

// ModelDefinition 是 SDK 静态模型目录的项目侧条目。
type ModelDefinition struct {
	ID          string
	DisplayName string
}

// ModelInfo 是运行时注册模型的项目侧条目（SDK ModelInfo 的字段映射）。
type ModelInfo struct {
	ID               string
	DisplayName      string
	ContextLength    int
	MaxContextLength int
	InputTokenLimit  int
}

// StaticModelDefinitions 转发 SDK 静态目录并转换为项目类型。
func StaticModelDefinitions(provider string) []ModelDefinition {
	definitions := clipcore.StaticModelDefinitions(provider)
	result := make([]ModelDefinition, 0, len(definitions))
	for _, definition := range definitions {
		if definition == nil || definition.ID == "" {
			continue
		}
		result = append(result, ModelDefinition{ID: definition.ID, DisplayName: definition.DisplayName})
	}
	return result
}

// LoginManager 是登录流程的项目侧声明（mode/T07 审计更正，A22）：SDK
// 类型不再出现在公开签名——宿主只见 AccountView 与清理函数；登录配置、
// LoginOptions、凭据重指与掩码提取全部内聚在实现（LoginFlow）内，
// 测试可注入同签名假实现。
type LoginManager interface {
	Login(ctx context.Context, provider, targetAccountID string, mask func(string) string) (AccountView, func(context.Context) error, error)
}

// 项目侧状态常量（coreauth.Status* 的适配层映射）。
const (
	AccountStatusActive   = string(coreauth.StatusActive)
	AccountStatusDisabled = string(coreauth.StatusDisabled)
	AccountStatusError    = string(coreauth.StatusError)
)

// 登录重指阶段的哨兵错误：宿主映射为既有稳定 errorCode。
var (
	ErrCredentialReplaceFailed = errors.New("credential_replace_failed")
	ErrCredentialCleanupFailed = errors.New("credential_cleanup_failed")
)

// sdkLoginManager 抽象 SDK 登录管理器（测试可注入桩实现）。
type sdkLoginManager interface {
	Login(ctx context.Context, provider string, cfg *clipconfig.Config, opts *sdkauth.LoginOptions) (*coreauth.Auth, string, error)
}

// LoginFlow 装配 SDK 登录管理器与其配置（组合根经 NewLoginFlow 构造）。
type LoginFlow struct {
	store   *AuthStore
	manager sdkLoginManager
	config  *clipconfig.Config
}

// NewLoginFlow 以当前 AuthStore 装配登录管理器（供应商集合与既有装配一致）。
func NewLoginFlow(store *AuthStore) *LoginFlow {
	return &LoginFlow{
		store: store,
		manager: sdkauth.NewManager(store,
			sdkauth.NewCodexAuthenticator(),
			sdkauth.NewClaudeAuthenticator(),
			sdkauth.NewAntigravityAuthenticator(),
			sdkauth.NewKimiAuthenticator(),
			sdkauth.NewXAIAuthenticator(),
		),
		config: &clipconfig.Config{Host: "127.0.0.1"},
	}
}

// Login 实现 LoginManager：执行 SDK 登录并在需要时把新凭据重指到既存
// 账户（Clone／ID 重指／持久化／临时记录清理留在适配层）。mask 由宿主
// 注入用于访问令牌脱敏；SDK 类型不出本方法签名。
func (f *LoginFlow) Login(ctx context.Context, provider, targetAccountID string, mask func(string) string) (AccountView, func(context.Context) error, error) {
	record, _, err := f.manager.Login(ctx, provider, f.config, &sdkauth.LoginOptions{NoBrowser: false})
	if err != nil {
		return AccountView{}, nil, err
	}
	if targetAccountID != "" && record.ID != targetAccountID {
		temporaryID := record.ID
		reassigned := record.Clone()
		reassigned.ID = targetAccountID
		if _, saveErr := f.store.Save(ctx, reassigned); saveErr != nil {
			_ = f.store.Delete(ctx, temporaryID)
			return AccountView{}, nil, ErrCredentialReplaceFailed
		}
		if deleteErr := f.store.Delete(ctx, temporaryID); deleteErr != nil {
			return AccountView{}, nil, ErrCredentialCleanupFailed
		}
		record = reassigned
	}
	view := viewFromAuth(record, mask)
	id := record.ID
	cleanup := func(ctx context.Context) error { return f.store.Delete(ctx, id) }
	return view, cleanup, nil
}

// ---- 注册类包装（组合根不再直接触碰 SDK 全局注册点） ----

// SetModelRegistryHook 注册项目侧模型目录回调；SDK 原生 hook 对象与
// ModelInfo 转换留在本包。
func SetModelRegistryHook(hook func(accountID string, models []ModelInfo)) {
	if hook == nil {
		clipcore.SetGlobalModelRegistryHook(nil)
		return
	}
	clipcore.SetGlobalModelRegistryHook(modelRegistryHookFunc(hook))
}

// ClearModelRegistryHook 注销模型目录回调。
func ClearModelRegistryHook() { clipcore.SetGlobalModelRegistryHook(nil) }

type modelRegistryHookFunc func(accountID string, models []ModelInfo)

func (f modelRegistryHookFunc) OnModelsRegistered(_ context.Context, _ string, accountID string, models []*clipcore.ModelInfo) {
	converted := make([]ModelInfo, 0, len(models))
	for _, info := range models {
		if info == nil {
			continue
		}
		converted = append(converted, ModelInfo{ID: info.ID, DisplayName: info.DisplayName, ContextLength: info.ContextLength, MaxContextLength: info.MaxContextLength, InputTokenLimit: info.InputTokenLimit})
	}
	f(accountID, converted)
}

func (modelRegistryHookFunc) OnModelsUnregistered(context.Context, string, string) {}

// RegisterUsagePlugin 注册用量插件（组合根装配入口）。
func RegisterUsagePlugin(name string, plugin cliproxyusage.Plugin) {
	cliproxyusage.RegisterNamedPlugin(name, plugin)
}

// CompiledKernelVersion 读取当前编译内核版本（kernel_handlers 的唯一取数口）。
func CompiledKernelVersion() string { return clipcore.NativesKernelVersion }

// ---- AuthStore 视图与意图操作 ----

func viewFromAuth(auth *coreauth.Auth, mask func(string) string) AccountView {
	view := AccountView{ID: auth.ID, Provider: auth.Provider, Label: auth.Label, Status: string(auth.Status), Disabled: auth.Disabled}
	if auth.LastError != nil {
		view.LastErrorRetryable = auth.LastError.Retryable
	}
	view.ExcludedModels = excludedModelsOf(auth)
	if mask != nil {
		if token, ok := auth.Metadata["access_token"].(string); ok {
			view.Mask = mask(token)
		}
	}
	return view
}

func excludedModelsOf(auth *coreauth.Auth) []string {
	if value := auth.Attributes["excluded_models"]; strings.TrimSpace(value) != "" {
		return splitCSV(value)
	}
	// 旧记录把规则写在 Metadata；读取保持既有回退口径。
	raw, _ := auth.Metadata["excluded_models"].([]any)
	result := make([]string, 0, len(raw))
	for _, value := range raw {
		if model, ok := value.(string); ok {
			result = append(result, model)
		}
	}
	if typed, ok := auth.Metadata["excluded_models"].([]string); ok {
		result = append(result, typed...)
	}
	return result
}

func splitCSV(value string) []string {
	parts := strings.Split(value, ",")
	result := make([]string, 0, len(parts))
	for _, part := range parts {
		if strings.TrimSpace(part) != "" {
			result = append(result, part)
		}
	}
	return result
}

func joinCSV(rules []string) string { return strings.Join(rules, ",") }

// AccountViews 返回全部授权记录的项目视图（不含明文）。
func (s *AuthStore) AccountViews(ctx context.Context) ([]AccountView, error) {
	credentials, err := s.List(ctx)
	if err != nil {
		return nil, err
	}
	views := make([]AccountView, 0, len(credentials))
	for _, credential := range credentials {
		views = append(views, viewFromAuth(credential, nil))
	}
	return views, nil
}

func findAuth(credentials []*coreauth.Auth, id string) (*coreauth.Auth, bool) {
	for _, credential := range credentials {
		if credential.ID == id {
			return credential, true
		}
	}
	return nil, false
}

var errCredentialNotFound = errors.New("credential_not_found")

// SwapAccountDisabled 设置账户停用状态并持久化；返回恢复原记录的补偿函数
// （宿主在后续步骤失败时调用，与既有补偿语义逐字段一致）。
func (s *AuthStore) SwapAccountDisabled(ctx context.Context, id string, disabled bool) (func(context.Context) error, error) {
	credentials, err := s.List(ctx)
	if err != nil {
		return nil, err
	}
	original, ok := findAuth(credentials, id)
	if !ok {
		return nil, errCredentialNotFound
	}
	updated := original.Clone()
	updated.Disabled = disabled
	if disabled {
		updated.Status = coreauth.StatusDisabled
	} else {
		updated.Status = coreauth.StatusActive
	}
	if _, err = s.Save(ctx, updated); err != nil {
		return nil, err
	}
	return func(ctx context.Context) error { _, serr := s.Save(ctx, original); return serr }, nil
}

// SwapAccountExcludedModels 更新账户模型排除规则并持久化；返回恢复原记录的
// 补偿函数（与既有 updateAccountModels 失败回滚语义一致）。
func (s *AuthStore) SwapAccountExcludedModels(ctx context.Context, id string, rules []string) (func(context.Context) error, error) {
	credentials, err := s.List(ctx)
	if err != nil {
		return nil, err
	}
	original, ok := findAuth(credentials, id)
	if !ok {
		return nil, errCredentialNotFound
	}
	updated := original.Clone()
	if updated.Metadata == nil {
		updated.Metadata = make(map[string]any)
	}
	updated.Metadata["excluded_models"] = rules
	if updated.Attributes == nil {
		updated.Attributes = make(map[string]string)
	}
	if len(rules) == 0 {
		delete(updated.Attributes, "excluded_models")
	} else {
		updated.Attributes["excluded_models"] = joinCSV(rules)
	}
	if _, err = s.Save(ctx, updated); err != nil {
		return nil, err
	}
	return func(ctx context.Context) error { _, serr := s.Save(ctx, original); return serr }, nil
}

// OAuthQuotaCredential 为额度查询取 OAuth 记录的短时令牌与附加字段；
// token 只返回给 Model Host 进程内的额度客户端（R-S4：不落日志／DTO）。
func (s *AuthStore) OAuthQuotaCredential(ctx context.Context, provider, accountID string) (token, extraJSON, accountLabel, matchedID string, err error) {
	credentials, listErr := s.List(ctx)
	if listErr != nil {
		return "", "", "", "", listErr
	}
	for _, credential := range credentials {
		if accountID != "" && credential.ID != accountID {
			continue
		}
		if accountID == "" && !strings.EqualFold(credential.Provider, provider) {
			continue
		}
		token, _ = credential.Metadata["access_token"].(string)
		projectID, _ := credential.Metadata["project_id"].(string)
		accountLabel = credential.Label
		matchedID = credential.ID
		if projectID == "" {
			return token, "", accountLabel, matchedID, nil
		}
		project, marshalErr := json.Marshal(map[string]string{"project_id": projectID})
		if marshalErr != nil {
			return token, "", accountLabel, matchedID, nil
		}
		return token, string(project), accountLabel, matchedID, nil
	}
	return "", "", accountLabel, "", nil
}

// RefreshToken 读取用于上游换发的 refresh token（进程内短时使用）。
func (s *AuthStore) RefreshToken(ctx context.Context, id string) (string, error) {
	credentials, err := s.List(ctx)
	if err != nil {
		return "", err
	}
	credential, ok := findAuth(credentials, id)
	if !ok {
		return "", errCredentialNotFound
	}
	refreshToken, _ := credential.Metadata["refresh_token"].(string)
	return refreshToken, nil
}

// SaveRefreshedTokens 将上游换发结果合并进凭据元数据并持久化。
func (s *AuthStore) SaveRefreshedTokens(ctx context.Context, id string, values map[string]any) error {
	credentials, err := s.List(ctx)
	if err != nil {
		return err
	}
	credential, ok := findAuth(credentials, id)
	if !ok {
		return errCredentialNotFound
	}
	updated := credential.Clone()
	if updated.Metadata == nil {
		updated.Metadata = make(map[string]any)
	}
	for key, value := range values {
		updated.Metadata[key] = value
	}
	_, err = s.Save(ctx, updated)
	return err
}

// DeleteAccount 删除凭据并返回补偿函数（补偿由宿主在后续步骤失败时调用）。
func (s *AuthStore) DeleteAccount(ctx context.Context, id string) (undo func(context.Context) error, err error) {
	credentials, listErr := s.List(ctx)
	if listErr != nil {
		return nil, listErr
	}
	restored, ok := findAuth(credentials, id)
	if deleteErr := s.Delete(ctx, id); deleteErr != nil {
		return nil, deleteErr
	}
	if !ok {
		return func(context.Context) error { return nil }, nil
	}
	return func(ctx context.Context) error { _, serr := s.Save(ctx, restored); return serr }, nil
}

// SyncAccount 把存储中的既存凭据重新同步进运行时（宿主持有 id 而非 SDK 对象）。
func (r *Runtime) SyncAccount(ctx context.Context, store *AuthStore, id string) error {
	credentials, err := store.List(ctx)
	if err != nil {
		return err
	}
	credential, ok := findAuth(credentials, id)
	if !ok {
		return errCredentialNotFound
	}
	return r.SyncAuth(ctx, credential)
}

// OnAccountSaved 注册项目侧保存回调（SDK Auth→AccountView 转换在本包内）。
// mask 由宿主注入；视图只带掩码，不带明文。
func (s *AuthStore) OnAccountSaved(mask func(string) string, cb func(AccountView) error) {
	s.SetOnSave(func(auth *coreauth.Auth) error { return cb(viewFromAuth(auth, mask)) })
}
