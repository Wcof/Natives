// NativesStatusBarData.h: Model Host 8766 用量与额度只读查询（ADR-0033）
#import <Foundation/Foundation.h>
#import <sqlite3.h>
#import <math.h>

#if 0 // Historical migration reference only; Status Bar must not read module SQLite.
static inline NSString *LocateTokenUsageDbPath(void) {
    NSString *home = NSHomeDirectory();
    NSString *p1 = [home stringByAppendingPathComponent:@".natives-local/apps/tokenusage/data/tokenusage.db"];
    if ([[NSFileManager defaultManager] fileExistsAtPath:p1]) return p1;
    NSString *p2 = [home stringByAppendingPathComponent:@".natives/apps/tokenusage/data/tokenusage.db"];
    if ([[NSFileManager defaultManager] fileExistsAtPath:p2]) return p2;
    return nil;
}
#endif

static inline NSString *FormatCompactTokens(int64_t tokens) {
    if (tokens >= 1000000000LL) return [NSString stringWithFormat:@"%.2fB", (double)tokens / 1000000000.0];
    if (tokens >= 1000000LL) return [NSString stringWithFormat:@"%.1fM", (double)tokens / 1000000.0];
    if (tokens >= 1000LL) return [NSString stringWithFormat:@"%.1fK", (double)tokens / 1000.0];
    return [NSString stringWithFormat:@"%lld", tokens];
}

static inline const char *UsagePeriodWhere(NSString *range) {
    if ([range isEqualToString:@"today"]) {
        return "WHERE datetime(requested_at) >= datetime('now','localtime','start of day','utc')";
    }
    if ([range isEqualToString:@"7d"]) {
        return "WHERE datetime(requested_at) >= datetime('now','-7 days')";
    }
    if ([range isEqualToString:@"30d"]) {
        return "WHERE datetime(requested_at) >= datetime('now','-30 days')";
    }
    return "";
}

// 价格联接：按供应商／模型和来源优先级取完整价格行，别名只用于展示，
// 金额 = 存储计费 + 零计费事件按目录动态估算（与 model-host 查询层同一口径）。
// 只读库兜底路径没有 Go 计价器，金额直接在 SQL 内联价格表计算。
#define STATUSBAR_PRICE_JOIN \
    "LEFT JOIN model_prices p ON p.rowid = (SELECT rowid FROM model_prices " \
    "WHERE LOWER(model_id) = LOWER(e.model) AND " \
    "((source = 'manual' AND provider_id = e.provider) OR " \
    "(source IN ('manual','manual_global') AND provider_id = '') OR source IN ('synced','imported')) " \
    "ORDER BY CASE WHEN source = 'manual' AND provider_id != '' THEN 1 " \
    "WHEN source IN ('manual','manual_global') AND provider_id = '' THEN 2 " \
    "WHEN source = 'synced' THEN 3 ELSE 4 END, rowid LIMIT 1) "

#define STATUSBAR_COST_EXPR \
    "(COALESCE(SUM(e.cost_micro),0) + COALESCE(SUM(CASE WHEN e.cost_micro = 0 THEN " \
    "(e.input_tokens * p.input_price_micro + e.output_tokens * p.output_price_micro + e.cache_read_tokens * p.cache_read_price_micro " \
    "+ e.cache_write_tokens * p.cache_write_price_micro) ELSE 0 END), 0) / 1000000) "

// 完整计价标记：全部有用量的行都有存储计费或价格覆盖。
// false 时面板必须显示 "--"（未知），严禁把无价渲染成 $0.00。
// 该片段总是紧随 STATUSBAR_COST_EXPR 之后，故以前导逗号开头。
#define STATUSBAR_PRICED_EXPR \
    ", COALESCE(MIN(CASE WHEN e.total_tokens = 0 OR e.cost_micro != 0 OR p.rowid IS NOT NULL THEN 1 ELSE 0 END),1) "

static inline NSDictionary *ReadUsagePeriod(sqlite3 *db, NSString *range) {
    NSString *sql = [NSString stringWithFormat:
        @"SELECT COALESCE(SUM(e.total_tokens),0), " STATUSBAR_COST_EXPR
         STATUSBAR_PRICED_EXPR "FROM usage_events e " STATUSBAR_PRICE_JOIN "%s",
        UsagePeriodWhere(range)];
    sqlite3_stmt *stmt = NULL;
    if (sqlite3_prepare_v2(db, sql.UTF8String, -1, &stmt, NULL) != SQLITE_OK) return nil;
    int64_t tokens = 0, costMicro = 0;
    BOOL priced = NO;
    if (sqlite3_step(stmt) == SQLITE_ROW) {
        tokens = sqlite3_column_int64(stmt, 0);
        costMicro = sqlite3_column_int64(stmt, 1);
        priced = sqlite3_column_int(stmt, 2) != 0;
    } else { sqlite3_finalize(stmt); return nil; }
    sqlite3_finalize(stmt);
    return @{@"totalTokens": @(tokens), @"costUsd": @((double)costMicro / 1000000.0),
             @"priced": @(priced)};
}

static inline NSArray *ReadUsageModels(sqlite3 *db, NSString *range) {
    NSString *sql = [NSString stringWithFormat:
        @"SELECT COALESCE(NULLIF(e.model_alias, ''), e.model) AS name, "
         "SUM(e.total_tokens), " STATUSBAR_COST_EXPR STATUSBAR_PRICED_EXPR
         "FROM usage_events e " STATUSBAR_PRICE_JOIN "%s "
         "GROUP BY name HAVING SUM(e.total_tokens)>0 ORDER BY SUM(e.total_tokens) DESC LIMIT 8",
        UsagePeriodWhere(range)];
    sqlite3_stmt *stmt = NULL;
    if (sqlite3_prepare_v2(db, sql.UTF8String, -1, &stmt, NULL) != SQLITE_OK) return nil;
    NSMutableArray *items = [NSMutableArray array];
    int step;
    while ((step = sqlite3_step(stmt)) == SQLITE_ROW) {
        const unsigned char *name = sqlite3_column_text(stmt, 0);
        [items addObject:@{
            @"name": name ? [NSString stringWithUTF8String:(const char *)name] : @"unknown",
            @"tokens": @(sqlite3_column_int64(stmt, 1)),
            @"costUsd": @((double)sqlite3_column_int64(stmt, 2) / 1000000.0),
            @"priced": @(sqlite3_column_int(stmt, 3) != 0)
        }];
    }
    sqlite3_finalize(stmt);
    return step == SQLITE_DONE ? items : nil;
}

static inline NSArray *ReadUsageSessions(sqlite3 *db, NSString *range) {
    NSString *sql = [NSString stringWithFormat:
        @"SELECT e.session_id, e.tool_id, COALESCE(MAX(s.label), ''), "
            "SUM(e.total_tokens), " STATUSBAR_COST_EXPR
            STATUSBAR_PRICED_EXPR ", MAX(e.requested_at) "
        "FROM usage_events e " STATUSBAR_PRICE_JOIN
        "LEFT JOIN usage_subjects s ON s.kind = 'session' AND s.id = e.session_id "
        "%s %s e.session_id != '' "
        "GROUP BY e.tool_id, e.source_instance_id, e.session_id "
        "ORDER BY MAX(e.requested_at) DESC LIMIT 12",
        UsagePeriodWhere(range), [range isEqualToString:@"all"] ? "WHERE" : "AND"];
    sqlite3_stmt *stmt = NULL;
    if (sqlite3_prepare_v2(db, sql.UTF8String, -1, &stmt, NULL) != SQLITE_OK) return nil;
    NSMutableArray *items = [NSMutableArray array];
    int step;
    while ((step = sqlite3_step(stmt)) == SQLITE_ROW) {
        const unsigned char *sid = sqlite3_column_text(stmt, 0);
        const unsigned char *tool = sqlite3_column_text(stmt, 1);
        const unsigned char *title = sqlite3_column_text(stmt, 2);
        const unsigned char *lastAt = sqlite3_column_text(stmt, 6);
        NSString *toolName = tool ? [NSString stringWithUTF8String:(const char *)tool] : @"";
        NSString *titleName = title ? [NSString stringWithUTF8String:(const char *)title] : @"";
        NSString *display = titleName.length > 0 ? titleName
            : (sid ? [NSString stringWithUTF8String:(const char *)sid] : @"");
        [items addObject:@{
            @"id": display,
            @"tool": toolName,
            @"totalTokens": @(sqlite3_column_int64(stmt, 3)),
            @"costUsd": @((double)sqlite3_column_int64(stmt, 4) / 1000000.0),
            @"priced": @(sqlite3_column_int(stmt, 5) != 0),
            @"lastActive": lastAt ? [NSString stringWithUTF8String:(const char *)lastAt] : @""
        }];
    }
    sqlite3_finalize(stmt);
    return step == SQLITE_DONE ? items : nil;
}

static inline NSArray *ReadUsageTrend(sqlite3 *db) {
    const char *sql =
        "SELECT date(datetime(requested_at),'localtime'), SUM(total_tokens), SUM(cost_micro) "
        "FROM usage_events WHERE datetime(requested_at) >= datetime('now','localtime','start of day','-6 days','utc') "
        "GROUP BY 1 ORDER BY 1 ASC LIMIT 7";
    sqlite3_stmt *stmt = NULL;
    if (sqlite3_prepare_v2(db, sql, -1, &stmt, NULL) != SQLITE_OK) return nil;
    NSMutableArray *items = [NSMutableArray array];
    int step;
    while ((step = sqlite3_step(stmt)) == SQLITE_ROW) {
        const unsigned char *date = sqlite3_column_text(stmt, 0);
        [items addObject:@{
            @"date": date ? [NSString stringWithUTF8String:(const char *)date] : @"",
            @"totalTokens": @(sqlite3_column_int64(stmt, 1)),
            @"costUsd": @((double)sqlite3_column_int64(stmt, 2) / 1000000.0)
        }];
    }
    sqlite3_finalize(stmt);
    return step == SQLITE_DONE ? items : nil;
}

static inline NSDictionary *StatusBarUsageSummary(NSDictionary *today, NSDictionary *all) {
    BOOL hasToday = [today[@"totalTokens"] longLongValue] > 0;
    NSDictionary *period = hasToday ? today : all;
    NSString *cost = [period[@"priced"] boolValue]
        ? [NSString stringWithFormat:@"$%.2f", [period[@"costUsd"] doubleValue]] : @"--";
    NSString *display = [NSString stringWithFormat:@"%@ · %@", FormatCompactTokens([period[@"totalTokens"] longLongValue]), cost];
    return @{@"mode": @"both", @"displayText": display, @"worstLimit": [NSNull null],
        @"tooltip": [NSString stringWithFormat:@"Natives: %@ (%@；费用估算)", display, hasToday ? @"今日" : @"累计"]};
}

// ADR-0033 三级降级中的只读库兜底。只读 Model Host 权威 usage.db，
// 不写库、不迁移、不扫描会话文件，也不读取 Secret。
static inline NSDictionary *FetchStateFromUsageDbAtPath(NSString *dbPath) {
    if (dbPath.length == 0 || ![[NSFileManager defaultManager] fileExistsAtPath:dbPath]) return nil;
    sqlite3 *db = NULL;
    if (sqlite3_open_v2(dbPath.UTF8String, &db, SQLITE_OPEN_READONLY, NULL) != SQLITE_OK) {
        if (db) sqlite3_close(db);
        return nil;
    }
    sqlite3_busy_timeout(db, 1000);
    if (sqlite3_exec(db, "BEGIN", NULL, NULL, NULL) != SQLITE_OK) { sqlite3_close(db); return nil; }
    NSDictionary *ranges = @{@"today": @"today", @"thisWeek": @"7d", @"thisMonth": @"30d", @"allTime": @"all"};
    NSMutableDictionary *panel = [NSMutableDictionary dictionary];
    NSMutableDictionary *models = [NSMutableDictionary dictionary], *sessions = [NSMutableDictionary dictionary];
    for (NSString *key in ranges) {
        NSDictionary *period = ReadUsagePeriod(db, ranges[key]);
        NSArray *modelRows = ReadUsageModels(db, ranges[key]), *sessionRows = ReadUsageSessions(db, ranges[key]);
        if (!period || !modelRows || !sessionRows) { sqlite3_close(db); return nil; }
        panel[key] = period; models[key] = modelRows; sessions[key] = sessionRows;
    }
    NSArray *trend = ReadUsageTrend(db);
    BOOL committed = sqlite3_exec(db, "COMMIT", NULL, NULL, NULL) == SQLITE_OK;
    sqlite3_close(db);
    if (!trend || !committed) return nil;
    panel[@"toolsByPeriod"] = models; panel[@"sessionsByPeriod"] = sessions;
    panel[@"tools"] = models[@"today"]; panel[@"sessions"] = sessions[@"today"]; panel[@"trends"] = trend;
    NSMutableDictionary *state = [StatusBarUsageSummary(panel[@"today"], panel[@"allTime"]) mutableCopy];
    state[@"panel"] = panel; state[@"usageSource"] = @"database";
    return state;
}

static inline NSDictionary *FetchStateFromUsageDb(void) {
    NSString *root = [NSProcessInfo processInfo].environment[@"NATIVES_MODEL_HOST_CONFIG_DIR"];
    if (root.length > 0 && !root.isAbsolutePath) return nil;
    NSString *path = root.length > 0 && root.isAbsolutePath
        ? [root stringByAppendingPathComponent:@"usage.db"]
        : [NSHomeDirectory() stringByAppendingPathComponent:@".natives/usage.db"];
    return FetchStateFromUsageDbAtPath(path);
}

// 读取账号额度：仅消费 Model Host 8766 的真实查询结果；无真实窗口数据时不填充占位百分比。
// Google 额度端点返回英文窗口类型名；中文系统下映射为中文（与面板其余中文文案一致），
// 其余语言保留原文
static inline NSString *NativeQuotaWindowTypeLabel(NSString *name) {
    NSString *raw = name ?: @"";
    if (raw.length == 0) return raw;
    NSString *preferred = [NSLocale preferredLanguages].firstObject ?: @"";
    if (![preferred.lowercaseString hasPrefix:@"zh"]) return raw;
    NSString *lower = raw.lowercaseString;
    if ([lower containsString:@"weekly"]) return @"周额度";
    if ([lower containsString:@"five hour"] || [lower containsString:@"5-hour"] || [lower containsString:@"5 hour"]) return @"5小时额度";
    if ([lower containsString:@"daily"]) return @"日额度";
    if ([lower containsString:@"monthly"]) return @"月额度";
    return raw;
}

// Antigravity 模型组名（Gemini Models / Claude and GPT models 各自独立额度）
static inline NSString *NativeQuotaGroupLabel(NSString *group) {
    NSString *raw = group ?: @"";
    if (raw.length == 0) return raw;
    NSString *preferred = [NSLocale preferredLanguages].firstObject ?: @"";
    if (![preferred.lowercaseString hasPrefix:@"zh"]) return raw;
    NSString *lower = raw.lowercaseString;
    if ([lower isEqualToString:@"gemini models"]) return @"Gemini 模型";
    if ([lower containsString:@"claude"] && [lower containsString:@"gpt"]) return @"Claude 与 GPT 模型";
    return raw;
}

// 组装「模型组 · 窗口类型」标签；两个模型组各自拥有独立的周/5小时窗口，
// 严禁按窗口类型去重折叠（那会把两个模型组并成一个额度）
static inline NSString *NativeQuotaLocalizedLabel(NSString *type, NSString *group) {
    NSString *typeLabel = NativeQuotaWindowTypeLabel(type);
    NSString *groupLabel = NativeQuotaGroupLabel(group);
    return groupLabel.length > 0 ? [NSString stringWithFormat:@"%@ · %@", groupLabel, typeLabel] : typeLabel;
}

// 额度窗口展示排序：每个模型组内部，5小时排在上面，周额度排在下面，符合用户直觉习惯
static inline NSInteger QuotaWindowSortPriority(NSString *label) {
    if (![label isKindOfClass:[NSString class]]) return 99;
    NSString *lower = label.lowercaseString;
    if ([lower containsString:@"5小时"] || [lower containsString:@"5h"] || [lower containsString:@"hour"]) return 1;
    if ([lower containsString:@"日"] || [lower containsString:@"day"]) return 2;
    if ([lower containsString:@"周"] || [lower containsString:@"week"]) return 3;
    if ([lower containsString:@"月"] || [lower containsString:@"month"]) return 4;
    return 5;
}

static inline NSString *ExtractModelGroupFromLabel(NSString *label) {
    if (![label isKindOfClass:[NSString class]]) return @"";
    if ([label containsString:@"·"]) {
        NSArray *parts = [label componentsSeparatedByString:@"·"];
        if (parts.count > 1) {
            return [parts[0] stringByTrimmingCharactersInSet:[NSCharacterSet whitespaceCharacterSet]];
        }
    }
    return @"";
}

static inline void SortQuotaWindows5HFirst(NSMutableArray *windows) {
    if (![windows isKindOfClass:[NSMutableArray class]] || windows.count <= 1) return;
    [windows sortUsingComparator:^NSComparisonResult(NSDictionary *a, NSDictionary *b) {
        NSString *grpA = a[@"group"] ?: ExtractModelGroupFromLabel(a[@"label"]);
        NSString *grpB = b[@"group"] ?: ExtractModelGroupFromLabel(b[@"label"]);
        if (![grpA isEqualToString:grpB]) {
            return [grpA compare:grpB];
        }
        NSInteger rA = QuotaWindowSortPriority(a[@"label"] ?: a[@"name"]);
        NSInteger rB = QuotaWindowSortPriority(b[@"label"] ?: b[@"name"]);
        if (rA != rB) {
            return rA < rB ? NSOrderedAscending : NSOrderedDescending;
        }
        return NSOrderedSame;
    }];
}

// model-host 额度缓存：扩展「刷新额度」成功时写入的真实结果（各 provider 的
// QuotaResult JSON）。顶栏读取并按 provider+account 合并进账号窗口；
// 零假数据：缓存里只有真实成功结果，缺失的账号如实显示无窗口。
static inline NSArray *ParseAIQueryQuotaAccounts(NSData *data);
static inline NSDate *NativeParseISO8601(NSString *value);
static inline NSDictionary *FetchAIQueryJSON(NSString *path);

static inline NSArray *FetchQuotaCacheAccounts(void) {
    NSString *root = [NSProcessInfo processInfo].environment[@"NATIVES_MODEL_HOST_CONFIG_DIR"];
    if (root.length > 0 && !root.isAbsolutePath) return @[];
    NSString *path = root.length > 0 && root.isAbsolutePath
        ? [root stringByAppendingPathComponent:@"quota-cache.json"]
        : [NSHomeDirectory() stringByAppendingPathComponent:@"Library/Application Support/Natives/model-host/quota-cache.json"];
    NSData *data = [NSData dataWithContentsOfFile:path];
    id cache = data ? [NSJSONSerialization JSONObjectWithData:data options:0 error:nil] : nil;
    if (![cache isKindOfClass:NSDictionary.class]) return @[];
    NSData *wrapped = [NSJSONSerialization dataWithJSONObject:@{@"entries": cache} options:0 error:nil];
    return ParseAIQueryQuotaAccounts(wrapped);
}

// ===== AI 数据域查询客户端（ADR-0033 Phase 2 / 架构 Wave 2 终态）=====
// 顶栏不再直连上游额度端点（Codex wham / Google cloudcode-pa）、不再直读
// Keychain 续期、不再第二写方写 quota-cache.json（B2 退役）。额度数据路径：
//   手动刷新 → POST 127.0.0.1:8766/ai/quota/refresh（唯一上游触发口，model-host 执行）
//           → GET  127.0.0.1:8766/ai/quota（读 Go 侧最近探测结果）
//   8766 不可达 → 读 quota-cache.json（同一 Go 写方的缓存文件，离线兜底，只读）
//   两者皆空   → 空列表 + 面板显示"模型服务未运行"（零假数据）

// 解析 /ai/quota 响应为账号窗口行，形状与 FetchQuotaCacheAccounts 一致
// （provider/providerId/account/status/windows）。响应合约（aiquery.go
// serveQuotaCache）：{ status, generatedAt, entries: {credKey: QuotaResult} }，
// entries 是以凭据为键的字典（不是 results 数组）；空缓存时 status="empty"。
static inline NSArray *ParseAIQueryQuotaAccounts(NSData *data) {
    if (![data isKindOfClass:NSData.class] || data.length == 0) return @[];
    id json = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
    NSDictionary *entries = [json isKindOfClass:[NSDictionary class]] ? json[@"entries"] : nil;
    if (![entries isKindOfClass:[NSDictionary class]]) return @[];

    NSDateFormatter *shortFmt = [[NSDateFormatter alloc] init];
    shortFmt.dateFormat = @"MM-dd HH:mm";
    NSMutableArray *items = [NSMutableArray array];
    for (NSDictionary *entry in entries.allValues) {
        if (![entry isKindOfClass:NSDictionary.class] || ![entry[@"status"] isEqual:@"success"]) continue;
        if (![entry isKindOfClass:[NSDictionary class]]) continue;
        NSString *provider = [entry[@"provider"] isKindOfClass:[NSString class]] ? entry[@"provider"] : @"";
        NSString *account = [entry[@"account"] isKindOfClass:[NSString class]] ? entry[@"account"] : @"";
        NSArray *windows = [entry[@"windows"] isKindOfClass:[NSArray class]] ? entry[@"windows"] : @[];
        if (provider.length == 0 || windows.count == 0) continue;

        NSMutableArray *trayWindows = [NSMutableArray array];
        for (NSDictionary *w in windows) {
            if (![w isKindOfClass:[NSDictionary class]]) continue;
            NSNumber *pct = [w[@"remainingPercent"] isKindOfClass:[NSNumber class]] ? w[@"remainingPercent"] : nil;
            if (!pct || !isfinite(pct.doubleValue) || pct.doubleValue < 0 || pct.doubleValue > 100) continue;
            NSMutableDictionary *row = [NSMutableDictionary dictionary];
            NSString *typeName = [w[@"name"] isKindOfClass:[NSString class]] && [w[@"name"] length] > 0 ? w[@"name"] : @"额度";
            NSString *group = [w[@"group"] isKindOfClass:[NSString class]] ? w[@"group"] : @"";
            row[@"label"] = NativeQuotaLocalizedLabel(typeName, group);
            row[@"pct"] = @(MAX(0.0, MIN(100.0, pct.doubleValue)));
            if ([group containsString:@"Gemini"]) row[@"group"] = @"Gemini 模型";
            else if ([group containsString:@"Claude"]) row[@"group"] = @"Claude 与 GPT 模型";
            NSString *resetISO = [w[@"resetTime"] isKindOfClass:[NSString class]] ? w[@"resetTime"] : @"";
            if (resetISO.length > 0) {
                NSDate *resetDate = NativeParseISO8601(resetISO);
                if (resetDate) {
                    row[@"reset"] = [NSString stringWithFormat:@"%@ 重置", [shortFmt stringFromDate:resetDate]];
                    row[@"resetEpoch"] = @(resetDate.timeIntervalSince1970);
                }
            }
            [trayWindows addObject:row];
        }
        if (trayWindows.count == 0) continue;
        NSMutableDictionary *byLabel = [NSMutableDictionary dictionary];
        for (NSDictionary *row in trayWindows) {
            NSDictionary *previous = byLabel[row[@"label"]];
            if (!previous || [row[@"pct"] doubleValue] < [previous[@"pct"] doubleValue]) byLabel[row[@"label"]] = row;
        }
        trayWindows = [byLabel.allValues mutableCopy];
        SortQuotaWindows5HFirst(trayWindows);
        [items addObject:@{
            @"provider": [provider capitalizedString],
            @"providerId": provider,
            @"account": account,
            @"status": @"缓存",
            @"windows": trayWindows
        }];
    }
    return items;
}

// 走 8766 只读查询面获取账号额度。refresh=YES 时先 POST /ai/quota/refresh
// （model-host 对全部已启用凭据做一次上游探测并写缓存——上游请求仍只在用户
// 手动刷新时发生，反封号口径不变）。quotaSource 回填 "8766"/"cache"/"none"。
static inline NSArray *FetchQuotaAccountsViaAIQuery(BOOL refresh, NSString **quotaSource) {
    NSString *base = @"http://127.0.0.1:8766";
    __block BOOL refreshSucceeded = !refresh;
    if (refresh) {
        NSMutableURLRequest *post = [NSMutableURLRequest requestWithURL:
            [NSURL URLWithString:[base stringByAppendingString:@"/ai/quota/refresh"]]];
        post.HTTPMethod = @"POST";
        post.timeoutInterval = 30.0; // 上游逐凭据探测，给足超时
        [post setValue:@"application/json" forHTTPHeaderField:@"Content-Type"];
        post.HTTPBody = [@"{}" dataUsingEncoding:NSUTF8StringEncoding];
        dispatch_semaphore_t psem = dispatch_semaphore_create(0);
        NSURLSession *psess = [NSURLSession sessionWithConfiguration:[NSURLSessionConfiguration ephemeralSessionConfiguration]];
        [[psess dataTaskWithRequest:post completionHandler:^(NSData *d, NSURLResponse *r, NSError *e) {
            id result = d ? [NSJSONSerialization JSONObjectWithData:d options:0 error:nil] : nil;
            refreshSucceeded = !e && [(NSHTTPURLResponse *)r statusCode] == 200 && [result isKindOfClass:NSDictionary.class];
            for (NSDictionary *item in [result isKindOfClass:NSDictionary.class] && [result[@"results"] isKindOfClass:NSArray.class] ? result[@"results"] : @[]) {
                if (![item isKindOfClass:NSDictionary.class] || ![item[@"status"] isEqual:@"success"]) refreshSucceeded = NO;
            }
            dispatch_semaphore_signal(psem);
        }] resume];
        dispatch_semaphore_wait(psem, dispatch_time(DISPATCH_TIME_NOW, 35 * NSEC_PER_SEC));
        [psess invalidateAndCancel];
    }

    NSDictionary *response = FetchAIQueryJSON(@"/ai/quota");
    if ([response[@"entries"] isKindOfClass:NSDictionary.class]) {
        NSData *body = [NSJSONSerialization dataWithJSONObject:response options:0 error:nil];
        if (quotaSource) *quotaSource = refreshSucceeded ? @"8766" : @"refreshFailed";
        return ParseAIQueryQuotaAccounts(body);
    }
    // 8766 不可达（模型服务未运行）：缓存文件兜底（同一 Go 写方，只读）
    NSArray *cached = FetchQuotaCacheAccounts();
    if (quotaSource) *quotaSource = cached.count > 0 ? @"cache" : @"none";
    return cached;
}

// 8766 只读 GET 通用查询（失败/非 200 返回 nil）
static inline NSDictionary *FetchAIQueryJSON(NSString *path) {
    NSMutableURLRequest *request = [NSMutableURLRequest requestWithURL:
        [NSURL URLWithString:[@"http://127.0.0.1:8766" stringByAppendingString:path]]];
    request.timeoutInterval = 3.0;
    __block NSData *data = nil;
    dispatch_semaphore_t semaphore = dispatch_semaphore_create(0);
    NSURLSession *session = [NSURLSession sessionWithConfiguration:[NSURLSessionConfiguration ephemeralSessionConfiguration]];
    [[session dataTaskWithRequest:request completionHandler:^(NSData *body, NSURLResponse *response, NSError *error) {
        if (!error && [(NSHTTPURLResponse *)response statusCode] == 200) data = body;
        dispatch_semaphore_signal(semaphore);
    }] resume];
    dispatch_semaphore_wait(semaphore, dispatch_time(DISPATCH_TIME_NOW, 5 * NSEC_PER_SEC));
    [session invalidateAndCancel];
    id value = data ? [NSJSONSerialization JSONObjectWithData:data options:0 error:nil] : nil;
    return [value isKindOfClass:[NSDictionary class]] ? value : nil;
}

// 8766 存活探测（/ai/healthz）。超时给短，用于拉起前的快速检查与拉起后的就绪等待。
static inline BOOL AIQueryHealthzAlive(NSTimeInterval timeout) {
    NSMutableURLRequest *request = [NSMutableURLRequest requestWithURL:
        [NSURL URLWithString:@"http://127.0.0.1:8766/ai/healthz"]];
    request.timeoutInterval = timeout;
    __block BOOL alive = NO;
    dispatch_semaphore_t semaphore = dispatch_semaphore_create(0);
    NSURLSession *session = [NSURLSession sessionWithConfiguration:[NSURLSessionConfiguration ephemeralSessionConfiguration]];
    [[session dataTaskWithRequest:request completionHandler:^(NSData *data, NSURLResponse *response, NSError *error) {
        alive = !error && [(NSHTTPURLResponse *)response statusCode] == 200;
        dispatch_semaphore_signal(semaphore);
    }] resume];
    dispatch_semaphore_wait(semaphore, dispatch_time(DISPATCH_TIME_NOW, (int64_t)((timeout + 1.0) * NSEC_PER_SEC)));
    [session invalidateAndCancel];
    return alive;
}

// model-host 可执行文件候选（顶栏按需拉起 worker 用）。manifestDir 可注入以便测试。
// 顺序：顶栏同目录（开发态 target/debug 与 model-host 同级）→ Chrome NM manifest
//（开发 com.natives.local.model_host / 生产 com.natives.model_host）的 path 字段。
static inline NSArray *ModelHostExecutableCandidates(NSString *manifestDir) {
    NSMutableArray *candidates = [NSMutableArray array];
    NSString *selfPath = [[NSBundle mainBundle] executablePath];
    if (selfPath.length == 0) selfPath = [NSProcessInfo processInfo].arguments.firstObject;
    if (selfPath.length > 0) {
        NSString *dir = selfPath.stringByDeletingLastPathComponent;
        if (dir.length > 0) [candidates addObject:[dir stringByAppendingPathComponent:@"model-host"]];
    }
    NSFileManager *fm = [NSFileManager defaultManager];
    for (NSString *name in @[@"com.natives.model_host.json", @"com.natives.local.model_host.json"]) {
        NSString *manifestPath = [manifestDir stringByAppendingPathComponent:name];
        NSData *data = [NSData dataWithContentsOfFile:manifestPath];
        NSDictionary *manifest = data ? [NSJSONSerialization JSONObjectWithData:data options:0 error:nil] : nil;
        NSString *path = [manifest isKindOfClass:[NSDictionary class]] ? manifest[@"path"] : nil;
        if ([path isKindOfClass:[NSString class]] && path.length > 0 &&
            [fm fileExistsAtPath:path]) {
            [candidates addObject:path];
        }
    }
    return candidates;
}

static inline NSString *ModelHostManifestDirectory(void) {
    return [NSHomeDirectory() stringByAppendingPathComponent:
        @"Library/Application Support/Google/Chrome/NativeMessagingHosts"];
}

// 定位可启动的 model-host 二进制；找不到返回 nil（调用方保持既有离线兜底路径）。
static inline NSString *LocateModelHostExecutable(void) {
    NSFileManager *fm = [NSFileManager defaultManager];
    for (NSString *path in ModelHostExecutableCandidates(ModelHostManifestDirectory())) {
        if (path.length > 0 && [fm isExecutableFileAtPath:path]) return path;
    }
    return nil;
}

// RFC3339/ISO8601 解析：兼容带毫秒（2026-09-29T02:09:07.431Z）与不带毫秒
// （2026-09-29T02:09:07Z）两种形状。
static inline NSDate *NativeParseISO8601(NSString *value) {
    if (value.length == 0) return nil;
    NSISO8601DateFormatter *fractional = [[NSISO8601DateFormatter alloc] init];
    fractional.formatOptions = NSISO8601DateFormatWithInternetDateTime |
                               NSISO8601DateFormatWithFractionalSeconds;
    NSDate *date = [fractional dateFromString:value];
    if (date) return date;
    NSISO8601DateFormatter *plain = [[NSISO8601DateFormatter alloc] init];
    plain.formatOptions = NSISO8601DateFormatWithInternetDateTime;
    return [plain dateFromString:value];
}

// 使用情况（按模型分解）：8766 /ai/usage/analysis?range=… 的 byModel
// （按 token 降序，取前 8）。range 合约：Model Host 只接受 today/7d/30d/all。
static inline NSArray *ToolRowsFromResponse(NSDictionary *analysis) {
    NSMutableArray *tools = [NSMutableArray array];
    NSArray *byModel = [analysis[@"byModel"] isKindOfClass:[NSArray class]] ? analysis[@"byModel"] : @[];
    for (NSDictionary *m in byModel) {
        if (![m isKindOfClass:[NSDictionary class]]) continue;
        long long tokens = [m[@"tokens"] isKindOfClass:[NSNumber class]] ? [m[@"tokens"] longLongValue] : 0;
        NSString *name = [m[@"name"] isKindOfClass:[NSString class]] ? m[@"name"] : @"";
        if (tokens <= 0 || name.length == 0) continue;
        [tools addObject:@{
            @"name": name,
            @"tokens": @(tokens),
            @"costUsd": [m[@"costUsd"] isKindOfClass:[NSNumber class]] ? m[@"costUsd"] : @0.0,
            @"priced": [m[@"priced"] isKindOfClass:[NSNumber class]] ? m[@"priced"] : @NO
        }];
        if (tools.count >= 8) break;
    }
    return tools;
}

// 会话行映射：8766 /ai/usage/sessions（按 lastAt 倒序；带 title 会话名称与
// toolId，供面板按工具分组展示）。同一映射复用于全部周期。
static inline NSArray *SessionRowsFromResponse(NSDictionary *sessionsResp) {
    NSMutableArray *sessions = [NSMutableArray array];
    NSArray *sessionList = [sessionsResp[@"sessions"] isKindOfClass:[NSArray class]] ? sessionsResp[@"sessions"] : @[];
    for (NSDictionary *s in sessionList) {
        if (![s isKindOfClass:[NSDictionary class]]) continue;
        NSString *toolId = [s[@"toolId"] isKindOfClass:[NSString class]] ? s[@"toolId"] : @"";
        NSString *lastAt = [s[@"lastAt"] isKindOfClass:[NSString class]] ? s[@"lastAt"] : @"";
        NSString *title = [s[@"title"] isKindOfClass:[NSString class]] ? s[@"title"] : @"";
        if (toolId.length == 0 && lastAt.length == 0) continue;
        // 显示名：会话名称（真实标题）优先；无标题时回退工具名 + 最后活跃时间。
        // lastAt 实际形状为 RFC3339 带毫秒（2026-09-29T02:09:07.431Z），需按
        // ISO8601 解析，再兜底无小数秒形状。
        NSString *display = title;
        NSDate *d = lastAt.length > 0 ? NativeParseISO8601(lastAt) : nil;
        if (display.length == 0) {
            display = toolId;
            if (d) {
                NSDateFormatter *out = [[NSDateFormatter alloc] init];
                out.dateFormat = @"MM-dd HH:mm";
                display = [NSString stringWithFormat:@"%@ · %@", toolId, [out stringFromDate:d]];
            }
        }
        [sessions addObject:@{
            @"id": display,
            @"tool": toolId,
            @"totalTokens": [s[@"tokens"] isKindOfClass:[NSNumber class]] ? s[@"tokens"] : @0,
            @"costUsd": [s[@"costUsd"] isKindOfClass:[NSNumber class]] ? s[@"costUsd"] : @0.0,
            @"priced": [s[@"priced"] isKindOfClass:[NSNumber class]] ? s[@"priced"] : @NO
        }];
    }
    return sessions;
}

static inline NSString *AIUsageQueryPath(NSString *path, NSString *timezoneName) {
    NSMutableCharacterSet *allowed = [NSCharacterSet.URLQueryAllowedCharacterSet mutableCopy];
    [allowed removeCharactersInString:@"+&=?"];
    NSString *timezone = [timezoneName stringByAddingPercentEncodingWithAllowedCharacters:allowed];
    return [path stringByAppendingFormat:@"&timezone=%@", timezone];
}

static inline NSDictionary *UsagePeriodFromOverview(NSDictionary *overview) {
    NSDictionary *metrics = [overview[@"metrics"] isKindOfClass:NSDictionary.class] ? overview[@"metrics"] : nil;
    for (NSString *key in @[@"totalTokens", @"estimatedCostUsd", @"priced"]) {
        if (![metrics[key] isKindOfClass:NSNumber.class] || !isfinite([metrics[key] doubleValue])) return nil;
    }
    if ([metrics[@"totalTokens"] longLongValue] < 0) return nil;
    return @{@"totalTokens": metrics[@"totalTokens"], @"costUsd": metrics[@"estimatedCostUsd"], @"priced": metrics[@"priced"]};
}

static inline NSDictionary *FetchCompactStatusBarState(void) {
    NSDictionary *today = UsagePeriodFromOverview(FetchAIQueryJSON(AIUsageQueryPath(@"/ai/usage/overview?range=today", NSTimeZone.localTimeZone.name)));
    if (!today) return FetchStateFromUsageDb();
    NSDictionary *all = [today[@"totalTokens"] longLongValue] > 0 ? today
        : UsagePeriodFromOverview(FetchAIQueryJSON(AIUsageQueryPath(@"/ai/usage/overview?range=all", NSTimeZone.localTimeZone.name)));
    return all ? StatusBarUsageSummary(today, all) : FetchStateFromUsageDb();
}

static inline NSDictionary *FetchStateViaAIQueryWithFetcher(NSDictionary *(^fetch)(NSString *)) {
    NSDictionary *(^query)(NSString *) = ^NSDictionary *(NSString *path) { return fetch(AIUsageQueryPath(path, NSTimeZone.localTimeZone.name)); };
    // range 合约（ADR-0033 Usage Query）：Model Host 只接受 today/7d/30d/all；
    // 旧 day/week/month 不在合法集合内，会按无界范围处理导致今日/周/月显示错误。
    NSDictionary *all = query(@"/ai/usage/overview?range=all");
    if (!all) return nil;
    NSDictionary *today = query(@"/ai/usage/overview?range=today");
    NSDictionary *week = query(@"/ai/usage/overview?range=7d");
    NSDictionary *month = query(@"/ai/usage/overview?range=30d");
    if (!today || !week || !month) return nil;
    NSDictionary *todayPeriod = UsagePeriodFromOverview(today), *allPeriod = UsagePeriodFromOverview(all);
    NSDictionary *weekPeriod = UsagePeriodFromOverview(week), *monthPeriod = UsagePeriodFromOverview(month);
    if (!todayPeriod || !allPeriod || !weekPeriod || !monthPeriod) return nil;
    NSCalendar *calendar = NSCalendar.currentCalendar;
    NSDate *now = NSDate.date;
    NSDate *start = [calendar dateByAddingUnit:NSCalendarUnitDay value:-6 toDate:[calendar startOfDayForDate:now] options:0];
    NSISO8601DateFormatter *iso = [NSISO8601DateFormatter new];
    NSDictionary *trendResponse = query([NSString stringWithFormat:@"/ai/usage/overview?range=custom&startTime=%@&endTime=%@", [iso stringFromDate:start], [iso stringFromDate:now]]);
    if (!UsagePeriodFromOverview(trendResponse)) return nil;
    NSMutableArray *trend = [NSMutableArray array];
    for (NSDictionary *point in [trendResponse[@"trend"] isKindOfClass:NSArray.class] ? trendResponse[@"trend"] : @[]) {
        if (![point isKindOfClass:NSDictionary.class]) return nil;
        NSString *timestamp = [point[@"timestamp"] isKindOfClass:NSString.class] ? point[@"timestamp"] : nil;
        if (timestamp.length < 10 || ![point[@"tokens"] isKindOfClass:NSNumber.class]) return nil;
        [trend addObject:@{@"date": [timestamp substringToIndex:10], @"totalTokens": point[@"tokens"]}];
    }

    // 周期切换（面板页签）：使用情况与最近会话按所选周期分别查询——
    // 每个周期独立调 analysis / sessions（range 已由 Model Host 校验），
    // 不再把全量数据四路复用冒充周期数据。额度与余量不随周期切换。
    NSDictionary *rangePairs = @{
        @"today": @"today", @"thisWeek": @"7d", @"thisMonth": @"30d", @"allTime": @"all"
    };
    NSMutableDictionary *toolsByPeriod = [NSMutableDictionary dictionary];
    NSMutableDictionary *sessionsByPeriod = [NSMutableDictionary dictionary];
    for (NSString *panelKey in rangePairs) {
        NSString *range = rangePairs[panelKey];
        NSDictionary *analysis = query([NSString stringWithFormat:@"/ai/usage/analysis?range=%@", range]);
        NSDictionary *sessionResponse = query([NSString stringWithFormat:@"/ai/usage/sessions?range=%@&limit=12", range]);
        if (!analysis || !sessionResponse) return nil;
        toolsByPeriod[panelKey] = ToolRowsFromResponse(analysis);
        sessionsByPeriod[panelKey] = SessionRowsFromResponse(sessionResponse);
    }
    // 空周期保持空，不能拿其他周期的历史数据冒充。
    NSArray *todaySessions = sessionsByPeriod[@"today"];
    NSArray *tools = toolsByPeriod[@"today"] ?: @[];

    // 最近会话（默认页签「天」）：供无 sessionsByPeriod 的旧消费方回退。
    NSArray *sessions = todaySessions;

    NSMutableDictionary *state = [StatusBarUsageSummary(todayPeriod, allPeriod) mutableCopy];
    state[@"panel"] = @{
            @"today": todayPeriod, @"thisWeek": weekPeriod,
            @"thisMonth": monthPeriod, @"allTime": allPeriod,
            @"tools": tools, @"toolsByPeriod": toolsByPeriod,
            @"sessions": sessions, @"sessionsByPeriod": sessionsByPeriod,
            @"trends": trend
    };
    return state;
}

static inline NSDictionary *FetchStateViaAIQuery(void) {
    return FetchStateViaAIQueryWithFetcher(^NSDictionary *(NSString *path) {
        return FetchAIQueryJSON(path);
    });
}

static inline NSDictionary *FetchStatusBarState(void) {
    NSDictionary *state = FetchStateViaAIQuery();
    if (!state) return FetchStateFromUsageDb();
    NSMutableDictionary *tagged = [state mutableCopy];
    tagged[@"usageSource"] = @"8766";
    return tagged;
}

#if 0 // Historical migration reference only; replaced by FetchStateViaAIQuery.
// 按日期过滤查询工具分解（dateFilter 传 nil/空表示全量），按 token 降序，最多 8 条；
// 与 modules/tokenusage/src/api/stats.rs 的 tools_decomposed 口径一致
static inline NSMutableArray *FetchToolDecomposition(sqlite3 *db, NSString *dateFilter, NSString *dateValue) {
    NSMutableArray *tools = [NSMutableArray array];
    sqlite3_stmt *stmt = NULL;
    NSString *sql = nil;
    if (dateFilter.length == 0) {
        sql = @"SELECT s.display_name, COALESCE(SUM(d.total_tokens), 0), COALESCE(SUM(d.cost_micros), 0) "
               "FROM usage_sources s LEFT JOIN daily_aggregates d ON s.id = d.source_id "
               "GROUP BY s.id HAVING SUM(d.total_tokens) > 0 ORDER BY SUM(d.total_tokens) DESC LIMIT 8";
    } else {
        sql = [NSString stringWithFormat:@"SELECT s.display_name, COALESCE(SUM(d.total_tokens), 0), COALESCE(SUM(d.cost_micros), 0) "
               "FROM usage_sources s LEFT JOIN daily_aggregates d ON s.id = d.source_id "
               "WHERE %@?1 GROUP BY s.id HAVING SUM(d.total_tokens) > 0 ORDER BY SUM(d.total_tokens) DESC LIMIT 8", dateFilter];
    }
    if (sqlite3_prepare_v2(db, [sql UTF8String], -1, &stmt, NULL) == SQLITE_OK) {
        if (dateValue.length > 0) {
            sqlite3_bind_text(stmt, 1, [dateValue UTF8String], -1, SQLITE_TRANSIENT);
        }
        while (sqlite3_step(stmt) == SQLITE_ROW) {
            const unsigned char *nameChars = sqlite3_column_text(stmt, 0);
            NSString *name = nameChars ? [NSString stringWithUTF8String:(const char *)nameChars] : @"";
            int64_t t = sqlite3_column_int64(stmt, 1);
            int64_t c = sqlite3_column_int64(stmt, 2);
            if (t <= 0) continue;
            [tools addObject:[@{
                @"name": name,
                @"tokens": @(t),
                @"costUsd": @((double)c / 1000000.0)
            } mutableCopy]];
        }
        sqlite3_finalize(stmt);
    }
    return tools;
}

// 将指定工具用量合并进周期分解列表（已存在则累加，否则插入并保持降序）
static inline void MergeToolEntry(NSMutableArray *tools, NSString *name, int64_t tokens, double costUsd) {
    if (tokens <= 0 || name.length == 0) return;
    for (NSMutableDictionary *it in tools) {
        if ([it[@"name"] isEqualToString:name]) {
            it[@"tokens"] = @([it[@"tokens"] longLongValue] + tokens);
            it[@"costUsd"] = @([it[@"costUsd"] doubleValue] + costUsd);
            [tools sortUsingComparator:^NSComparisonResult(NSDictionary *a, NSDictionary *b) {
                return [@([b[@"tokens"] longLongValue]) compare:@([a[@"tokens"] longLongValue])];
            }];
            return;
        }
    }
    [tools addObject:[@{@"name": name, @"tokens": @(tokens), @"costUsd": @(costUsd)} mutableCopy]];
    [tools sortUsingComparator:^NSComparisonResult(NSDictionary *a, NSDictionary *b) {
        return [@([b[@"tokens"] longLongValue]) compare:@([a[@"tokens"] longLongValue])];
    }];
}

// 兜底本地状态读取（纯 tokenusage.db 只读；与 Rust panel_json 同库同口径）：
static inline NSDictionary *FetchStateFromLocalSqlite(void) {
    NSString *dbPath = LocateTokenUsageDbPath();
    if (!dbPath) return nil;

    sqlite3 *db = NULL;
    if (sqlite3_open_v2([dbPath UTF8String], &db, SQLITE_OPEN_READONLY, NULL) != SQLITE_OK) {
        if (db) sqlite3_close(db);
        return nil;
    }

    NSDateFormatter *df = [[NSDateFormatter alloc] init];
    [df setDateFormat:@"yyyy-MM-dd"];
    [df setTimeZone:[NSTimeZone localTimeZone]];
    NSDate *now = [NSDate date];
    NSString *todayStr = [df stringFromDate:now];

    NSDate *weekAgo = [now dateByAddingTimeInterval:-7 * 86400];
    NSString *weekStr = [df stringFromDate:weekAgo];

    NSString *monthStr = [todayStr substringToIndex:MIN((NSUInteger)7, todayStr.length)];
    monthStr = [monthStr stringByAppendingString:@"-01"];

    // 1) today
    int64_t todayTokens = 0, todayCostMicros = 0;
    sqlite3_stmt *stmt = NULL;
    const char *qToday = "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0) FROM daily_aggregates WHERE date = ?1";
    if (sqlite3_prepare_v2(db, qToday, -1, &stmt, NULL) == SQLITE_OK) {
        sqlite3_bind_text(stmt, 1, [todayStr UTF8String], -1, SQLITE_STATIC);
        if (sqlite3_step(stmt) == SQLITE_ROW) {
            todayTokens = sqlite3_column_int64(stmt, 0);
            todayCostMicros = sqlite3_column_int64(stmt, 1);
        }
        sqlite3_finalize(stmt);
    }

    // 2) thisWeek
    int64_t weekTokens = 0, weekCostMicros = 0;
    const char *qWeek = "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0) FROM daily_aggregates WHERE date >= ?1";
    if (sqlite3_prepare_v2(db, qWeek, -1, &stmt, NULL) == SQLITE_OK) {
        sqlite3_bind_text(stmt, 1, [weekStr UTF8String], -1, SQLITE_STATIC);
        if (sqlite3_step(stmt) == SQLITE_ROW) {
            weekTokens = sqlite3_column_int64(stmt, 0);
            weekCostMicros = sqlite3_column_int64(stmt, 1);
        }
        sqlite3_finalize(stmt);
    }

    // 3) thisMonth
    int64_t monthTokens = 0, monthCostMicros = 0;
    const char *qMonth = "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0) FROM daily_aggregates WHERE date >= ?1";
    if (sqlite3_prepare_v2(db, qMonth, -1, &stmt, NULL) == SQLITE_OK) {
        sqlite3_bind_text(stmt, 1, [monthStr UTF8String], -1, SQLITE_STATIC);
        if (sqlite3_step(stmt) == SQLITE_ROW) {
            monthTokens = sqlite3_column_int64(stmt, 0);
            monthCostMicros = sqlite3_column_int64(stmt, 1);
        }
        sqlite3_finalize(stmt);
    }

    // 4) allTime
    int64_t allTokens = 0, allCostMicros = 0;
    const char *qAll = "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0) FROM daily_aggregates";
    if (sqlite3_prepare_v2(db, qAll, -1, &stmt, NULL) == SQLITE_OK) {
        if (sqlite3_step(stmt) == SQLITE_ROW) {
            allTokens = sqlite3_column_int64(stmt, 0);
            allCostMicros = sqlite3_column_int64(stmt, 1);
        }
        sqlite3_finalize(stmt);
    }

    // 5) 工具分解：与 modules/tokenusage/src/api/stats.rs tools_decomposed 口径 1:1 对齐
    NSMutableArray *toolsAll = FetchToolDecomposition(db, nil, nil);
    NSMutableArray *toolsToday = FetchToolDecomposition(db, @"d.date >=", todayStr);
    NSMutableArray *toolsWeek = FetchToolDecomposition(db, @"d.date >=", weekStr);
    NSMutableArray *toolsMonth = FetchToolDecomposition(db, @"d.date >=", monthStr);

    // 6) sessions: 最近会话 Top5（1:1 对齐 Rust panel_json）
    NSMutableArray *sessions = [NSMutableArray array];
    const char *qSess = "SELECT session_id, source_id, "
                        "total_input_tokens + total_output_tokens + total_cache_read_tokens + total_cache_write_tokens + total_reasoning_tokens, "
                        "total_cost_micros, last_used_at "
                        "FROM sessions ORDER BY last_used_at DESC LIMIT 5";
    if (sqlite3_prepare_v2(db, qSess, -1, &stmt, NULL) == SQLITE_OK) {
        while (sqlite3_step(stmt) == SQLITE_ROW) {
            const unsigned char *sidChars = sqlite3_column_text(stmt, 0);
            const unsigned char *srcChars = sqlite3_column_text(stmt, 1);
            int64_t t = sqlite3_column_int64(stmt, 2);
            int64_t c = sqlite3_column_int64(stmt, 3);
            const unsigned char *actChars = sqlite3_column_text(stmt, 4);
            [sessions addObject:@{
                @"id": sidChars ? [NSString stringWithUTF8String:(const char *)sidChars] : @"",
                @"source": srcChars ? [NSString stringWithUTF8String:(const char *)srcChars] : @"",
                @"totalTokens": @(t),
                @"costUsd": @((double)c / 1000000.0),
                @"lastActive": actChars ? [NSString stringWithUTF8String:(const char *)actChars] : @""
            }];
        }
        sqlite3_finalize(stmt);
    }

    // 7) trends Top 7（1:1 对齐 Rust panel_json）
    NSMutableArray *trends = [NSMutableArray array];
    const char *qTrends = "SELECT date, SUM(total_tokens), SUM(cost_micros) FROM daily_aggregates "
                          "GROUP BY date ORDER BY date DESC LIMIT 7";
    if (sqlite3_prepare_v2(db, qTrends, -1, &stmt, NULL) == SQLITE_OK) {
        while (sqlite3_step(stmt) == SQLITE_ROW) {
            const unsigned char *dateChars = sqlite3_column_text(stmt, 0);
            int64_t t = sqlite3_column_int64(stmt, 1);
            int64_t c = sqlite3_column_int64(stmt, 2);
            [trends addObject:@{
                @"date": dateChars ? [NSString stringWithUTF8String:(const char *)dateChars] : @"",
                @"totalTokens": @(t),
                @"costUsd": @((double)c / 1000000.0)
            }];
        }
        sqlite3_finalize(stmt);
    }

    // ZCode 直读兜底已退役（审计 B2）：tokenusage.db 经 host_sync 持续同步
    // model-host 权威库（ZCode SQLite 采集器），统一库即唯一面板数据源。
    NSDictionary *toolsByPeriod = @{@"today": toolsToday, @"thisWeek": toolsWeek,
                                    @"thisMonth": toolsMonth, @"allTime": toolsAll};

    // 8) worstLimit: 从真实额度窗口中提取最紧张余量；无真实数据时保持 nil（不造假）
    NSDictionary *worstLimit = nil;
    NSArray *accountsList = FetchQuotaCacheAccounts();
    double lowestPct = 101.0;
    for (NSDictionary *acc in accountsList) {
        NSArray *wins = acc[@"windows"];
        if (![wins isKindOfClass:[NSArray class]]) continue;
        for (NSDictionary *w in wins) {
            id pctVal = w[@"pct"];
            if (![pctVal isKindOfClass:[NSNumber class]]) continue;
            double p = MAX(0.0, MIN(100.0, [pctVal doubleValue]));
            if (p < lowestPct) {
                lowestPct = p;
                worstLimit = @{
                    @"providerId": acc[@"provider"] ?: @"AI",
                    @"windowKind": w[@"label"] ?: @"",
                    @"remainingPercent": @(p),
                    @"resetsAt": w[@"reset"] ?: @""
                };
            }
        }
    }

    // 9) 最近活跃日数据（与 Rust api_tray_state 口径一致：今日为 0 时回退最近活跃日）
    int64_t dispTokens = todayTokens;
    double dispCost = (double)todayCostMicros / 1000000.0;
    if (dispTokens == 0) {
        const char *qRecent = "SELECT SUM(total_tokens), SUM(cost_micros) FROM daily_aggregates GROUP BY date HAVING SUM(total_tokens) > 0 ORDER BY date DESC LIMIT 1";
        if (sqlite3_prepare_v2(db, qRecent, -1, &stmt, NULL) == SQLITE_OK) {
            if (sqlite3_step(stmt) == SQLITE_ROW) {
                dispTokens = sqlite3_column_int64(stmt, 0);
                dispCost = (double)sqlite3_column_int64(stmt, 1) / 1000000.0;
            }
            sqlite3_finalize(stmt);
        }
        if (dispTokens == 0 && allTokens > 0) {
            dispTokens = allTokens;
            dispCost = (double)allCostMicros / 1000000.0;
        }
    }

    sqlite3_close(db);

    NSString *tokensFormatted = FormatCompactTokens(dispTokens);
    NSString *costFormatted = [NSString stringWithFormat:@"$%.2f", dispCost];
    NSString *displayText = [NSString stringWithFormat:@"%@ · %@", tokensFormatted, costFormatted];

    NSDictionary *panel = @{
        @"today": @{
            @"totalTokens": @(todayTokens),
            @"costUsd": @((double)todayCostMicros / 1000000.0)
        },
        @"thisWeek": @{
            @"totalTokens": @(weekTokens),
            @"costUsd": @((double)weekCostMicros / 1000000.0)
        },
        @"thisMonth": @{
            @"totalTokens": @(monthTokens),
            @"costUsd": @((double)monthCostMicros / 1000000.0)
        },
        @"allTime": @{
            @"totalTokens": @(allTokens),
            @"costUsd": @((double)allCostMicros / 1000000.0)
        },
        @"tools": toolsAll,
        @"toolsByPeriod": toolsByPeriod,
        @"sessions": sessions,
        @"trends": trends
    };

    return @{
        @"mode": @"both",
        @"displayText": displayText,
        @"tooltip": [NSString stringWithFormat:@"Natives: %@ (%@)", displayText, todayTokens > 0 ? @"今日" : @"累计"],
        @"worstLimit": worstLimit ?: [NSNull null],
        @"panel": panel
    };
}
#endif
