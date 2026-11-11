// The app's icon: three checkboxes one behind another on a belt with links, the belt going away from the eye.
// His pick of 2026-10-11, through seven lots of drawings: «Очередь едет» in steel first («вот это ничего надо»),
// then checkboxes with no line under them, the one behind dull and empty and the one in front ticked, of which he
// took the stack of three; then «давай ещё сюда конвейерность добавим», the belt with links («вот это охуенно»),
// and of the belts seen from different points the wider one, «Ставь выбранный».
//
//     swift demo/icon.swift demo/ConveyorDemo/Assets.xcassets/AppIcon.appiconset/icon.png
//
// draws it again, 1024 points square and with no transparency, as the system wants an icon.
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

let side = 1024
let unit = CGFloat(side) / 100  // the drawing is thought of on a field of 100 by 100
let space = CGColorSpace(name: CGColorSpace.sRGB)!
let context = CGContext(data: nil, width: side, height: side, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                        bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
// The top left is where the drawing begins, as on a page.
context.translateBy(x: 0, y: CGFloat(side))
context.scaleBy(x: unit, y: -unit)

func color(_ hex: UInt32) -> CGColor {
    CGColor(colorSpace: space, components: [CGFloat((hex >> 16) & 0xFF) / 255, CGFloat((hex >> 8) & 0xFF) / 255, CGFloat(hex & 0xFF) / 255, 1])!
}

func fall(_ from: UInt32, _ to: UInt32) -> CGGradient {
    CGGradient(colorsSpace: space, colors: [color(from), color(to)] as CFArray, locations: [0, 1])!
}

let both: CGGradientDrawingOptions = [.drawsBeforeStartLocation, .drawsAfterEndLocation]
let dim = color(0xC5CAD6), rim = color(0x2B2D34)

// The ground: steel, dark toward the bottom right.
context.drawLinearGradient(fall(0x3A3D46, 0x16171B), start: .zero, end: CGPoint(x: 100, y: 100), options: both)

// The belt: its far edge at the top of the icon and its near edge at the bottom, wider than the icon is.
let far: (CGFloat, CGFloat) = (30, 70), near: (CGFloat, CGFloat) = (-24, 124), horizon: CGFloat = 2
let tall = 100 - horizon

/// Where the belt's two edges are at a height of the icon.
func edges(at y: CGFloat) -> (CGFloat, CGFloat) {
    let along = (y - horizon) / tall
    return (far.0 + (near.0 - far.0) * along, far.1 + (near.1 - far.1) * along)
}

func middle(at y: CGFloat) -> CGFloat {
    let edge = edges(at: y)
    return (edge.0 + edge.1) / 2
}

let cloth = CGMutablePath()
cloth.move(to: CGPoint(x: far.0, y: horizon))
cloth.addLine(to: CGPoint(x: far.1, y: horizon))
cloth.addLine(to: CGPoint(x: near.1, y: 100))
cloth.addLine(to: CGPoint(x: near.0, y: 100))
cloth.closeSubpath()
context.saveGState()
context.setAlpha(0.11)
context.addPath(cloth)
context.setFillColor(dim)
context.fillPath()
context.restoreGState()

// Its links: lines across it, thin far away and wider near.
for along in [0.10, 0.265, 0.47, 0.71, 0.94] as [CGFloat] {
    let y = horizon + tall * along, edge = edges(at: y)
    context.saveGState()
    context.setAlpha(0.3)
    context.setStrokeColor(dim)
    context.setLineWidth(1.6 + (3.2 - 1.6) * along)
    context.move(to: CGPoint(x: edge.0, y: y))
    context.addLine(to: CGPoint(x: edge.1, y: y))
    context.strokePath()
    context.restoreGState()
}

// The checkboxes. The one in front, and how each one behind is smaller, duller and further up the belt.
let size: CGFloat = 58, centre: CGFloat = 60, corner: CGFloat = 0.27
let top = centre - size / 2

func box(_ x: CGFloat, _ y: CGFloat, _ size: CGFloat) -> CGPath {
    CGPath(roundedRect: CGRect(x: x, y: y, width: size, height: size), cornerWidth: size * corner, cornerHeight: size * corner, transform: nil)
}

for behind in [(small: 0.66, peek: 18.0, shows: 0.26), (small: 0.83, peek: 10.0, shows: 0.5)] as [(small: CGFloat, peek: CGFloat, shows: CGFloat)] {
    let own = size * behind.small, y = top - behind.peek
    let outline = box(middle(at: y + own / 2) - own / 2, y, own)
    // A rim of the ground's colour keeps it apart from the one behind it; rim and box are dulled as one.
    context.saveGState()
    context.setAlpha(behind.shows)
    context.beginTransparencyLayer(auxiliaryInfo: nil)
    context.addPath(outline)
    context.setStrokeColor(rim)
    context.setLineWidth(2.4)
    context.strokePath()
    context.addPath(outline)
    context.setFillColor(dim)
    context.fillPath()
    context.endTransparencyLayer()
    context.restoreGState()
}

let left = middle(at: centre) - size / 2
let front = box(left, top, size)
context.addPath(front)
context.setStrokeColor(rim)
context.setLineWidth(3)
context.strokePath()
context.saveGState()
context.addPath(front)
context.clip()
context.drawLinearGradient(fall(0xFFFFFF, 0xAEB4C2), start: CGPoint(x: 25, y: 25), end: CGPoint(x: 90, y: 95), options: both)
context.restoreGState()

// Its tick.
let cx = left + size / 2
context.move(to: CGPoint(x: cx - size * 0.23, y: centre + size * 0.02))
context.addLine(to: CGPoint(x: cx - size * 0.08, y: centre + size * 0.17))
context.addLine(to: CGPoint(x: cx + size * 0.23, y: centre - size * 0.16))
context.setStrokeColor(color(0x1E2026))
context.setLineWidth(size * 0.115)
context.setLineCap(.round)
context.setLineJoin(.round)
context.strokePath()

let out = URL(fileURLWithPath: CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "icon.png")
let file = CGImageDestinationCreateWithURL(out as CFURL, UTType.png.identifier as CFString, 1, nil)!
CGImageDestinationAddImage(file, context.makeImage()!, nil)
guard CGImageDestinationFinalize(file) else { fatalError("the icon was not written to \(out.path)") }
