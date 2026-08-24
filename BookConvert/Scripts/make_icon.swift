import AppKit

let canvasSize: CGFloat = 1024
let image = NSImage(size: NSSize(width: canvasSize, height: canvasSize))

image.lockFocus()
guard let ctx = NSGraphicsContext.current?.cgContext else { fatalError("no context") }

// Transparent canvas; the squircle itself carries the visible shape (matches
// modern macOS icons, which are pre-masked rather than relying on OS auto-masking).
ctx.clear(CGRect(x: 0, y: 0, width: canvasSize, height: canvasSize))

// Squircle background: rounded rect inset from the canvas edge, corner radius
// ~22.5% of the edge length, matching Apple's Big Sur+ icon convention.
let inset: CGFloat = 40
let squircleRect = CGRect(x: inset, y: inset, width: canvasSize - inset * 2, height: canvasSize - inset * 2)
let cornerRadius = squircleRect.width * 0.225
let squirclePath = CGPath(roundedRect: squircleRect, cornerWidth: cornerRadius, cornerHeight: cornerRadius, transform: nil)

ctx.saveGState()
ctx.addPath(squirclePath)
ctx.clip()

// Purple gradient fill, lighter top-left to deeper bottom-right for depth.
let colors = [
    NSColor(red: 0.62, green: 0.49, blue: 0.96, alpha: 1.0).cgColor,
    NSColor(red: 0.42, green: 0.27, blue: 0.80, alpha: 1.0).cgColor,
]
let gradient = CGGradient(colorsSpace: CGColorSpaceCreateDeviceRGB(), colors: colors as CFArray, locations: [0, 1])!
ctx.drawLinearGradient(
    gradient,
    start: CGPoint(x: squircleRect.minX, y: squircleRect.maxY),
    end: CGPoint(x: squircleRect.maxX, y: squircleRect.minY),
    options: []
)

// Subtle inner top highlight for a touch of glossiness — a smooth gradient
// rather than a hard-edged rect, so there's no visible seam.
let highlightColors = [
    NSColor.white.withAlphaComponent(0.16).cgColor,
    NSColor.white.withAlphaComponent(0.0).cgColor,
]
let highlightGradient = CGGradient(
    colorsSpace: CGColorSpaceCreateDeviceRGB(), colors: highlightColors as CFArray, locations: [0, 1])!
ctx.drawLinearGradient(
    highlightGradient,
    start: CGPoint(x: squircleRect.midX, y: squircleRect.maxY),
    end: CGPoint(x: squircleRect.midX, y: squircleRect.midY),
    options: []
)

ctx.restoreGState()

// Glyph: the same "arrow.down.doc" motif used in the app's own drop zone, in
// white, centered — ties the Dock icon directly to the in-app visual language.
let glyphConfig = NSImage.SymbolConfiguration(pointSize: 440, weight: .medium)
if let symbol = NSImage(systemSymbolName: "arrow.down.doc.fill", accessibilityDescription: nil)?
    .withSymbolConfiguration(glyphConfig)
{
    let tinted = NSImage(size: symbol.size)
    tinted.lockFocus()
    NSColor.white.set()
    let rect = NSRect(origin: .zero, size: symbol.size)
    symbol.draw(in: rect, from: .zero, operation: .sourceOver, fraction: 1.0)
    rect.fill(using: .sourceAtop)
    tinted.unlockFocus()

    let glyphSize = tinted.size
    let glyphOrigin = CGPoint(x: (canvasSize - glyphSize.width) / 2, y: (canvasSize - glyphSize.height) / 2 - 10)
    tinted.draw(in: CGRect(origin: glyphOrigin, size: glyphSize))
}

image.unlockFocus()

guard let tiff = image.tiffRepresentation, let rep = NSBitmapImageRep(data: tiff),
    let png = rep.representation(using: .png, properties: [:])
else { fatalError("failed to encode PNG") }

let outputPath = CommandLine.arguments[1]
try png.write(to: URL(fileURLWithPath: outputPath))
print("wrote \(outputPath)")
