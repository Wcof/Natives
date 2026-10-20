#import <Foundation/Foundation.h>
#import <sqlite3.h>

#import "NativesStatusBarData.h"
#import "NativesStatusBarPanel.h"

static void Exec(sqlite3 *db, NSString *sql) {
    char *error = NULL;
    NSCAssert(sqlite3_exec(db, sql.UTF8String, NULL, NULL, &error) == SQLITE_OK,
              @"SQL failed: %s", error ?: "unknown");
    sqlite3_free(error);
}

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        NSString *path = [NSTemporaryDirectory() stringByAppendingPathComponent:
            [NSString stringWithFormat:@"natives-statusbar-%@.db", NSUUID.UUID.UUIDString]];
        sqlite3 *db = NULL;
        NSCAssert(sqlite3_open(path.UTF8String, &db) == SQLITE_OK, @"open fixture db");
        Exec(db, @"CREATE TABLE usage_events (requested_at TEXT NOT NULL, total_tokens INTEGER NOT NULL, cost_micro INTEGER NOT NULL, tool_id TEXT NOT NULL, source_instance_id TEXT NOT NULL DEFAULT '', source TEXT NOT NULL, provider TEXT NOT NULL, session_id TEXT NOT NULL, model TEXT NOT NULL DEFAULT '', model_alias TEXT NOT NULL DEFAULT '', input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0, cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_write_tokens INTEGER NOT NULL DEFAULT 0)");
        Exec(db, @"CREATE TABLE model_prices (id TEXT, provider_id TEXT DEFAULT '', model_id TEXT NOT NULL, input_price_micro INTEGER NOT NULL, output_price_micro INTEGER NOT NULL, cache_read_price_micro INTEGER NOT NULL, cache_write_price_micro INTEGER NOT NULL, source TEXT NOT NULL, updated_at TEXT NOT NULL DEFAULT '')");
        Exec(db, @"CREATE TABLE usage_subjects (id TEXT PRIMARY KEY, kind TEXT NOT NULL, label TEXT NOT NULL DEFAULT '')");
        // 今日事件：带存储计费 $0.25（cost_micro 非 0 → 不做动态估算）
        Exec(db, @"INSERT INTO usage_events (requested_at, total_tokens, cost_micro, tool_id, source, provider, session_id, model, input_tokens) VALUES (strftime('%Y-%m-%dT%H:%M:%SZ','now'),100,250000,'codex','cli','openai','sess-live','gpt-5.4',100)");
        // 40 天前事件：零计费 → 按目录动态估算 (1000*15 + 200*75) = $0.03
        Exec(db, @"INSERT INTO usage_events (requested_at, total_tokens, cost_micro, tool_id, source, provider, session_id, model, input_tokens, output_tokens) VALUES (strftime('%Y-%m-%dT%H:%M:%SZ','now','-40 days'),1200,0,'claude','cli','anthropic','sess-old','claude-opus-4-1',1000,200)");
        // 今日事件：价格目录未覆盖的模型 → priced=false，面板须显示 '--'
        Exec(db, @"INSERT INTO usage_events (requested_at, total_tokens, cost_micro, tool_id, source, provider, session_id, model, input_tokens) VALUES (strftime('%Y-%m-%dT%H:%M:%SZ','now','localtime','start of day','utc'),500,0,'zcode','cli','bigmodel','sess-noprice','GLM-x-Flash',500)");
        Exec(db, @"INSERT INTO model_prices (model_id, input_price_micro, output_price_micro, cache_read_price_micro, cache_write_price_micro, source) VALUES ('gpt-5.4',2500000,15000000,250000,0,'synced')");
        Exec(db, @"INSERT INTO model_prices (model_id, input_price_micro, output_price_micro, cache_read_price_micro, cache_write_price_micro, source) VALUES ('claude-opus-4-1',15000000,75000000,1500000,18750000,'synced')");
        Exec(db, @"INSERT INTO usage_subjects VALUES ('sess-live','session','修复顶栏会话标题')");
        sqlite3_close(db);

        NSDictionary *state = FetchStateFromUsageDbAtPath(path);
        NSCAssert(state != nil, @"offline usage fallback must return state");
        NSDictionary *panel = state[@"panel"];
        NSCAssert([panel[@"today"][@"totalTokens"] longLongValue] == 600, @"today total");
        NSCAssert([panel[@"allTime"][@"totalTokens"] longLongValue] == 1800, @"all-time total");
        NSCAssert(fabs([panel[@"allTime"][@"costUsd"] doubleValue] - 0.28) < 1e-9,
                  @"all-time cost = stored $0.25 + estimated $0.03");
        NSCAssert(![panel[@"today"][@"priced"] boolValue], @"partially priced period must not claim a complete amount");
        NSCAssert([state[@"displayText"] isEqualToString:@"600 · --"], @"status text preserves partial cost");
        NSCAssert([state[@"usageSource"] isEqualToString:@"database"], @"fallback source label");
        NSCAssert(FetchStateFromUsageDbAtPath(@"/definitely/missing/usage.db") == nil,
                  @"missing db must remain an explicit offline state");

        // 使用情况：按模型聚合、按 token 降序，金额含动态估算
        NSArray *models = panel[@"toolsByPeriod"][@"allTime"];
        NSCAssert(models.count == 3, @"three model rows");
        NSCAssert([models[0][@"name"] isEqualToString:@"claude-opus-4-1"], @"models sorted by tokens");
        NSCAssert([models[0][@"tokens"] longLongValue] == 1200, @"model token sum");
        NSCAssert(fabs([models[0][@"costUsd"] doubleValue] - 0.03) < 1e-9,
                  @"zero-cost model estimated from price catalog");
        NSCAssert([models[0][@"priced"] boolValue], @"catalog-covered model is priced");
        NSCAssert([models[1][@"name"] isEqualToString:@"GLM-x-Flash"], @"uncataloged model present");
        NSCAssert(![models[1][@"priced"] boolValue], @"uncataloged model must not claim pricing");
        NSCAssert([models[2][@"name"] isEqualToString:@"gpt-5.4"], @"stored-cost model present");
        NSCAssert(fabs([models[2][@"costUsd"] doubleValue] - 0.25) < 1e-9, @"stored cost preserved");
        NSCAssert([models[2][@"priced"] boolValue], @"stored cost is priced");

        // 最近会话：标题优先显示，无标题回退会话 ID；带工具维度与金额
        NSCAssert([panel[@"sessionsByPeriod"][@"today"] count] == 2, @"today sessions exclude old history");
        NSArray *sessions = panel[@"sessionsByPeriod"][@"allTime"];
        NSCAssert(sessions.count == 3, @"three session rows");
        NSCAssert([sessions[0][@"id"] isEqualToString:@"修复顶栏会话标题"], @"session title preferred over raw id");
        NSCAssert([sessions[0][@"tool"] isEqualToString:@"codex"], @"session tool dimension");
        NSCAssert(fabs([sessions[0][@"costUsd"] doubleValue] - 0.25) < 1e-9, @"session stored cost");
        NSCAssert([sessions[1][@"tool"] isEqualToString:@"zcode"], @"second session tool");
        NSCAssert(![sessions[1][@"priced"] boolValue], @"uncataloged session must not claim pricing");
        NSCAssert([sessions[2][@"id"] isEqualToString:@"sess-old"], @"untitled session falls back to id");
        NSCAssert(fabs([sessions[2][@"costUsd"] doubleValue] - 0.03) < 1e-9, @"session estimated cost");

        NSDictionary *(^fixtureFetch)(NSString *) = ^NSDictionary *(NSString *query) {
            NSCAssert([query containsString:@"&timezone="], @"all usage queries must use local timezone");
            BOOL all = [query containsString:@"range=all"];
            if ([query containsString:@"overview"]) return @{@"metrics": @{@"totalTokens": all ? @1200 : @0, @"estimatedCostUsd": @0, @"priced": @NO}, @"trend": [query containsString:@"range=custom"] ? @[@{@"timestamp": @"2026-10-13T00:00:00Z", @"tokens": @10}] : @[]};
            if ([query containsString:@"analysis"]) return @{@"byModel": all ? @[@{@"name": @"gpt-6.1-sol", @"tokens": @1200}] : @[]};
            return @{@"sessions": all ? @[@{@"toolId": @"codex", @"title": @"历史会话", @"tokens": @1200}] : @[]};
        };
        NSDictionary *queried = FetchStateViaAIQueryWithFetcher(fixtureFetch);
        NSCAssert([queried[@"panel"][@"toolsByPeriod"][@"today"] count] == 0, @"empty today models stay empty");
        NSCAssert([queried[@"panel"][@"sessionsByPeriod"][@"today"] count] == 0, @"empty today sessions stay empty");
        NSCAssert([queried[@"panel"][@"toolsByPeriod"][@"allTime"] count] == 1, @"all-time models preserved");
        NSCAssert([queried[@"panel"][@"trends"][0][@"totalTokens"] intValue] == 10, @"trend token schema maps to panel");
        NSCAssert([queried[@"panel"][@"trends"][0][@"date"] isEqual:@"2026-10-13"], @"trend timestamp maps to local bucket label");
        NSCAssert(FetchStateViaAIQueryWithFetcher(^NSDictionary *(NSString *query) {
            return [query containsString:@"range=today"] ? nil : fixtureFetch(query);
        }) == nil, @"failed today request must not produce fake zero");
        NSCAssert(FetchStateViaAIQueryWithFetcher(^NSDictionary *(NSString *query) {
            return [query containsString:@"overview"] ? @{@"metrics": @{@"totalTokens": NSNull.null}} : fixtureFetch(query);
        }) == nil, @"malformed usage metrics cannot become zero");

        NSCAssert(sqlite3_open(path.UTF8String, &db) == SQLITE_OK, @"reopen fixture");
        Exec(db, @"UPDATE usage_events SET model_alias='display-only-alias' WHERE model='claude-opus-4-1'");
        Exec(db, @"INSERT INTO model_prices (provider_id,model_id,input_price_micro,output_price_micro,cache_read_price_micro,cache_write_price_micro,source) VALUES ('unrelated','claude-opus-4-1',100000000,500000000,0,0,'manual'),('','claude-opus-4-1',1000000,2000000,0,0,'manual_global'),('anthropic','claude-opus-4-1',2000000,3000000,0,0,'manual')");
        NSArray *pricedModels = ReadUsageModels(db, @"all");
        NSDictionary *aliasedModel = pricedModels[0];
        NSCAssert([aliasedModel[@"name"] isEqual:@"display-only-alias"], @"model alias is display-only");
        NSCAssert(fabs([aliasedModel[@"costUsd"] doubleValue] - 0.0026) < 1e-9, @"provider manual price wins as a complete row, not column maxima");
        NSCAssert([ReadUsageTrend(db) count] <= 2, @"old dates are excluded from the seven-calendar-day trend");
        Exec(db, @"DELETE FROM usage_events WHERE cost_micro != 0 OR model != 'GLM-x-Flash'");
        sqlite3_close(db);
        NSCAssert([FetchStateFromUsageDbAtPath(path)[@"displayText"] hasSuffix:@" · --"], @"offline unpriced amount stays unknown");
        NSCAssert(sqlite3_open(path.UTF8String, &db) == SQLITE_OK, @"open small-cost fixture");
        Exec(db, @"INSERT INTO model_prices (provider_id,model_id,input_price_micro,output_price_micro,cache_read_price_micro,cache_write_price_micro,source) VALUES ('fixture','tiny',150000,0,0,0,'manual')");
        Exec(db, @"WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<100) INSERT INTO usage_events(requested_at,total_tokens,cost_micro,tool_id,source,provider,session_id,model,input_tokens) SELECT strftime('%Y-%m-%dT%H:%M:%SZ','now'),1,0,'fixture','test','fixture','tiny','tiny',1 FROM n");
        NSDictionary *tiny = ReadUsageModels(db, @"all")[1];
        NSCAssert([tiny[@"name"] isEqual:@"tiny"] && fabs([tiny[@"costUsd"] doubleValue] - 0.000015) < 1e-12, @"small costs aggregate before integer scaling");
        Exec(db, @"DROP TABLE usage_subjects");
        sqlite3_close(db);
        NSCAssert(FetchStateFromUsageDbAtPath(path) == nil, @"failed offline session query cannot look like an empty list");

        NSData *quotaJSON = [@"{\"entries\":{\"cred\":{\"provider\":\"antigravity\",\"account\":\"test\",\"status\":\"success\",\"windows\":[{\"name\":\"Weekly Limit Remaining\",\"remainingPercent\":42}]}}}" dataUsingEncoding:NSUTF8StringEncoding];
        NSArray *accounts = ParseAIQueryQuotaAccounts(quotaJSON);
        NSCAssert(accounts.count == 1, @"quota entries response must produce one account");
        NSCAssert([accounts[0][@"windows"][0][@"pct"] integerValue] == 42, @"quota percentage");
        NSData *quotaEdges = [@"{\"entries\":{\"broken\":42,\"failed\":{\"status\":\"error\"},\"valid\":{\"status\":\"success\",\"provider\":\"codex\",\"windows\":[{\"name\":\"Weekly\",\"remainingPercent\":42},{\"name\":\"Weekly\",\"remainingPercent\":0,\"resetTime\":\"2026-10-13T20:00:00.123Z\"},{\"name\":\"Invalid\",\"remainingPercent\":101}]}}}" dataUsingEncoding:NSUTF8StringEncoding];
        NSArray *edgeAccounts = ParseAIQueryQuotaAccounts(quotaEdges);
        NSCAssert(edgeAccounts.count == 1 && [edgeAccounts[0][@"windows"] count] == 1, @"invalid entries rejected and duplicate windows collapsed");
        NSCAssert([edgeAccounts[0][@"windows"][0][@"pct"] intValue] == 0, @"real zero quota is preserved");
        NSCAssert(edgeAccounts[0][@"windows"][0][@"resetEpoch"] != nil, @"fractional quota reset time is parsed");
        NSCAssert(ParseAIQueryQuotaAccounts(nil).count == 0, @"missing quota body is unavailable");
        NSString *originalRoot = NSProcessInfo.processInfo.environment[@"NATIVES_MODEL_HOST_CONFIG_DIR"];
        setenv("NATIVES_MODEL_HOST_CONFIG_DIR", "relative-fixture", 1);
        NSCAssert(FetchStateFromUsageDb() == nil && FetchQuotaCacheAccounts().count == 0, @"invalid isolated root never falls back to production data");
        if (originalRoot) setenv("NATIVES_MODEL_HOST_CONFIG_DIR", originalRoot.UTF8String, 1); else unsetenv("NATIVES_MODEL_HOST_CONFIG_DIR");
        NSCAssert([AIUsageQueryPath(@"/ai/usage/overview?range=today", @"Etc/GMT+5") containsString:@"timezone=Etc/GMT%2B5"], @"IANA timezone plus sign must not decode as a space");
        NSCAssert([edgeAccounts[0][@"status"] isEqual:@"缓存"], @"read-only quota results do not claim a live probe");

        // RFC3339 解析：model-host lastAt 实际形状带毫秒，此前 yyyy-MM-dd'T'HH:mm:ss
        // 格式解析失败导致最近会话丢失时间显示
        NSCAssert(NativeParseISO8601(@"2026-09-29T02:09:07.431Z") != nil, @"lastAt with milliseconds must parse");
        NSCAssert(NativeParseISO8601(@"2026-09-29T02:09:07Z") != nil, @"lastAt without milliseconds must parse");
        NSCAssert(NativeParseISO8601(@"") == nil, @"empty timestamp must not parse");
        NSCAssert(NativeParseISO8601(@"not-a-date") == nil, @"garbage timestamp must not parse");
        NSDateFormatter *sessionOut = [[NSDateFormatter alloc] init];
        sessionOut.dateFormat = @"MM-dd HH:mm";
        sessionOut.timeZone = [NSTimeZone timeZoneForSecondsFromGMT:0];
        NSCAssert([[sessionOut stringFromDate:NativeParseISO8601(@"2026-09-29T02:09:07.431Z")]
                  isEqualToString:@"09-29 02:09"], @"parsed lastAt should render as compact UTC time");

        // model-host 二进制定位：manifest path 字段必须被读出（顶栏按需拉起的依赖）
        NSString *manifestDir = [NSTemporaryDirectory() stringByAppendingPathComponent:
            [NSString stringWithFormat:@"natives-statusbar-manifest-%@", NSUUID.UUID.UUIDString]];
        NSCAssert([[NSFileManager defaultManager] createDirectoryAtPath:manifestDir
                                            withIntermediateDirectories:YES attributes:nil error:nil],
                  @"create manifest fixture dir");
        NSString *fakeHost = [manifestDir stringByAppendingPathComponent:@"fake-model-host"];
        NSCAssert([@"" writeToFile:fakeHost atomically:YES encoding:NSUTF8StringEncoding error:nil],
                  @"write fake model-host file");
        NSDictionary *manifestFixture = @{@"name": @"com.natives.model_host", @"type": @"stdio",
                                          @"path": fakeHost, @"allowed_origins": @[]};
        NSData *manifestJSON = [NSJSONSerialization dataWithJSONObject:manifestFixture options:0 error:nil];
        NSCAssert([manifestJSON writeToFile:[manifestDir stringByAppendingPathComponent:@"com.natives.model_host.json"]
                                atomically:YES], @"write manifest fixture");
        NSArray *candidates = ModelHostExecutableCandidates(manifestDir);
        NSCAssert(candidates.count == 2, @"same-directory candidate plus manifest candidate");
        NSCAssert([candidates[0] hasSuffix:@"/model-host"],
                  @"first candidate is executable-sibling model-host");
        NSCAssert([candidates[1] isEqualToString:fakeHost],
                  @"manifest path field must be surfaced as candidate");
        NSArray *bareCandidates = ModelHostExecutableCandidates(
            [NSTemporaryDirectory() stringByAppendingPathComponent:@"natives-statusbar-missing-dir"]);
        NSCAssert(bareCandidates.count == 1 && [bareCandidates[0] hasSuffix:@"/model-host"],
                  @"missing manifest dir still yields the same-directory candidate");
        [[NSFileManager defaultManager] removeItemAtPath:manifestDir error:nil];

        // 周期切换（面板）：会话行映射为纯函数，可离线回归；
        // sessionsByPeriod 由 8766 路径按周期独立查询后装配
        NSArray *mapped = SessionRowsFromResponse(@{@"sessions": @[@{
            @"toolId": @"codex", @"title": @"周期会话", @"tokens": @42,
            @"costUsd": @0.25, @"priced": @YES, @"lastAt": @"2026-09-30T01:02:03.456Z"},
            @{@"toolId": @"zcode", @"tokens": @7, @"lastAt": @""}]});
        NSCAssert(mapped.count == 2, @"session rows mapped from response");
        NSCAssert([mapped[0][@"id"] isEqualToString:@"周期会话"], @"title mapping preserved");
        NSCAssert([mapped[0][@"totalTokens"] longLongValue] == 42, @"token mapping preserved");
        NSCAssert([mapped[1][@"id"] isEqualToString:@"zcode"], @"untitled falls back to tool id");

        NSString *panelHTML = NativesStatusBarPanelHTML();
        if (argc > 1) NSCAssert([panelHTML writeToFile:[NSString stringWithUTF8String:argv[1]]
            atomically:YES encoding:NSUTF8StringEncoding error:nil], @"export production panel for browser regression");
        NSCAssert([panelHTML containsString:@"本地数据"], @"offline usage source feedback");
        NSCAssert([panelHTML containsString:@"显示上次缓存额度"], @"cached quota feedback");
        // 面板周期切换：会话区按页签读取 sessionsByPeriod，页签切换联动 renderSessions
        NSCAssert([panelHTML containsString:@"sessionsByPeriod"],
                  @"panel must consume per-period session queries");
        NSCAssert([panelHTML containsString:@"renderTotal();renderTools();renderSessions();"],
                  @"period tab switch must re-render sessions");
        NSCAssert([panelHTML containsString:@"sessMore"], @"session section exposes period label");
        // 额度卡：下行展示今日消耗金额，禁止再同组堆叠第二条额度
        NSCAssert([panelHTML containsString:@"今日消耗"], @"quota card shows consumed amount row");
        NSCAssert([panelHTML containsString:@"(TODAY_COST?fmtCP(TODAY_COST.costUsd,TODAY_COST.priced):'--')"],
                  @"missing panel data must render '--' not a fake $0.00");
        NSCAssert([panelHTML containsString:@"m-kind cost-kind"], @"consumed row is styled");

        [[NSFileManager defaultManager] removeItemAtPath:path error:nil];
        NSLog(@"statusbar offline usage fallback passed");
    }
    return 0;
}
