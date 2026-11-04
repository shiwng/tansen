// Usage: swift ocr-boxes.swift <image.png> — Vision text recognition with boxes (image px, origin top-left).
import AppKit
import Foundation
import Vision

guard CommandLine.arguments.count > 1,
      let img = NSImage(contentsOfFile: CommandLine.arguments[1]),
      let cg = img.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
    FileHandle.standardError.write("usage: ocr-boxes <image.png>\n".data(using: .utf8)!)
    exit(1)
}
let req = VNRecognizeTextRequest()
req.recognitionLevel = .accurate
req.usesLanguageCorrection = false
let handler = VNImageRequestHandler(cgImage: cg, options: [:])
try handler.perform([req])
let H = CGFloat(cg.height), W = CGFloat(cg.width)
for obs in (req.results ?? []) {
    guard let top = obs.topCandidates(1).first else { continue }
    let b = obs.boundingBox
    let x = b.minX * W, y = (1 - b.maxY) * H
    print(String(format: "%.0f %.0f %.0f %.0f | %@", x, y, b.width * W, b.height * H, top.string))
}
