import Cocoa
import ApplicationServices

// APPV2-T05 真机 Spike：钉钉 / 微信 / ChatGPT 的 AX 窗口控制能力证据。
//
// 非破坏性：位置写测试只做「把 AXPosition 设为当前值」的 no-op 写 + 读回验证，
// 不挪动用户窗口、不改窗口大小、不触碰任何应用内部状态。
//
// 证据口径（typed，供 T06/T07 的 capability 判定）：
//   trusted        AXIsProcessTrusted（0/1）
//   running / pid  应用是否在运行
//   wins           AXWindows 数量
//   main           主窗口 AXSubrole（AXStandardWindow = 可归位标准窗口）
//   pos / size     主窗口当前 position/size
//   pSet / sSet    AXPosition / AXSize 是否 settable（0/1）
//   full           AXFullScreen（-1 = 属性不可读）
//   sheet          主窗口是否存在 AXSheet 子元素
//   set            写测试：ok_samepos / not_settable / err<AXError> / not_running
//   rb             读回验证：match / mismatch / -
//
// 编译：swiftc -O ax_window_spike.swift -o /tmp/ax_window_spike && /tmp/ax_window_spike
// 每应用输出一行（<110 字符）。

struct Target {
    let name: String
    let bundleIds: [String]
    let names: [String]
}

let targets = [
    Target(name: "DingTalk", bundleIds: ["com.alibaba.DingTalkMac"], names: ["dingtalk", "钉钉"]),
    Target(name: "WeChat", bundleIds: ["com.tencent.xinWeChat"], names: ["wechat", "weixin", "微信"]),
    Target(name: "ChatGPT", bundleIds: ["com.openai.chat"], names: ["chatgpt"]),
]

func rawValue(_ el: AXUIElement, _ a: String) -> CFTypeRef? {
    var v: CFTypeRef?
    guard AXUIElementCopyAttributeValue(el, a as CFString, &v) == .success else { return nil }
    return v
}

func attrString(_ el: AXUIElement, _ a: String) -> String? {
    (rawValue(el, a) as? String)
}

func attrBool(_ el: AXUIElement, _ a: String) -> Bool? {
    (rawValue(el, a) as? NSNumber).map { $0.boolValue }
}

func attrElements(_ el: AXUIElement, _ a: String) -> [AXUIElement] {
    guard let cf = rawValue(el, a), CFGetTypeID(cf) == CFArrayGetTypeID() else { return [] }
    let arr = unsafeDowncast(cf, to: CFArray.self) as NSArray
    return arr.map { unsafeDowncast($0 as AnyObject, to: AXUIElement.self) }
}

func attrMain(_ el: AXUIElement) -> AXUIElement? {
    guard let cf = rawValue(el, "AXMainWindow"),
          CFGetTypeID(cf) == AXUIElementGetTypeID() else { return nil }
    return unsafeDowncast(cf, to: AXUIElement.self)
}

func attrPoint(_ el: AXUIElement, _ a: String) -> CGPoint? {
    guard let cf = rawValue(el, a), CFGetTypeID(cf) == AXValueGetTypeID() else { return nil }
    var p = CGPoint.zero
    guard AXValueGetValue(unsafeDowncast(cf, to: AXValue.self), .cgPoint, &p) else { return nil }
    return p
}

func attrSize(_ el: AXUIElement, _ a: String) -> CGSize? {
    guard let cf = rawValue(el, a), CFGetTypeID(cf) == AXValueGetTypeID() else { return nil }
    var s = CGSize.zero
    guard AXValueGetValue(unsafeDowncast(cf, to: AXValue.self), .cgSize, &s) else { return nil }
    return s
}

func isSettable(_ el: AXUIElement, _ a: String) -> Bool {
    var b: DarwinBoolean = false
    _ = AXUIElementIsAttributeSettable(el, a as CFString, &b)
    return b.boolValue
}

func findApp(_ t: Target) -> NSRunningApplication? {
    for bid in t.bundleIds {
        if let a = NSRunningApplication.runningApplications(withBundleIdentifier: bid).first {
            return a
        }
    }
    return NSWorkspace.shared.runningApplications.first { a in
        guard !a.isTerminated else { return false }
        let n = (a.localizedName ?? "").lowercased()
        let bid = (a.bundleIdentifier ?? "").lowercased()
        return t.names.contains { n.contains($0) } || t.bundleIds.contains { bid == $0 }
    }
}

print("axspike trusted=\(AXIsProcessTrusted() ? 1 : 0) host=\(ProcessInfo.processInfo.processName)")

for t in targets {
    guard let app = findApp(t) else {
        print("\(t.name) running=0 set=not_running")
        continue
    }
    let appEl = AXUIElementCreateApplication(app.processIdentifier)
    let wins = attrElements(appEl, "AXWindows")
    var mainWin: AXUIElement? = attrMain(appEl)
    if mainWin == nil {
        mainWin = wins.first { (attrString($0, "AXSubrole") ?? "") == "AXStandardWindow" }
    }
    if mainWin == nil {
        mainWin = wins.first
    }
    guard let w = mainWin else {
        print("\(t.name) running=1 pid=\(app.processIdentifier) wins=\(wins.count) main=none set=n/a")
        continue
    }

    let sub = attrString(w, "AXSubrole") ?? "?"
    let title = (attrString(w, "AXTitle") ?? "").prefix(6)
    let pos = attrPoint(w, "AXPosition").map { "(\(Int($0.x)),\(Int($0.y)))" } ?? "-"
    let sz = attrSize(w, "AXSize").map { "(\(Int($0.width))x\(Int($0.height)))" } ?? "-"
    let pSet = isSettable(w, "AXPosition") ? 1 : 0
    let sSet = isSettable(w, "AXSize") ? 1 : 0
    let full = attrBool(w, "AXFullScreen").map { $0 ? 1 : 0 } ?? -1

    let hasSheet = attrElements(w, "AXChildren").contains {
        (attrString($0, "AXSubrole") ?? "") == "AXSheet"
    } ? 1 : 0

    // no-op 位置写测试（不挪动窗口）+ 读回验证
    var setTest = "not_settable"
    var rb = "-"
    if pSet == 1, let p0 = attrPoint(w, "AXPosition") {
        var p = p0
        if let val = AXValueCreate(.cgPoint, &p) {
        switch AXUIElementSetAttributeValue(w, "AXPosition" as CFString, val) {
        case .success:
            if let np = attrPoint(w, "AXPosition") {
                rb = (abs(np.x - p.x) < 0.5 && abs(np.y - p.y) < 0.5) ? "match" : "mismatch"
                setTest = "ok_samepos"
            } else {
                setTest = "ok_rb_fail"
            }
        case let e:
            setTest = "err\(e.rawValue)"
        }
        }
    }

    print("\(t.name) running=1 pid=\(app.processIdentifier) wins=\(wins.count) main=\(sub) "
        + "pos=\(pos) size=\(sz) pSet=\(pSet) sSet=\(sSet) full=\(full) sheet=\(hasSheet) "
        + "set=\(setTest) rb=\(rb) t=\(title)")
}
