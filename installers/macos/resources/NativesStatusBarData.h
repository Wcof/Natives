// NativesStatusBarData.h: tokenusage.db 只读兜底与账号额度聚合（额度走 8766 查询面，ADR-0033）
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
            SortQuotaWindows5HFirst(finalWindows);
            trayWindows = finalWindows;
        } else {
            SortQuotaWindows5HFirst(trayWindows);
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
    for (NSMutableDictionary *it in [accMap allValues]) {
        if ([it[@"windows"] isKindOfClass:[NSMutableArray class]]) {
            SortQuotaWindows5HFirst(it[@"windows"]);
        }
    }
    [accounts addObjectsFromArray:[accMap allValues]];
    return accounts;
}

// ===== AI 数据域查询客户端（ADR-0033 Phase 2 / 审计 B2）=====
// 顶栏不再直连上游额度端点（Codex wham / Google cloudcode-pa）、不再直读
// Keychain 续期、不再第二写方写 quota-cache.json（B2 退役）。额度数据路径：
//   手动刷新 → POST 127.0.0.1:8766/ai/quota/refresh（唯一上游触发口，model-host 执行）
//           → GET  127.0.0.1:8766/ai/quota（读 Go 侧最近探测结果）
//   8766 不可达 → 读 quota-cache.json（同一 Go 写方的缓存文件，离线兜底，只读）
//   两者皆空   → 空列表 + 面板显示"模型服务未运行"（零假数据）

// 解析 /ai/quota 响应（QuotaResult 数组）为账号窗口行，形状与
// FetchQuotaCacheAccounts 一致（provider/providerId/account/status/windows）。
static inline NSArray *ParseAIQueryQuotaAccounts(NSData *data) {
    id json = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
    NSArray *results = [json isKindOfClass:[NSDictionary class]] ? json[@"results"] : nil;
    if (![results isKindOfClass:[NSArray class]]) return @[];

    NSISO8601DateFormatter *iso = [[NSISO8601DateFormatter alloc] init];
    NSDateFormatter *shortFmt = [[NSDateFormatter alloc] init];
    shortFmt.dateFormat = @"MM-dd HH:mm";
    NSMutableArray *items = [NSMutableArray array];
    for (NSDictionary *entry in results) {
        if (![entry isKindOfClass:[NSDictionary class]]) continue;
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
            if ([group containsString:@"Gemini"]) row[@"group"] = @"Gemini 模型";
            else if ([group containsString:@"Claude"]) row[@"group"] = @"Claude 与 GPT 模型";
            NSString *resetISO = [w[@"resetTime"] isKindOfClass:[NSString class]] ? w[@"resetTime"] : @"";
            if (resetISO.length > 0) {
                NSDate *resetDate = [iso dateFromString:resetISO];
                if (resetDate) {
                    row[@"reset"] = [NSString stringWithFormat:@"%@ 重置", [shortFmt stringFromDate:resetDate]];
                    row[@"resetEpoch"] = @(resetDate.timeIntervalSince1970);
                }
            }
            [trayWindows addObject:row];
        }
        if (trayWindows.count == 0) continue;
        SortQuotaWindows5HFirst(trayWindows);
        [items addObject:@{
            @"provider": [provider capitalizedString],
            @"providerId": provider,
            @"account": account,
            @"status": @"活跃",
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
            dispatch_semaphore_signal(psem);
        }] resume];
        dispatch_semaphore_wait(psem, dispatch_time(DISPATCH_TIME_NOW, 35 * NSEC_PER_SEC));
        [psess finishTasksAndInvalidate];
    }

    NSMutableURLRequest *req = [NSMutableURLRequest requestWithURL:
        [NSURL URLWithString:[base stringByAppendingString:@"/ai/quota"]]];
    req.timeoutInterval = 3.0;
    __block NSData *respData = nil;
    dispatch_semaphore_t sem = dispatch_semaphore_create(0);
    NSURLSession *sess = [NSURLSession sessionWithConfiguration:[NSURLSessionConfiguration ephemeralSessionConfiguration]];
    [[sess dataTaskWithRequest:req completionHandler:^(NSData *d, NSURLResponse *r, NSError *e) {
        respData = d;
        dispatch_semaphore_signal(sem);
    }] resume];
    dispatch_semaphore_wait(sem, dispatch_time(DISPATCH_TIME_NOW, 5 * NSEC_PER_SEC));
    [sess finishTasksAndInvalidate];

    if (respData) {
        NSArray *accounts = ParseAIQueryQuotaAccounts(respData);
        if (accounts.count > 0) {
            if (quotaSource) *quotaSource = @"8766";
            return accounts;
        }
    }
    // 8766 不可达（模型服务未运行）：缓存文件兜底（同一 Go 写方，只读）
    NSArray *cached = FetchQuotaCacheAccounts();
    if (quotaSource) *quotaSource = cached.count > 0 ? @"cache" : @"none";
    return cached;
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
    NSArray *accountsList = FetchLimitsFromLocalSqlite();
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

