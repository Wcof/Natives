#import <AppKit/AppKit.h>
#import <WebKit/WebKit.h>
#import "NativesStatusBar.h"
#import "NativesStatusBarData.h"

@interface NativesStatusBar (TestEntry)
- (NSPanel *)ensurePanel;
- (void)fetchPanelData;
- (void)shutdownModelHostConnection;
- (void)modelHostDidExit:(NSTask *)finished;
- (NSDictionary *)selectDisplayWindow:(NSArray *)windows;
- (NSString *)quotaWindowShortLabel:(NSString *)label;
@end

@interface FixtureBar : NativesStatusBar
@property (atomic) NSUInteger reads;
@property (nonatomic) NSUInteger liveRefreshes;
@property (nonatomic) NSMutableArray *messages;
@end
@implementation FixtureBar
- (BOOL)ensureModelHostAlive { return YES; }
- (NSDictionary *)readUsageState {
    self.reads++;
    [NSThread sleepForTimeInterval:0.05];
    return StatusBarUsageSummary(@{@"totalTokens": @1234, @"costUsd": @0, @"priced": @NO}, @{});
}
- (NSDictionary *)readPanelState {
    [NSThread sleepForTimeInterval:0.05];
    NSMutableDictionary *state = [[self readUsageState] mutableCopy];
    state[@"panel"] = @{@"today": @{@"totalTokens": @1234, @"costUsd": @0, @"priced": @NO}};
    return state;
}
- (void)fetchLimitsWithCompletion:(void (^)(NSArray *))completion live:(BOOL)live {
    if (live) self.liveRefreshes++;
    NSArray *items = @[@{@"providerId": @"codex", @"windows": @[@{@"label": @"Weekly limit", @"pct": @42}]}];
    [self setValue:items forKey:@"lastQuotaAccounts"];
    completion(items);
}
- (void)postToPanel:(NSDictionary *)message { if (!self.messages) self.messages = [NSMutableArray array]; [self.messages addObject:message]; }
@end

static void WaitUntil(BOOL (^done)(void)) {
    NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:3];
    while (!done() && [deadline timeIntervalSinceNow] > 0) [[NSRunLoop mainRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.01]];
    NSCAssert(done(), @"native fixture timed out");
}

int main(void) {
    @autoreleasepool {
        [NSApplication sharedApplication];
        FixtureBar *bar = [FixtureBar new];
        NSCAssert([[bar valueForKey:@"worstLimitPercent"] intValue] == -1, @"missing quota must not initialize as exhausted zero");
        [bar setupStatusItem];
        [bar fetchAndUpdateState]; [bar fetchAndUpdateState];
        WaitUntil(^BOOL { return ![[bar valueForKey:@"usageFetchInFlight"] boolValue]; });
        NSCAssert(bar.reads == 1, @"overlapping automatic queries are coalesced");
        NSCAssert([[bar valueForKey:@"latestTokensText"] isEqualToString:@"1.2K"], @"automatic refresh updates the real NSStatusItem owner");
        NSCAssert([[bar valueForKey:@"latestCostText"] isEqualToString:@"--"], @"unknown cost stays unknown");
        NSCAssert(bar.statusItem.button.image != nil, @"real system status item is rendered");
        NSCAssert(bar.liveRefreshes == 0, @"startup must not probe upstream quotas");
        NSCAssert([[bar quotaWindowShortLabel:@"Weekly limit"] isEqual:@"周"], @"English quota labels use the same window classification");
        NSCAssert([[bar valueForKey:@"worstLimitPercent"] intValue] == 42, @"bars mode receives real quota values");
        NSCAssert(NativesStatusBarTargetURL(@"bad$(command)") == nil, @"unknown bridge targets are rejected");
        NSCAssert([NativesStatusBarTargetURL(@"tokenusage").absoluteString hasSuffix:@"app.html?app=tokenusage"], @"usage dashboard preserves the registered module entry");
        NSUserDefaults *defaults = NSUserDefaults.standardUserDefaults;
        NSDictionary *arguments = [defaults volatileDomainForName:NSArgumentDomain];
        [defaults setVolatileDomain:@{@"NativesPreferredEntry": @"web"} forName:NSArgumentDomain];
        NSCAssert(NativesStatusBarUsesWeb(), @"status bar consumes the launcher's saved entry choice");
        NSCAssert([NativesStatusBarTargetURL(@"files").scheme isEqual:@"http"], @"Web preference opens the local service");
        [defaults setVolatileDomain:@{@"NativesPreferredEntry": @"extension"} forName:NSArgumentDomain];
        NSCAssert(!NativesStatusBarUsesWeb(), @"extension entry choice remains supported");
        [defaults setVolatileDomain:arguments forName:NSArgumentDomain];
        [bar ensurePanel];
        [bar fetchPanelData];
        [bar closePanel];
        WaitUntil(^BOOL { return ![[bar valueForKey:@"panelFetchInFlight"] boolValue]; });
        NSPredicate *panelMessage = [NSPredicate predicateWithBlock:^BOOL(NSDictionary *message, NSDictionary *bindings) { return [message[@"type"] isEqual:@"panel"]; }];
        NSCAssert([[bar.messages filteredArrayUsingPredicate:panelMessage] count] == 0, @"old requests cannot write a closed/reopened panel");
        [bar fetchPanelData];
        WaitUntil(^BOOL { return ![[bar valueForKey:@"panelFetchInFlight"] boolValue]; });
        NSCAssert([[bar.messages filteredArrayUsingPredicate:panelMessage] count] == 1, @"fresh panel request is delivered");
        [bar updateWithStateJson:@"{\"displayText\":\"42\",\"worstLimit\":{\"remainingPercent\":null,\"providerId\":\"invalid\"}}"];
        NSCAssert([[bar valueForKey:@"latestCostText"] isEqual:@"--"], @"single token value must not become a currency amount");
        NSCAssert([[bar valueForKey:@"worstLimitPercent"] intValue] == -1, @"malformed quota remains unavailable");
        NSTask *oldTask = [NSTask new], *newTask = [NSTask new];
        [bar setValue:newTask forKey:@"modelHostTask"];
        [bar modelHostDidExit:oldTask];
        NSCAssert([bar valueForKey:@"modelHostTask"] == newTask, @"old termination cannot clear the new worker owner");
        [bar modelHostDidExit:newTask];
        NSCAssert([bar valueForKey:@"modelHostTask"] == nil, @"matching worker owner is released");
        [bar shutdownModelHostConnection];
        [[NSStatusBar systemStatusBar] removeStatusItem:bar.statusItem];
        NSLog(@"statusbar native owner: real item, single-flight, stale-panel rejection, quotas and safe routes passed");
    }
    return 0;
}
