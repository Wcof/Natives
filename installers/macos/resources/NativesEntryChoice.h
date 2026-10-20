// Launcher-only preference; no business state is stored in NSUserDefaults.
#import <AppKit/AppKit.h>

static NSString *NativesEntryText(NSString *zh, NSString *en) {
    return [NSLocale.preferredLanguages.firstObject hasPrefix:@"zh"] ? zh : en;
}

static NSString *NativesChooseEntry(BOOL chooseAgain) {
    NSUserDefaults *defaults = NSUserDefaults.standardUserDefaults;
    NSString *preferred = [defaults stringForKey:@"NativesPreferredEntry"];
    if (!chooseAgain && ([preferred isEqualToString:@"web"] || [preferred isEqualToString:@"extension"])) return preferred;
    NSAlert *alert = [NSAlert new];
    alert.messageText = NativesEntryText(@"打开 Natives", @"Open Natives");
    alert.informativeText = NativesEntryText(@"独立网页无需安装扩展。两种入口共用相同数据，可同时使用。按住 Option 再次启动可重新选择。", @"The local website works without an extension. Both entries share your data and can run together. Hold Option when launching to choose again.");
    [alert addButtonWithTitle:NativesEntryText(@"独立网页", @"Local website")];
    [alert addButtonWithTitle:NativesEntryText(@"Chrome 扩展", @"Chrome extension")];
    [alert addButtonWithTitle:NativesEntryText(@"取消", @"Cancel")];
    alert.showsSuppressionButton = YES;
    alert.suppressionButton.title = NativesEntryText(@"记住我的选择", @"Remember my choice");
    NSModalResponse response = alert.runModal;
    if (response != NSAlertFirstButtonReturn && response != NSAlertSecondButtonReturn) return nil;
    NSString *entry = response == NSAlertFirstButtonReturn ? @"web" : @"extension";
    if (alert.suppressionButton.state == NSControlStateValueOn) [defaults setObject:entry forKey:@"NativesPreferredEntry"];
    else [defaults removeObjectForKey:@"NativesPreferredEntry"];
    return entry;
}
