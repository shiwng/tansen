// Usage: swift find-window.swift <owner-name-substring>
// Prints "<windowID> <x> <y> <w> <h> <owner> <title>" for on-screen windows.
import CoreGraphics
import Foundation

let needle = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "Music"
let opts = CGWindowListOption(arrayLiteral: .optionOnScreenOnly, .excludeDesktopElements)
guard let list = CGWindowListCopyWindowInfo(opts, kCGNullWindowID) as? [[String: Any]] else {
    FileHandle.standardError.write("window list unavailable\n".data(using: .utf8)!)
    exit(1)
}
for w in list {
    let owner = w[kCGWindowOwnerName as String] as? String ?? ""
    guard owner.localizedCaseInsensitiveContains(needle) else { continue }
    let layer = w[kCGWindowLayer as String] as? Int ?? -1
    guard layer == 0 else { continue }
    let id = w[kCGWindowNumber as String] as? Int ?? 0
    if let b = w[kCGWindowBounds as String] as? [String: Any],
       let x = b["X"] as? Double, let y = b["Y"] as? Double,
       let ww = b["Width"] as? Double, let hh = b["Height"] as? Double {
        let title = w[kCGWindowName as String] as? String ?? ""
        print("\(id) \(Int(x)) \(Int(y)) \(Int(ww)) \(Int(hh)) \(owner) \(title)")
    }
}
