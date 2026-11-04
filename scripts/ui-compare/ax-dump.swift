// Usage: swift ax-dump.swift <pid> [maxDepth] — recursive AX tree with frames.
import ApplicationServices
import Foundation

let pid = pid_t(CommandLine.arguments.dropFirst().first.flatMap { Int32($0) } ?? 0)
let maxDepth = CommandLine.arguments.count > 2 ? Int(CommandLine.arguments[2])! : 12
guard pid > 0 else { FileHandle.standardError.write("usage: ax-dump <pid> [depth]\n".data(using: .utf8)!); exit(1) }

func attr(_ el: AXUIElement, _ name: String) -> AnyObject? {
    var v: AnyObject?
    return AXUIElementCopyAttributeValue(el, name as CFString, &v) == .success ? v : nil
}
func str(_ el: AXUIElement, _ name: String) -> String {
    if let s = attr(el, name) as? String { return s }
    return ""
}
func frame(_ el: AXUIElement) -> String {
    var pos = CGPoint.zero, size = CGSize.zero
    if let p = attr(el, kAXPositionAttribute), CFGetTypeID(p) == AXValueGetTypeID() {
        AXValueGetValue(p as! AXValue, .cgPoint, &pos)
    }
    if let s = attr(el, kAXSizeAttribute), CFGetTypeID(s) == AXValueGetTypeID() {
        AXValueGetValue(s as! AXValue, .cgSize, &size)
    }
    return "\(Int(pos.x)),\(Int(pos.y)) \(Int(size.width))x\(Int(size.height))"
}

func walk(_ el: AXUIElement, _ depth: Int) {
    if depth > maxDepth { return }
    let role = str(el, kAXRoleAttribute)
    let sub = str(el, kAXSubroleAttribute)
    let title = str(el, kAXTitleAttribute)
    let desc = str(el, kAXDescriptionAttribute)
    let value = str(el, kAXValueAttribute)
    let pad = String(repeating: "  ", count: depth)
    var line = "\(pad)\(role)\(sub.isEmpty ? "" : " (\(sub))") [\(frame(el))]"
    if !title.isEmpty { line += " title=\(title.debugDescription)" }
    if !desc.isEmpty { line += " desc=\(desc.debugDescription)" }
    if !value.isEmpty && value.count < 120 { line += " value=\(value.debugDescription)" }
    print(line)
    if let kids = attr(el, kAXChildrenAttribute) as? [AXUIElement] {
        for k in kids { walk(k, depth + 1) }
    }
}
walk(AXUIElementCreateApplication(pid), 0)
