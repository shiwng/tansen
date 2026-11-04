// Usage: swift input.swift move <x> <y> | click <x> <y> | scroll <dy> | key <keycode> [cmd] | type <text>
import CoreGraphics
import Foundation

func post(_ e: CGEvent?) { e?.post(tap: .cghidEventTap); usleep(60_000) }
let a = CommandLine.arguments
guard a.count > 1 else { exit(1) }
let p = CGPoint(x: Double(a.count > 2 ? a[2] : "0") ?? 0, y: Double(a.count > 3 ? a[3] : "0") ?? 0)
switch a[1] {
case "move":
    post(CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: p, mouseButton: .left))
case "click":
    post(CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: p, mouseButton: .left))
    post(CGEvent(mouseEventSource: nil, mouseType: .leftMouseDown, mouseCursorPosition: p, mouseButton: .left))
    post(CGEvent(mouseEventSource: nil, mouseType: .leftMouseUp, mouseCursorPosition: p, mouseButton: .left))
case "scroll":
    post(CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1, wheel1: Int32(a[2]) ?? 0, wheel2: 0, wheel3: 0))
case "type":
    for u in a[2].utf16 {
        var c = u
        let down = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: true)
        down?.keyboardSetUnicodeString(stringLength: 1, unicodeString: &c)
        let up = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: false)
        post(down); post(up)
    }
case "key":
    let code = CGKeyCode(UInt16(a[2]) ?? 0)
    let down = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: true)
    let up = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: false)
    if a.count > 3 && a[3] == "cmd" { down?.flags = .maskCommand; up?.flags = .maskCommand }
    post(down); post(up)
default: exit(1)
}
// drag x1 y1 x2 y2 [steps] — press at (x1,y1), move in steps, release at (x2,y2)
if a[1] == "drag" {
    let q = CGPoint(x: Double(a[4]) ?? 0, y: Double(a[5]) ?? 0)
    let steps = Int(a[6] ?? "12") ?? 12
    post(CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: p, mouseButton: .left))
    post(CGEvent(mouseEventSource: nil, mouseType: .leftMouseDown, mouseCursorPosition: p, mouseButton: .left))
    for i in 1...steps {
        let t = CGFloat(i) / CGFloat(steps)
        let at = CGPoint(x: p.x + (q.x - p.x) * t, y: p.y + (q.y - p.y) * t)
        post(CGEvent(mouseEventSource: nil, mouseType: .leftMouseDragged, mouseCursorPosition: at, mouseButton: .left))
    }
    post(CGEvent(mouseEventSource: nil, mouseType: .leftMouseUp, mouseCursorPosition: q, mouseButton: .left))
    exit(0)
}
