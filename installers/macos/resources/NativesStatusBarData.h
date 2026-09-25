// NativesStatusBarData.h: 本地数据库直读与多工具（含 ZCode）用量与账号额度聚合引擎
#import <Foundation/Foundation.h>
#import <sqlite3.h>

static inline NSString *LocateTokenUsageDbPath(void) {
    NSString *home = NSHomeDirectory();
    NSString *p1 = [home stringByAppendingPathComponent:@".natives-local/apps/tokenusage/data/tokenusage.db"];
    if ([[NSFileManager defaultManager] fileExistsAtPath:p1]) return p1;
    NSString *p2 = [home stringByAppendingPathComponent:@".natives/apps/tokenusage/data/tokenusage.db"];
    if ([[NSFileManager defaultManager] fileExistsAtPath:p2]) return p2;
    return nil;
}

static inline NSString *FormatCompactTokens(int64_t tokens) {
    if (tokens >= 1000000000LL) return [NSString stringWithFormat:@"%.2fB", (double)tokens / 1000000000.0];
    if (tokens >= 1000000LL) return [NSString stringWithFormat:@"%.1fM", (double)tokens / 1000000.0];
    if (tokens >= 1000LL) return [NSString stringWithFormat:@"%.1fK", (double)tokens / 1000.0];
    return [NSString stringWithFormat:@"%lld", tokens];
}

// 读取账号额度：仅真实数据。账号身份与启停状态来自 model-host state.json 与
// ~/.codex/auth.json；窗口余量仅来自 tokenusage.db limits_cache 的真实同步行。
// 无真实窗口数据时不填充任何占位百分比；Codex 实时额度由 FetchCodexUsageLive 请求。
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

// model-host 额度缓存：扩展「刷新额度」成功时写入的真实结果（各 provider 的
// QuotaResult JSON）。顶栏读取并按 provider+account 合并进账号窗口；
// 零假数据：缓存里只有真实成功结果，缺失的账号如实显示无窗口。
static inline NSArray *FetchQuotaCacheAccounts(void) {
    NSString *path = [NSHomeDirectory() stringByAppendingPathComponent:
        @"Library/Application Support/Natives/model-host/quota-cache.json"];
    NSData *data = [NSData dataWithContentsOfFile:path];
    if (!data) return @[];
    NSDictionary *cache = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
    if (![cache isKindOfClass:[NSDictionary class]]) return @[];

    NSMutableArray *items = [NSMutableArray array];
    NSISO8601DateFormatter *iso = [[NSISO8601DateFormatter alloc] init];
    NSDateFormatter *shortFmt = [[NSDateFormatter alloc] init];
    shortFmt.dateFormat = @"MM-dd HH:mm";
    for (NSString *key in cache) {
        NSDictionary *entry = [cache[key] isKindOfClass:[NSDictionary class]] ? cache[key] : nil;
        if (!entry) continue;
        NSString *provider = [entry[@"provider"] isKindOfClass:[NSString class]] ? entry[@"provider"] : @"";
        NSString *account = [entry[@"account"] isKindOfClass:[NSString class]] ? entry[@"account"] : @"";
        NSArray *windows = [entry[@"windows"] isKindOfClass:[NSArray class]] ? entry[@"windows"] : @[];
        if (provider.length == 0 || windows.count == 0) continue;

        NSMutableArray *trayWindows = [NSMutableArray array];
        for (NSDictionary *w in windows) {
            if (![w isKindOfClass:[NSDictionary class]]) continue;
            NSNumber *pct = [w[@"remainingPercent"] isKindOfClass:[NSNumber class]] ? w[@"remainingPercent"] : nil;
            if (!pct) continue;
            NSMutableDictionary *row = [NSMutableDictionary dictionary];
            NSString *typeName = [w[@"name"] isKindOfClass:[NSString class]] && [w[@"name"] length] > 0 ? w[@"name"] : @"额度";
            NSString *group = [w[@"group"] isKindOfClass:[NSString class]] ? w[@"group"] : @"";
            row[@"label"] = NativeQuotaLocalizedLabel(typeName, group);
            row[@"pct"] = @(MAX(0.0, MIN(100.0, pct.doubleValue)));
            NSString *resetISO = [w[@"resetTime"] isKindOfClass:[NSString class]] ? w[@"resetTime"] : @"";
            if (resetISO.length > 0) {
                NSDate *resetDate = [iso dateFromString:resetISO];
                if (resetDate) {
                    row[@"reset"] = [NSString stringWithFormat:@"%@ 重置", [shortFmt stringFromDate:resetDate]];
                    row[@"resetEpoch"] = @(resetDate.timeIntervalSince1970); // 顶栏轮播倒计时用
                }
            }
            [trayWindows addObject:row];
        }
        // 同名窗口去重（Google 会返回多组同名窗口）：保留约束最紧（剩余最少）的一行
        if (trayWindows.count > 1) {
            NSMutableDictionary *collapsed = [NSMutableDictionary dictionary];
            NSMutableArray *order = [NSMutableArray array];
            for (NSDictionary *row in trayWindows) {
                NSString *label = row[@"label"];
                NSDictionary *prev = collapsed[label];
                if (!prev) {
                    collapsed[label] = row;
                    [order addObject:label];
                } else if ([row[@"pct"] doubleValue] < [prev[@"pct"] doubleValue]) {
                    collapsed[label] = row;
                }
            }
            NSMutableArray *finalWindows = [NSMutableArray array];
            for (NSString *label in order) [finalWindows addObject:collapsed[label]];
            trayWindows = finalWindows;
        }
        if (trayWindows.count == 0) continue;
        [items addObject:@{
            @"provider": provider,
            @"providerId": provider,
            @"account": account,
            @"status": @"活跃",
            @"windows": trayWindows
        }];
    }
    return items;
}

static inline NSArray *FetchLimitsFromLocalSqlite(void) {
    NSMutableArray *accounts = [NSMutableArray array];
    NSMutableDictionary<NSString *, NSMutableDictionary *> *accMap = [NSMutableDictionary dictionary];

    NSString *home = NSHomeDirectory();
    NSString *proxyPath = [home stringByAppendingPathComponent:@"Library/Application Support/Natives/model-host/state.json"];
    if ([[NSFileManager defaultManager] fileExistsAtPath:proxyPath]) {
        NSData *pData = [NSData dataWithContentsOfFile:proxyPath];
        if (pData) {
            NSDictionary *pJson = [NSJSONSerialization JSONObjectWithData:pData options:0 error:nil];
            if ([pJson isKindOfClass:[NSDictionary class]]) {
                NSArray *accList = pJson[@"accounts"];
                if ([accList isKindOfClass:[NSArray class]]) {
                    for (NSDictionary *a in accList) {
                        if (![a isKindOfClass:[NSDictionary class]]) continue;
                        NSString *prov = a[@"provider"] ?: @"antigravity";
                        NSString *email = a[@"label"] ?: a[@"id"] ?: @"";
                        NSString *key = [NSString stringWithFormat:@"%@_%@", prov, email];
                        NSString *provDisplay = [prov capitalizedString];
                        if ([prov isEqualToString:@"antigravity"]) provDisplay = @"Antigravity";

                        // 零假数据：身份与启停状态为真实信息，额度窗口未知时不填占位百分比
                        NSMutableDictionary *item = [@{
                            @"provider": provDisplay,
                            @"providerId": prov,
                            @"account": email,
                            @"status": [a[@"enabled"] boolValue] ? @"活跃" : @"未启用",
                            @"windows": [NSMutableArray array]
                        } mutableCopy];
                        accMap[key] = item;
                    }
                }
            }
        }
    }

    // 2. 读取 ~/.codex/auth.json (Codex / ChatGPT 账号)
    NSString *codexAuthPath = [home stringByAppendingPathComponent:@".codex/auth.json"];
    if ([[NSFileManager defaultManager] fileExistsAtPath:codexAuthPath]) {
        NSData *cData = [NSData dataWithContentsOfFile:codexAuthPath];
        if (cData) {
            NSDictionary *cJson = [NSJSONSerialization JSONObjectWithData:cData options:0 error:nil];
            if ([cJson isKindOfClass:[NSDictionary class]]) {
                NSString *email = @"";
                NSDictionary *tok = cJson[@"tokens"];
                if ([tok isKindOfClass:[NSDictionary class]]) {
                    NSString *idToken = tok[@"id_token"];
                    if ([idToken isKindOfClass:[NSString class]]) {
                        NSArray *parts = [idToken componentsSeparatedByString:@"."];
                        if (parts.count >= 2) {
                            NSString *payloadB64 = parts[1];
                            while (payloadB64.length % 4 != 0) payloadB64 = [payloadB64 stringByAppendingString:@"="];
                            payloadB64 = [[payloadB64 stringByReplacingOccurrencesOfString:@"-" withString:@"+"] stringByReplacingOccurrencesOfString:@"_" withString:@"/"];
                            NSData *jwtData = [[NSData alloc] initWithBase64EncodedString:payloadB64 options:0];
                            if (jwtData) {
                                NSDictionary *payloadJson = [NSJSONSerialization JSONObjectWithData:jwtData options:0 error:nil];
                                if ([payloadJson isKindOfClass:[NSDictionary class]] && payloadJson[@"email"]) {
                                    email = payloadJson[@"email"];
                                }
                            }
                        }
                    }
                }
                // 零假数据：仅记录凭据存在与邮箱身份（解析失败不伪造邮箱）；真实额度由实时请求获取
                NSString *key = [NSString stringWithFormat:@"codex_%@", email];
                if (!accMap[key]) {
                    accMap[key] = [@{
                        @"provider": @"Codex (OpenAI)",
                        @"providerId": @"codex",
                        @"account": email,
                        @"status": @"活跃",
                        @"windows": [NSMutableArray array]
                    } mutableCopy];
                }
            }
        }
    }

    // 3. 从 tokenusage.db 的 limits_cache 表更新已同步的具体数据
    NSString *dbPath = LocateTokenUsageDbPath();
    if (dbPath) {
        sqlite3 *db = NULL;
        if (sqlite3_open_v2([dbPath UTF8String], &db, SQLITE_OPEN_READONLY, NULL) == SQLITE_OK) {
            sqlite3_stmt *stmt = NULL;
            const char *q = "SELECT provider_id, account_id, window_kind, remaining_percent, resets_at, label FROM limits_cache WHERE remaining_percent IS NOT NULL";
            if (sqlite3_prepare_v2(db, q, -1, &stmt, NULL) == SQLITE_OK) {
                while (sqlite3_step(stmt) == SQLITE_ROW) {
                    const unsigned char *pChars = sqlite3_column_text(stmt, 0);
                    const unsigned char *aChars = sqlite3_column_text(stmt, 1);
                    const unsigned char *wChars = sqlite3_column_text(stmt, 2);
                    double rem = sqlite3_column_double(stmt, 3);
                    const unsigned char *rChars = sqlite3_column_text(stmt, 4);
                    const unsigned char *lChars = sqlite3_column_text(stmt, 5);

                    NSString *prov = pChars ? [NSString stringWithUTF8String:(const char *)pChars] : @"";
                    NSString *acc = aChars ? [NSString stringWithUTF8String:(const char *)aChars] : @"";
                    NSString *win = wChars ? [NSString stringWithUTF8String:(const char *)wChars] : @"";
                    NSString *resets = rChars ? [NSString stringWithUTF8String:(const char *)rChars] : @"";
                    NSString *lbl = lChars ? [NSString stringWithUTF8String:(const char *)lChars] : @"";

                    NSString *key = [NSString stringWithFormat:@"%@_%@", prov, acc];
                    NSMutableDictionary *target = accMap[key];
                    if (!target) {
                        target = [@{
                            @"provider": [prov capitalizedString],
                            @"providerId": prov,
                            @"account": acc,
                            @"status": @"活跃",
                            @"windows": [NSMutableArray array]
                        } mutableCopy];
                        accMap[key] = target;
                    }

                    // 真实同步行：窗口名称按 window_kind 如实标注，余量与重置时间不加工
                    NSString *winLabel = nil;
                    if ([win isEqualToString:@"session"] || [win isEqualToString:@"5hour"] || [win isEqualToString:@"hourly"]) winLabel = @"5小时额度";
                    else if ([win isEqualToString:@"daily"]) winLabel = @"今日额度";
                    else if ([win isEqualToString:@"weekly"]) winLabel = @"周额度";
                    else if ([win isEqualToString:@"monthly"]) winLabel = @"月度额度";
                    else winLabel = @"账户额度";
                    NSMutableDictionary *row = [@{@"label": winLabel, @"pct": @(MAX(0.0, MIN(100.0, rem)))} mutableCopy];
                    if (resets.length > 0) row[@"reset"] = resets;
                    [(NSMutableArray *)target[@"windows"] addObject:row];
                }
                sqlite3_finalize(stmt);
            }
            sqlite3_close(db);
        }
    }

    // model-host 额度缓存（扩展「刷新额度」的真实成功结果）：覆盖匹配账号的窗口，
    // 让顶栏展示 Antigravity 等经 model-host 查询的真实余额
    for (NSDictionary *cached in FetchQuotaCacheAccounts()) {
        // 注意：for-in 遍历字典得到的是键，账号字典必须走 allValues
        for (NSMutableDictionary *it in [accMap allValues]) {
            if ([it[@"providerId"] isKindOfClass:[NSString class]] &&
                [it[@"providerId"] isEqualToString:cached[@"providerId"]] &&
                [it[@"account"] isKindOfClass:[NSString class]] &&
                [it[@"account"] isEqualToString:cached[@"account"]]) {
                it[@"windows"] = cached[@"windows"];
                break;
            }
        }
    }

    // 零假数据：无任何真实账号时不构造默认账号，返回空列表由面板如实显示空态
    [accounts addObjectsFromArray:[accMap allValues]];
    return accounts;
}

// 实时请求 Codex 真实额度（ChatGPT backend usage 端点，与 Codex CLI 同源）。
// 本机无 access_token 时直接返回 nil 不发起请求；请求失败或结构缺失也返回 nil，
// 由调用方保留本地真实缓存值，绝不回落到任何占位数字。仅限后台线程调用。
static inline NSDictionary *FetchCodexUsageLive(void) {
    NSString *authPath = [NSHomeDirectory() stringByAppendingPathComponent:@".codex/auth.json"];
    if (![[NSFileManager defaultManager] fileExistsAtPath:authPath]) return nil;
    NSData *authData = [NSData dataWithContentsOfFile:authPath];
    if (!authData) return nil;
    NSDictionary *auth = [NSJSONSerialization JSONObjectWithData:authData options:0 error:nil];
    if (![auth isKindOfClass:[NSDictionary class]]) return nil;
    NSDictionary *tokens = auth[@"tokens"];
    if (![tokens isKindOfClass:[NSDictionary class]]) return nil;
    NSString *token = tokens[@"access_token"];
    if (![token isKindOfClass:[NSString class]] || token.length == 0) return nil;
    NSString *accountId = [tokens[@"account_id"] isKindOfClass:[NSString class]] ? tokens[@"account_id"] : @"";

    NSMutableURLRequest *req = [NSMutableURLRequest requestWithURL:
        [NSURL URLWithString:@"https://chatgpt.com/backend-api/wham/usage"]];
    req.HTTPMethod = @"GET";
    req.timeoutInterval = 6.0;
    [req setValue:[NSString stringWithFormat:@"Bearer %@", token] forHTTPHeaderField:@"Authorization"];
    if (accountId.length > 0) [req setValue:accountId forHTTPHeaderField:@"chatgpt-account-id"];
    [req setValue:@"application/json" forHTTPHeaderField:@"Accept"];

    __block NSData *respData = nil;
    __block NSError *respError = nil;
    dispatch_semaphore_t sem = dispatch_semaphore_create(0);
    NSURLSession *session = [NSURLSession sessionWithConfiguration:[NSURLSessionConfiguration ephemeralSessionConfiguration]];
    [[session dataTaskWithRequest:req completionHandler:^(NSData *data, NSURLResponse *resp, NSError *error) {
        respData = data;
        respError = error;
        dispatch_semaphore_signal(sem);
    }] resume];
    dispatch_semaphore_wait(sem, dispatch_time(DISPATCH_TIME_NOW, (int64_t)(8 * NSEC_PER_SEC)));
    [session finishTasksAndInvalidate];
    if (respError || !respData) return nil;

    NSDictionary *json = [NSJSONSerialization JSONObjectWithData:respData options:0 error:nil];
    if (![json isKindOfClass:[NSDictionary class]]) return nil;
    NSDictionary *rateLimit = json[@"rate_limit"];
    if (![rateLimit isKindOfClass:[NSDictionary class]]) return nil;

    NSMutableArray *windows = [NSMutableArray array];
    for (NSString *key in @[@"primary_window", @"secondary_window"]) {
        NSDictionary *w = rateLimit[key];
        if (![w isKindOfClass:[NSDictionary class]]) continue;
        id usedVal = w[@"used_percent"];
        if (![usedVal isKindOfClass:[NSNumber class]]) continue;
        double usedPct = MAX(0.0, MIN(100.0, [usedVal doubleValue]));

        // 窗口名称按上游真实窗口长度标注，不假设 5 小时/周
        NSInteger winSecs = [w[@"limit_window_seconds"] isKindOfClass:[NSNumber class]] ? [w[@"limit_window_seconds"] integerValue] : 0;
        NSString *label = nil;
        if (winSecs > 0 && winSecs <= 6 * 3600) label = @"5小时额度";
        else if (winSecs > 6 * 3600 && winSecs <= 7 * 86400) label = @"周额度";
        else if (winSecs > 7 * 86400) label = [NSString stringWithFormat:@"%ld天额度", (long)(winSecs + 43200) / 86400];
        else label = @"账户额度";

        NSString *reset = @"";
        id resetAt = w[@"reset_at"];
        if ([resetAt isKindOfClass:[NSNumber class]] && [resetAt doubleValue] > 0) {
            NSDate *resetDate = [NSDate dateWithTimeIntervalSince1970:[resetAt doubleValue]];
            NSDateFormatter *df = [[NSDateFormatter alloc] init];
            [df setDateFormat:@"MM-dd HH:mm"];
            [df setTimeZone:[NSTimeZone localTimeZone]];
            reset = [NSString stringWithFormat:@"%@ 重置", [df stringFromDate:resetDate]];
        }
        // resetEpoch 供顶栏轮播的倒计时逐秒计算
        [windows addObject:@{@"label": label, @"pct": @(100.0 - usedPct), @"reset": reset,
                             @"resetEpoch": ([resetAt isKindOfClass:[NSNumber class]] ? @([resetAt doubleValue]) : @0.0)}];
    }
    if (windows.count == 0) return nil;

    NSString *email = [json[@"email"] isKindOfClass:[NSString class]] ? json[@"email"] : @"";
    BOOL limitReached = [rateLimit[@"limit_reached"] boolValue];
    return @{
        @"providerId": @"codex",
        @"account": email,
        @"status": limitReached ? @"限额中" : @"活跃",
        @"windows": windows
    };
}

// 将实时请求到的 Codex 窗口余量合并进账号列表（覆盖同账号或补建 Codex 条目）
static inline NSArray *MergeCodexLiveUsage(NSArray *accounts, NSDictionary *live) {
    if (!live.count) return accounts;
    NSMutableArray *out = accounts ? [accounts mutableCopy] : [NSMutableArray array];
    NSString *email = [live[@"account"] isKindOfClass:[NSString class]] ? live[@"account"] : @"";
    NSMutableDictionary *target = nil;
    for (NSMutableDictionary *it in out) {
        if ([it[@"providerId"] isKindOfClass:[NSString class]] &&
            [it[@"providerId"] isEqualToString:@"codex"]) {
            target = it;
            break;
        }
    }
    if (!target) {
        target = [NSMutableDictionary dictionary];
        target[@"provider"] = @"Codex (OpenAI)";
        target[@"providerId"] = @"codex";
        target[@"account"] = email;
        target[@"windows"] = [NSMutableArray array];
        [out addObject:target];
    }
    target[@"status"] = live[@"status"];
    target[@"windows"] = live[@"windows"];
    if (email.length > 0) target[@"account"] = email;
    return out;
}

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

// 将指定工具用量合并进周期分解列表（已存在则覆盖，否则插入并保持降序）
static inline void MergeToolEntry(NSMutableArray *tools, NSString *name, int64_t tokens, double costUsd) {
    if (tokens <= 0) return;
    for (NSMutableDictionary *it in tools) {
        if ([it[@"name"] isEqualToString:name]) {
            it[@"tokens"] = @(tokens);
            it[@"costUsd"] = @(costUsd);
            return;
        }
    }
    [tools addObject:[@{@"name": name, @"tokens": @(tokens), @"costUsd": @(costUsd)} mutableCopy]];
    [tools sortUsingComparator:^NSComparisonResult(NSDictionary *a, NSDictionary *b) {
        return [@([b[@"tokens"] longLongValue]) compare:@([a[@"tokens"] longLongValue])];
    }];
}

// ===== 直连 CLI 会话用量扫描（零假数据：只统计真实会话文件） =====
// tokenusage.db 的聚合依赖采集器运行，长期为 0；顶栏使用情况必须直接读取
// 各 CLI 的本地会话文件才能如实反映用量：
//   Codex: ~/.codex/sessions/**/rollout-*.jsonl —— event_msg/token_count 的
//          info.total_token_usage 为会话累计值，取文件内最后一条；缺失时累加
//          token_usage_record 的 payload.usage。日期取自文件名 rollout-YYYY-MM-DD-…
//   pi:    ~/.pi/agent/sessions/*/*.jsonl —— message.usage 为逐响应用量，逐条累加；
//          日期取自文件名前缀 YYYY-MM-DD
// 成本沿用 ZCode 合并的同一启发式口径（input*1.5 + output*6 计为 micros），
// pi 优先采用其自带的真实 cost.total。
// 返回：byDate[@"YYYY-MM-DD"][toolName] = @{@"tokens": n, @"micros": m}，30s 记忆缓存。
static inline void NativeUsageAccumulate(NSMutableDictionary *byDate, NSString *date, NSString *tool, int64_t tokens, int64_t micros) {
    if (date.length < 10 || tool.length == 0 || tokens <= 0) return;
    NSString *key = [date substringToIndex:10];
    // 写时复制：byDate 可能是记忆缓存的浅拷贝（内层字典与缓存共享），
    // 必须拷贝内层后再写，严禁原地改写共享字典导致跨调用叠加
    NSMutableDictionary *tools = [byDate[key] mutableCopy] ?: [NSMutableDictionary dictionary];
    NSDictionary *cur = tools[tool] ?: @{ @"tokens": @0, @"micros": @0 };
    tools[tool] = @{ @"tokens": @([cur[@"tokens"] longLongValue] + tokens),
                     @"micros": @([cur[@"micros"] longLongValue] + micros) };
    byDate[key] = tools;
}

// Codex 会话文件扫描（按 thread 去重，防 resume 续写文件重复计费）：
// - event_msg/token_count 的 info.total_token_usage 为会话累计值，同一 thread 跨
//   多个续写文件时取最大累计（而非按文件求和）；info 可能为 null，逐级判型
// - 文件无累计值时回退 token_usage_record 的逐响应用量，按 response_id 全局去重
//   （resume 重放的历史响应不会重复计入）
static inline void ScanCodexSessionFile(NSString *path, NSMutableDictionary *threadAcc) {
    NSString *name = path.lastPathComponent;
    if (name.length < 18 || ![name hasPrefix:@"rollout-"]) return;
    NSString *date = [name substringWithRange:NSMakeRange(8, 10)];

    NSFileHandle *fh = [NSFileHandle fileHandleForReadingAtPath:path];
    if (!fh) return;
    unsigned long long size = [fh seekToEndOfFile];
    unsigned long long offset = size > 262144 ? size - 262144 : 0; // 尾部 256KB 足够覆盖会话末尾的累计值
    [fh seekToFileOffset:offset];
    NSData *data = [fh readDataToEndOfFile];
    [fh closeFile];
    NSString *text = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
    if (!text) return;
    NSArray *lines = [text componentsSeparatedByString:@"\n"];

    NSString *threadId = nil;
    int64_t cumTotal = 0, cumIn = 0, cumOut = 0;
    int64_t recTokens = 0, recMicros = 0;
    NSMutableSet *seenResponses = nil;
    NSUInteger startIdx = offset > 0 ? 1 : 0; // 尾读时首行可能被截断
    for (NSUInteger i = startIdx; i < lines.count; i++) {
        NSData *lineData = [lines[i] dataUsingEncoding:NSUTF8StringEncoding];
        if (!lineData) continue;
        NSDictionary *obj = [NSJSONSerialization JSONObjectWithData:lineData options:0 error:nil];
        if (![obj isKindOfClass:[NSDictionary class]]) continue;
        NSDictionary *payload = [obj[@"payload"] isKindOfClass:[NSDictionary class]] ? obj[@"payload"] : nil;
        NSString *recordThread = [payload[@"thread_id"] isKindOfClass:[NSString class]] ? payload[@"thread_id"] : nil;
        if (recordThread.length > 0 && !threadId) threadId = recordThread;

        if ([obj[@"type"] isEqualToString:@"event_msg"] && [payload[@"type"] isEqualToString:@"token_count"]) {
            // info 可能为 null（无 token 统计的回合），逐级判型防止 NSNull 下标崩溃
            NSDictionary *info = [payload[@"info"] isKindOfClass:[NSDictionary class]] ? payload[@"info"] : nil;
            NSDictionary *ttu = [info[@"total_token_usage"] isKindOfClass:[NSDictionary class]] ? info[@"total_token_usage"] : nil;
            int64_t total = [ttu[@"total_tokens"] longLongValue];
            if (total > cumTotal) { // 累计值单调递增，取最大
                cumTotal = total;
                cumIn = [ttu[@"input_tokens"] longLongValue];
                cumOut = [ttu[@"output_tokens"] longLongValue];
            }
        } else if ([obj[@"type"] isEqualToString:@"token_usage_record"] && [payload[@"usage"] isKindOfClass:[NSDictionary class]]) {
            if (!seenResponses) seenResponses = [NSMutableSet set];
            NSString *rid = [payload[@"response_id"] isKindOfClass:[NSString class]] ? payload[@"response_id"] : nil;
            if (rid.length > 0 && [seenResponses containsObject:rid]) continue; // 重放去重
            if (rid.length > 0) [seenResponses addObject:rid];
            NSDictionary *u = payload[@"usage"];
            int64_t in = [u[@"input_tokens"] longLongValue];
            int64_t out = [u[@"output_tokens"] longLongValue];
            recTokens += in + out;
            recMicros += (int64_t)(in * 1.5 + out * 6.0);
        }
    }
    if (!threadId) threadId = name; // 无 thread 信息的旧格式：按文件名兜底去重

    NSMutableDictionary *acc = threadAcc[threadId] ?: [NSMutableDictionary dictionary];
    if (cumTotal > [acc[@"cum"] longLongValue]) {
        acc[@"cum"] = @(cumTotal);
        acc[@"in"] = @(cumIn);
        acc[@"out"] = @(cumOut);
        acc[@"date"] = date;
    }
    if (recTokens > 0) {
        acc[@"recT"] = @([acc[@"recT"] longLongValue] + recTokens);
        acc[@"recM"] = @([acc[@"recM"] longLongValue] + recMicros);
        acc[@"recDate"] = date;
    }
    threadAcc[threadId] = acc;
}

static inline void ScanPiSessionFile(NSString *path, NSMutableDictionary *byDate) {
    NSString *name = path.lastPathComponent;
    if (name.length < 10) return;
    NSString *date = [name substringToIndex:10];
    if ([date characterAtIndex:4] != '-') return;

    NSString *text = [NSString stringWithContentsOfFile:path encoding:NSUTF8StringEncoding error:nil];
    if (!text) return;
    int64_t tokens = 0;
    double costUsd = 0;
    for (NSString *line in [text componentsSeparatedByString:@"\n"]) {
        NSData *lineData = [line dataUsingEncoding:NSUTF8StringEncoding];
        if (!lineData) continue;
        NSDictionary *obj = [NSJSONSerialization JSONObjectWithData:lineData options:0 error:nil];
        if (![obj isKindOfClass:[NSDictionary class]] || ![obj[@"type"] isEqualToString:@"message"]) continue;
        NSDictionary *usage = [obj[@"message"] isKindOfClass:[NSDictionary class]] ? obj[@"message"][@"usage"] : nil;
        if (![usage isKindOfClass:[NSDictionary class]]) continue;
        tokens += [usage[@"totalTokens"] longLongValue];
        NSDictionary *cost = [usage[@"cost"] isKindOfClass:[NSDictionary class]] ? usage[@"cost"] : nil;
        double costValue = [cost[@"total"] doubleValue];
        if (costValue > 0) costUsd += costValue;
    }
    NativeUsageAccumulate(byDate, date, @"Pi", tokens, (int64_t)(costUsd * 1000000.0));
}

static inline NSDictionary *FetchNativeSessionUsageByDate(void) {
    static NSDictionary *memo = nil;
    static NSDate *memoAt = nil;
    static NSObject *lock = nil;
    static dispatch_once_t once;
    dispatch_once(&once, ^{ lock = [NSObject new]; });
    @synchronized (lock) {
        if (memo && memoAt && -[memoAt timeIntervalSinceNow] < 30.0) return memo;

        NSMutableDictionary *byDate = [NSMutableDictionary dictionary];
        NSMutableDictionary *threadAcc = [NSMutableDictionary dictionary];
        NSString *home = NSHomeDirectory();
        NSArray *roots = @[
            [home stringByAppendingPathComponent:@".codex/sessions"],
            [home stringByAppendingPathComponent:@".pi/agent/sessions"],
        ];
        NSFileManager *fm = [NSFileManager defaultManager];
        for (NSString *root in roots) {
            NSDirectoryEnumerator *enumerator = [fm enumeratorAtURL:[NSURL fileURLWithPath:root]
                                                 includingPropertiesForKeys:@[NSURLIsDirectoryKey]
                                                                    options:0
                                                               errorHandler:nil];
            for (NSURL *url in enumerator) {
                NSNumber *isDir = nil;
                [url getResourceValue:&isDir forKey:NSURLIsDirectoryKey error:nil];
                if (isDir.boolValue) continue;
                NSString *name = url.lastPathComponent;
                if ([name hasPrefix:@"rollout-"] && [name hasSuffix:@".jsonl"]) {
                    ScanCodexSessionFile(url.path, threadAcc);
                } else if (name.length >= 11 && [name characterAtIndex:4] == '-' && [name hasSuffix:@".jsonl"]) {
                    ScanPiSessionFile(url.path, byDate);
                }
            }
        }
        // Codex 按 thread 折叠：有会话累计值取最大累计（续写文件不重复计费）；
        // 无累计值的线程回退到 response_id 去重后的逐响应求和
        for (NSString *threadId in threadAcc) {
            NSDictionary *acc = threadAcc[threadId];
            if ([acc[@"cum"] longLongValue] > 0) {
                int64_t micros = (int64_t)([acc[@"in"] longLongValue] * 1.5 + [acc[@"out"] longLongValue] * 6.0);
                NativeUsageAccumulate(byDate, acc[@"date"], @"Codex", [acc[@"cum"] longLongValue], micros);
            } else if ([acc[@"recT"] longLongValue] > 0) {
                NativeUsageAccumulate(byDate, acc[@"recDate"], @"Codex", [acc[@"recT"] longLongValue], [acc[@"recM"] longLongValue]);
            }
        }
        // 内层以不可变快照入缓存：外部浅拷贝后写时复制，缓存永不被污染
        NSMutableDictionary *frozen = [NSMutableDictionary dictionary];
        for (NSString *dateKey in byDate) {
            frozen[dateKey] = [byDate[dateKey] copy];
        }
        memo = [frozen copy];
        memoAt = [NSDate date];
        return memo;
    }
}

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

    // 5) 查询并合并 ZCode 本地全量用量数据 (~/.zcode/cli/db/db.sqlite)
    // ZCode 按日期用量：并入直连扫描通道，供周期分解与趋势图统一消费
    NSMutableDictionary *zcodeByDate = [NSMutableDictionary dictionary];
    NSMutableArray *zcodeSessions = [NSMutableArray array];

    NSString *home = NSHomeDirectory();
    NSString *zcodeDbPath = [home stringByAppendingPathComponent:@".zcode/cli/db/db.sqlite"];
    if ([[NSFileManager defaultManager] fileExistsAtPath:zcodeDbPath]) {
        sqlite3 *zdb = NULL;
        if (sqlite3_open_v2([zcodeDbPath UTF8String], &zdb, SQLITE_OPEN_READONLY, NULL) == SQLITE_OK) {
            sqlite3_stmt *zstmt = NULL;

            const char *qZSess = "SELECT session_id, count(*), sum(computed_total_tokens), "
                                 "sum(input_tokens * 1.5 + output_tokens * 6.0), max(started_at) "
                                 "FROM model_usage GROUP BY session_id ORDER BY max(started_at) DESC LIMIT 5";
            if (sqlite3_prepare_v2(zdb, qZSess, -1, &zstmt, NULL) == SQLITE_OK) {
                while (sqlite3_step(zstmt) == SQLITE_ROW) {
                    const unsigned char *sid = sqlite3_column_text(zstmt, 0);
                    int64_t t = sqlite3_column_int64(zstmt, 2);
                    double c = sqlite3_column_double(zstmt, 3);
                    int64_t ts = sqlite3_column_int64(zstmt, 4);
                    NSDate *d = [NSDate dateWithTimeIntervalSince1970:ts / 1000.0];
                    NSString *actStr = [df stringFromDate:d];
                    [zcodeSessions addObject:@{
                        @"id": sid ? [NSString stringWithUTF8String:(const char *)sid] : @"",
                        @"source": @"zcode",
                        @"totalTokens": @(t),
                        @"costUsd": @(c / 1000000.0),
                        @"lastActive": actStr ?: @""
                    }];
                }
                sqlite3_finalize(zstmt);
            }

            // 按日期聚合 ZCode 用量：与直连 CLI 扫描走同一合并通道，保证
            // 周期分解与趋势图的口径一致
            const char *qZByDate = "SELECT date(started_at / 1000, 'unixepoch', 'localtime'), "
                                   "SUM(computed_total_tokens), SUM(input_tokens * 1.5 + output_tokens * 6.0) "
                                   "FROM model_usage GROUP BY 1";
            if (sqlite3_prepare_v2(zdb, qZByDate, -1, &zstmt, NULL) == SQLITE_OK) {
                while (sqlite3_step(zstmt) == SQLITE_ROW) {
                    const unsigned char *dateChars = sqlite3_column_text(zstmt, 0);
                    NSString *date = dateChars ? [NSString stringWithUTF8String:(const char *)dateChars] : @"";
                    NativeUsageAccumulate(zcodeByDate, date, @"ZCode",
                                          sqlite3_column_int64(zstmt, 1), sqlite3_column_int64(zstmt, 2));
                }
                sqlite3_finalize(zstmt);
            }
            sqlite3_close(zdb);
        }
    }

    // 直连 CLI 会话用量（Codex / pi 等）+ ZCode 按日期用量：补齐 tokenusage 库未覆盖的真实数据。
    // 按日期聚合到四个周期：日期字符串比较与上方 SQL 的 date >= ? 口径一致
    NSMutableDictionary *nativeByDate = [FetchNativeSessionUsageByDate() mutableCopy] ?: [NSMutableDictionary dictionary];
    for (NSString *date in zcodeByDate) {
        NSDictionary *tools = zcodeByDate[date];
        for (NSString *tool in tools) {
            NativeUsageAccumulate(nativeByDate, date, tool,
                                  [tools[tool][@"tokens"] longLongValue], [tools[tool][@"micros"] longLongValue]);
        }
    }
    NSMutableDictionary *nativeToday = [NSMutableDictionary dictionary];
    NSMutableDictionary *nativeWeek = [NSMutableDictionary dictionary];
    NSMutableDictionary *nativeMonth = [NSMutableDictionary dictionary];
    NSMutableDictionary *nativeAll = [NSMutableDictionary dictionary];
    for (NSString *date in nativeByDate) {
        NSDictionary *toolsOnDate = nativeByDate[date];
        for (NSString *tool in toolsOnDate) {
            NSDictionary *v = toolsOnDate[tool];
            int64_t t = [v[@"tokens"] longLongValue];
            int64_t m = [v[@"micros"] longLongValue];
            if (t <= 0) continue;
            void (^accumulate)(NSMutableDictionary *) = ^(NSMutableDictionary *bucket) {
                NSDictionary *cur = bucket[tool] ?: @{ @"tokens": @0, @"micros": @0 };
                bucket[tool] = @{ @"tokens": @([cur[@"tokens"] longLongValue] + t),
                                  @"micros": @([cur[@"micros"] longLongValue] + m) };
            };
            accumulate(nativeAll);
            if ([date compare:todayStr] >= 0) accumulate(nativeToday);
            if ([date compare:weekStr] >= 0) accumulate(nativeWeek);
            if ([date compare:monthStr] >= 0) accumulate(nativeMonth);
        }
    }
    void (^mergeNativeTotals)(NSDictionary *, int64_t *, int64_t *) = ^(NSDictionary *bucket, int64_t *tokensOut, int64_t *microsOut) {
        for (NSString *tool in bucket) {
            *tokensOut += [bucket[tool][@"tokens"] longLongValue];
            *microsOut += [bucket[tool][@"micros"] longLongValue];
        }
    };
    mergeNativeTotals(nativeToday, &todayTokens, &todayCostMicros);
    mergeNativeTotals(nativeWeek, &weekTokens, &weekCostMicros);
    mergeNativeTotals(nativeMonth, &monthTokens, &monthCostMicros);
    mergeNativeTotals(nativeAll, &allTokens, &allCostMicros);

    // 按周期的工具分解：today / thisWeek / thisMonth / allTime，各周期合并直连 CLI 与 DB 用量
    NSMutableArray *toolsAll = FetchToolDecomposition(db, nil, nil);
    NSMutableArray *toolsToday = FetchToolDecomposition(db, @"d.date >=", todayStr);
    NSMutableArray *toolsWeek = FetchToolDecomposition(db, @"d.date >=", weekStr);
    NSMutableArray *toolsMonth = FetchToolDecomposition(db, @"d.date >=", monthStr);
    for (NSString *tool in nativeToday) {
        MergeToolEntry(toolsToday, tool, [nativeToday[tool][@"tokens"] longLongValue], [nativeToday[tool][@"micros"] longLongValue] / 1000000.0);
    }
    for (NSString *tool in nativeWeek) {
        MergeToolEntry(toolsWeek, tool, [nativeWeek[tool][@"tokens"] longLongValue], [nativeWeek[tool][@"micros"] longLongValue] / 1000000.0);
    }
    for (NSString *tool in nativeMonth) {
        MergeToolEntry(toolsMonth, tool, [nativeMonth[tool][@"tokens"] longLongValue], [nativeMonth[tool][@"micros"] longLongValue] / 1000000.0);
    }
    for (NSString *tool in nativeAll) {
        MergeToolEntry(toolsAll, tool, [nativeAll[tool][@"tokens"] longLongValue], [nativeAll[tool][@"micros"] longLongValue] / 1000000.0);
    }
    NSMutableArray *tools = toolsAll; // 兼容旧字段：全量分解
    NSDictionary *toolsByPeriod = @{@"today": toolsToday, @"thisWeek": toolsWeek,
                                    @"thisMonth": toolsMonth, @"allTime": toolsAll};

    // 6) sessions: 合并 ZCode 会话与日常会话，按最近活跃时间倒序
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
    [sessions addObjectsFromArray:zcodeSessions];
    [sessions sortUsingComparator:^NSComparisonResult(NSDictionary *a, NSDictionary *b) {
        return [b[@"lastActive"] compare:a[@"lastActive"]];
    }];
    if (sessions.count > 5) {
        sessions = [[sessions subarrayWithRange:NSMakeRange(0, 5)] mutableCopy];
    }

    // 7) trends Top 7
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

    // 趋势并入直连 CLI 会话用量：DB 行缺失或为 0 的日期以本地扫描为准，
    // DB 已有真实行（采集器运行过）时以 DB 为准，避免双重计入
    if (nativeByDate.count > 0) {
        NSMutableDictionary *byDate = [NSMutableDictionary dictionary];
        for (NSDictionary *row in trends) {
            byDate[row[@"date"]] = row;
        }
        for (NSString *date in nativeByDate) {
            int64_t tokens = 0;
            int64_t micros = 0;
            for (NSString *tool in nativeByDate[date]) {
                tokens += [nativeByDate[date][tool][@"tokens"] longLongValue];
                micros += [nativeByDate[date][tool][@"micros"] longLongValue];
            }
            NSDictionary *dbRow = byDate[date];
            int64_t dbTokens = [dbRow[@"totalTokens"] longLongValue];
            if (dbRow == nil || dbTokens < tokens) {
                byDate[date] = @{@"date": date, @"totalTokens": @(tokens),
                                 @"costUsd": @((double)micros / 1000000.0)};
            }
        }
        trends = [[byDate allValues] mutableCopy];
        [trends sortUsingComparator:^NSComparisonResult(NSDictionary *a, NSDictionary *b) {
            return [b[@"date"] compare:a[@"date"]];
        }];
        if (trends.count > 7) {
            trends = [[trends subarrayWithRange:NSMakeRange(0, 7)] mutableCopy];
        }
    }

    // 8) worstLimit: 从真实额度窗口中提取最紧张余量；无真实数据时保持 nil（不造假）
    NSDictionary *worstLimit = nil;
    NSArray *accountsList = FetchLimitsFromLocalSqlite();
    double lowestPct = 101.0;
    for (NSDictionary *acc in accountsList) {
        NSArray *wins = [acc[@"windows"] isKindOfClass:[NSArray class]] ? acc[@"windows"] : @[];
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

    // 9) 最近活跃日数据
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
        @"tools": tools,
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
