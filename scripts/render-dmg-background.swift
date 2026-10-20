import AppKit
import Foundation

// Run from the repository root: swift scripts/render-dmg-background.swift
// The DMG is visible before CASE can start, so render its English i18n copy
// into the installer background instead of relying on the blocked app.
let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
let source = root.appendingPathComponent("src/i18n/locales/en/translation.json")
let translations = try JSONSerialization.jsonObject(with: Data(contentsOf: source)) as! [String: Any]
let installer = translations["installer"] as! [String: Any]
let copy = installer["macos"] as! [String: String]
let size = NSSize(width: 760, height: 560)
let ink = NSColor(calibratedWhite: 0.10, alpha: 1)
let muted = NSColor(calibratedWhite: 0.35, alpha: 1)

func text(_ key: String, x: CGFloat, y: CGFloat, width: CGFloat,
          font: NSFont, color: NSColor = ink) {
    let paragraph = NSMutableParagraphStyle()
    paragraph.lineSpacing = 3
    let value = copy[key]!
    let attributes: [NSAttributedString.Key: Any] = [
        .font: font, .foregroundColor: color, .paragraphStyle: paragraph,
    ]
    let rect = NSRect(x: x, y: y, width: width, height: 50)
    let measured = (value as NSString).boundingRect(
        with: rect.size, options: [.usesLineFragmentOrigin, .usesFontLeading],
        attributes: attributes
    )
    precondition(measured.height <= rect.height, "Installer text overflows: \(key)")
    (value as NSString).draw(in: rect, withAttributes: attributes)
}

let image = NSImage(size: size, flipped: true) { _ in
    NSColor(calibratedWhite: 0.97, alpha: 1).setFill()
    NSRect(origin: .zero, size: size).fill()
    text("title", x: 40, y: 24, width: 680,
         font: NSFont(name: "Georgia", size: 30)!)
    text("install", x: 40, y: 68, width: 680,
         font: .systemFont(ofSize: 15), color: muted)

    let arrow = NSBezierPath()
    arrow.move(to: NSPoint(x: 337, y: 155))
    arrow.line(to: NSPoint(x: 423, y: 155))
    arrow.move(to: NSPoint(x: 412, y: 144))
    arrow.line(to: NSPoint(x: 423, y: 155))
    arrow.line(to: NSPoint(x: 412, y: 166))
    arrow.lineWidth = 2
    NSColor(calibratedWhite: 0.55, alpha: 1).setStroke()
    arrow.stroke()

    let divider = NSBezierPath()
    divider.move(to: NSPoint(x: 40, y: 258))
    divider.line(to: NSPoint(x: 720, y: 258))
    NSColor(calibratedWhite: 0.84, alpha: 1).setStroke()
    divider.stroke()
    text("warning", x: 40, y: 282, width: 680,
         font: .systemFont(ofSize: 18, weight: .semibold))
    text("explanation", x: 40, y: 316, width: 680,
         font: .systemFont(ofSize: 14), color: muted)
    for (index, key) in ["done", "settings", "openAnyway", "confirm"].enumerated() {
        let y = CGFloat(360 + index * 42)
        ("\(index + 1)." as NSString).draw(
            at: NSPoint(x: 40, y: y),
            withAttributes: [.font: NSFont.monospacedDigitSystemFont(ofSize: 14, weight: .medium),
                             .foregroundColor: muted]
        )
        text(key, x: 68, y: y, width: 652, font: .systemFont(ofSize: 15))
    }
    return true
}

let bitmap = NSBitmapImageRep(
    bitmapDataPlanes: nil, pixelsWide: Int(size.width), pixelsHigh: Int(size.height),
    bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
    colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
image.draw(in: NSRect(origin: .zero, size: size))
NSGraphicsContext.restoreGraphicsState()
let output = root.appendingPathComponent("src-tauri/dmg/background.png")
try bitmap.representation(using: .png, properties: [:])!.write(to: output)
print("Rendered \(output.path)")
