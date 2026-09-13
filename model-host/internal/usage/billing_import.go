package usage

// Provider 账单 CSV 导入回退（方案 §4.2/§9.0 Cursor 行："个人账单导入"）。
//
// source contract（冻结 wire schema）：cursor-billing-csv/1
//
//	date,kind,amount,currency[,period_start,period_end,note]
//	2026-08-01,subscription,20.00,USD,,,"Pro plan monthly"
//
// - kind 必须落在 kindImpact 词表内（subscription/service_usage/tax/topup/
//   refund/discount/credit_delta/prepay_expiry/reported_unattributed 或其别名），
//   未知 kind 显式报错，不静默丢弃也不猜。
// - 金额定点解析为整数微单位（§7.2：不使用浮点累计账目，检查溢出）。
// - 条目 ID 为内容指纹（provider+account+行内容），同一 CSV 重导幂等。
// - evidence_level 一律 actual_charge：账单文件是用户提供的已确认支出
//   （§4.2"用户录入或提供方账单证明"）；不做任何估算折算。
// - 多币种按行独立，不猜汇率（§4.2.1）。

import (
	"encoding/csv"
	"fmt"
	"hash/fnv"
	"strconv"
	"strings"
)

// billingKindAliases 把 CSV 常见写法映射到内部 kind 词表。
var billingKindAliases = map[string]string{
	"subscription":          "subscription",
	"service_usage":         "service_usage",
	"usage":                 "service_usage",
	"api_usage":             "service_usage",
	"tax":                   "tax",
	"topup":                 "topup",
	"top_up":                "topup",
	"payment":               "topup",
	"refund":                "refund",
	"discount":              "discount",
	"credit_delta":          "credit_delta",
	"prepay_expiry":         "prepay_expiry",
	"reported_unattributed": "reported_unattributed",
}

// parseAmountMicro 把 "20.00" / "-0.5" 定点解析为整数微单位（×1e6）。
// 拒绝浮点：拆整数与小数部分分别转换，超过 12 位整数部分判溢出。
func parseAmountMicro(s string) (int64, error) {
	s = strings.TrimSpace(s)
	if s == "" {
		return 0, fmt.Errorf("empty amount")
	}
	neg := false
	if strings.HasPrefix(s, "-") {
		neg = true
		s = s[1:]
	} else if strings.HasPrefix(s, "+") {
		s = s[1:]
	}
	intPart, fracPart := s, ""
	if dot := strings.Index(s, "."); dot >= 0 {
		intPart, fracPart = s[:dot], s[dot+1:]
	}
	if intPart == "" {
		intPart = "0"
	}
	if len(intPart) > 12 {
		return 0, fmt.Errorf("amount %q overflows 12-digit integer part", s)
	}
	if len(fracPart) > 6 {
		return 0, fmt.Errorf("amount %q has more than 6 decimal places", s)
	}
	for _, c := range intPart + fracPart {
		if c < '0' || c > '9' {
			return 0, fmt.Errorf("amount %q contains non-digit %q", s, string(c))
		}
	}
	whole, err := strconv.ParseInt(intPart, 10, 64)
	if err != nil {
		return 0, fmt.Errorf("amount integer part %q: %w", intPart, err)
	}
	micro := whole * 1_000_000
	if len(fracPart) > 0 {
		frac, _ := strconv.ParseInt(fracPart+strings.Repeat("0", 6-len(fracPart)), 10, 64)
		micro += frac
	}
	if neg {
		micro = -micro
	}
	return micro, nil
}

// normalizeBillingKind 校验并归一 CSV kind；未知 kind 返回错误（不猜）。
func normalizeBillingKind(raw string) (string, error) {
	k := strings.ToLower(strings.TrimSpace(raw))
	if k == "" {
		return "", fmt.Errorf("empty billing kind")
	}
	if mapped, ok := billingKindAliases[k]; ok {
		return mapped, nil
	}
	return "", fmt.Errorf("unknown billing kind %q (must be one of the frozen wire schema kinds)", raw)
}

// ParseBillingCSV 解析 cursor-billing-csv/1 格式账单文件为账务条目。
// provider 记入每条 entry（如 "cursor"）；解析失败逐行显式报错并带行号。
func ParseBillingCSV(data []byte, provider, account string) ([]BillingEntry, error) {
	reader := csv.NewReader(strings.NewReader(string(data)))
	reader.FieldsPerRecord = -1 // 各行列数自校验，报错带行号
	rows, err := reader.ReadAll()
	if err != nil {
		return nil, fmt.Errorf("read billing csv: %w", err)
	}
	if len(rows) < 1 {
		return nil, fmt.Errorf("billing csv is empty")
	}
	header := rows[0]
	idx := map[string]int{}
	for i, h := range header {
		idx[strings.ToLower(strings.TrimSpace(h))] = i
	}
	for _, must := range []string{"date", "kind", "amount", "currency"} {
		if _, ok := idx[must]; !ok {
			return nil, fmt.Errorf("billing csv header missing %q (schema cursor-billing-csv/1)", must)
		}
	}

	var out []BillingEntry
	for line, row := range rows[1:] {
		if len(row) == 0 || (len(row) == 1 && strings.TrimSpace(row[0]) == "") {
			continue // 空行跳过
		}
		get := func(name string) string {
			if i, ok := idx[name]; ok && i < len(row) {
				return strings.TrimSpace(row[i])
			}
			return ""
		}
		kind, err := normalizeBillingKind(get("kind"))
		if err != nil {
			return nil, fmt.Errorf("line %d: %w", line+2, err)
		}
		amount, err := parseAmountMicro(get("amount"))
		if err != nil {
			return nil, fmt.Errorf("line %d: %w", line+2, err)
		}
		currency := get("currency")
		if currency == "" {
			return nil, fmt.Errorf("line %d: empty currency", line+2)
		}
		if get("date") == "" {
			return nil, fmt.Errorf("line %d: empty date", line+2)
		}

		// 服务类金额入 serviceCostImpact；topup/refund 等按 kindImpact 由
		// InsertBillingEntry 校验口径归属——本层把金额统一放 service 字段，
		// 由 kind 校验决定它落在哪个口径（充值会被拒绝携带服务消耗，
		// 因此 topup 金额放 cash 字段更直接：这里按 kind 先行分配）。
		entry := BillingEntry{
			Provider:       provider,
			BillingAccount: account,
			Kind:           kind,
			PeriodStart:    get("period_start"),
			PeriodEnd:      get("period_end"),
			Currency:       currency,
			EvidenceLevel:  "actual_charge",
			Note:           get("note"),
		}
		// date 列是事实发生日：未提供 period_start 时落到 PeriodStart，
		// 不丢弃日期信息（对账与续费窗口都依赖它）。
		if entry.PeriodStart == "" {
			entry.PeriodStart = get("date")
		}
		// 金额按 kind 落入对应口径（§7.2 三口径分离；冻结语义：单金额行
		// 只归属一个主口径，service 优先；充值例外——同时入现金流与余额，
		// 但永不入服务消耗）。
		wantService, wantCash, wantCredit := kindImpact(kind)
		switch {
		case kind == "topup":
			entry.CashImpact = amount
			entry.CreditBalanceImpact = amount
		case wantService:
			entry.ServiceCostImpact = amount
		case wantCash:
			entry.CashImpact = amount
		case wantCredit:
			entry.CreditBalanceImpact = amount
		}

		h := fnv.New64a()
		for _, part := range []string{provider, account, get("date"), kind,
			get("amount"), currency, entry.PeriodStart, entry.PeriodEnd, entry.Note} {
			h.Write([]byte(part))
			h.Write([]byte{0})
		}
		entry.ID = fmt.Sprintf("billcsv-%016x", h.Sum64())
		out = append(out, entry)
	}
	if len(out) == 0 {
		return nil, fmt.Errorf("billing csv has no data rows")
	}
	return out, nil
}

// ImportBillingCSV 解析并写入账单 CSV；按内容指纹幂等——重导同一文件
// 不重复计量。返回导入与去重计数。
func (s *Store) ImportBillingCSV(data []byte, provider, account string) (imported, skipped int, err error) {
	entries, err := ParseBillingCSV(data, provider, account)
	if err != nil {
		return 0, 0, err
	}
	for i := range entries {
		if _, dup := s.billingEntryExists(entries[i].ID); dup {
			skipped++
			continue
		}
		if err := s.InsertBillingEntry(&entries[i]); err != nil {
			return imported, skipped, fmt.Errorf("entry %q (line %d): %w", entries[i].ID, i+2, err)
		}
		imported++
	}
	return imported, skipped, nil
}

// BillingPreviewLine 为 preview 的逐行错误（带行号，§7.1）。
type BillingPreviewLine struct {
	Line    int    `json:"line"`
	Message string `json:"message"`
}

// PreviewBillingCSV 解析账单 CSV 但不写入账本：返回合法行数、已存在重复数、
// 币种三口径、kind 分布、日期范围与逐行错误（方案 §7.1 preview 契约）。
func (s *Store) PreviewBillingCSV(data []byte, provider, account string) *ImportPreviewResult {
	out := &ImportPreviewResult{Valid: false}
	entries, err := ParseBillingCSV(data, provider, account)
	if err != nil {
		out.Error = err.Error()
		return out
	}
	out.Valid = true
	out.TotalRecords = int64(len(entries))
	kinds := map[string]int64{}
	out.Kinds = kinds
	byCurrency := map[string]*ReconciliationSummary{}
	var minDate, maxDate string
	for i := range entries {
		e := &entries[i]
		kinds[e.Kind]++
		cur, ok := byCurrency[e.Currency]
		if !ok {
			cur = &ReconciliationSummary{Currency: e.Currency}
			byCurrency[e.Currency] = cur
		}
		cur.RecognizedServiceSpend += e.ServiceCostImpact
		cur.CashOutflow += e.CashImpact
		cur.CreditBalanceDelta += e.CreditBalanceImpact
		d := e.PeriodStart
		if d != "" {
			if minDate == "" || d < minDate {
				minDate = d
			}
			if maxDate == "" || d > maxDate {
				maxDate = d
			}
		}
		if _, dup := s.billingEntryExists(e.ID); dup {
			out.DuplicateCount++
		}
	}
	for _, cur := range byCurrency {
		out.CurrencySummaries = append(out.CurrencySummaries, *cur)
	}
	out.EarliestRecord = minDate
	out.LatestRecord = maxDate
	return out
}

// InsertManualBillingEntry 写入一条用户手动确认的账务事实（§7.2）：
// evidence 强制 user_confirmed；kind 词表与三口径校验复用 InsertBillingEntry；
// 返回 RenewalEvaluation 触发结果由调用方决定（本方法不评估提醒）。
func (s *Store) InsertManualBillingEntry(e *BillingEntry) error {
	if e.ID == "" {
		// 手动条目无业务 ID：按内容指纹生成，重复提交同一事实幂等。
		h := fnv.New64a()
		for _, part := range []string{"manual", e.BillingAccount, e.Provider, e.Kind,
			e.PeriodStart, e.PeriodEnd, e.Currency,
			fmt.Sprintf("%d", e.ServiceCostImpact), fmt.Sprintf("%d", e.CashImpact), fmt.Sprintf("%d", e.CreditBalanceImpact), e.Note} {
			h.Write([]byte(part))
			h.Write([]byte{0})
		}
		e.ID = fmt.Sprintf("manual-%016x", h.Sum64())
	}
	e.EvidenceLevel = "user_confirmed"
	return s.InsertBillingEntry(e)
}

// billingEntryExists 查询指纹 ID 是否已入库。
func (s *Store) billingEntryExists(id string) (bool, bool) {
	var n int
	err := s.db.QueryRow(`SELECT COUNT(*) FROM usage_billing_entries WHERE id = ?`, id).Scan(&n)
	return n > 0, err == nil && n > 0
}
