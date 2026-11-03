// Render a page in WebKit at a phone width; print `scrollWidth viewportWidth` of the body
// with every <details> open, and optionally save a full-height PNG (with `js`: run it
// after the tab/mode switch and save one 844 px tall screen, e.g. an open sheet).
// Numbers the js leaves in `window.measure` are printed after the two widths (with a png, `window.late` instead if set).
// #200, with `close`: the page under a sheet as it closes. A baseline before `js` opens the sheet, then `close` and
// snapshots back to back for 400 ms; prints the frame count, the worst frame's share of pixels above the open sheet
// that are not the baseline dimmed evenly, no darker than under the open sheet (the backdrop fading), and how dimmed
// the last frame is (0: the page as it was); the worst frame goes to out.png.
// #206: `HERDSMAN_TAB` is the stored tab (a private localStorage) before the page loads; tab `-`: no tab()/mode() and no
// <details> opened, the page as it loaded (with its scripts cut, as it first paints).
// swift webview.swift <page.html> <width> <tab> <mode> [out.png] [js] [close]
import AppKit
import WebKit

let a = CommandLine.arguments
let html = try! String(contentsOfFile: a[1], encoding: .utf8)
let width = CGFloat(Double(a[2])!)
let (tab, mode, png, extra) = (a[3], a[4], a.count > 5 ? a[5] : nil, a.count > 6 ? a[6] : "")
let close = a.count > 7 ? a[7] : nil
let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
let conf = WKWebViewConfiguration()
if let t = ProcessInfo.processInfo.environment["HERDSMAN_TAB"] {
    conf.websiteDataStore = .nonPersistent() // its own localStorage: other runs' tab() writes never reach it
    conf.userContentController.addUserScript(WKUserScript(source: "localStorage.tab = '\(t)'", injectionTime: .atDocumentStart, forMainFrameOnly: true))
}
let web = WKWebView(frame: NSRect(x: 0, y: 0, width: width, height: 844), configuration: conf)
let win = NSWindow(contentRect: web.frame, styleMask: [.borderless], backing: .buffered, defer: false)
win.contentView = web

// A snapshot's RGBA bytes, row by row.
func rgba(_ img: NSImage) -> [UInt8] {
    let cg = img.cgImage(forProposedRect: nil, context: nil, hints: nil)!
    var buf = [UInt8](repeating: 0, count: cg.width * cg.height * 4)
    let ctx = CGContext(data: &buf, width: cg.width, height: cg.height, bitsPerComponent: 8, bytesPerRow: cg.width * 4,
                        space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.draw(cg, in: CGRect(x: 0, y: 0, width: cg.width, height: cg.height))
    return buf
}
// Over the rows above `top` (CSS px): the frame as the baseline times k (the black backdrop at some opacity), and the
// share of pixels off that by more than 12 on a channel; with `lo`, k under it (darker than the open sheet's backdrop,
// e.g. black) or over 1.02 (lighter than the page, e.g. grey) is all off.
func judge(_ b: [UInt8], _ f: [UInt8], _ top: Double, _ lo: Double = 0) -> (off: Double, k: Double) {
    let px = Int((Double(f.count / 4) / Double(width) / 844).squareRoot()), w = Int(width) * px
    // Every third pixel of every third row: the snapshots are ~1.3 M pixels and this runs unoptimised.
    let at = stride(from: 0, to: min(Int(max(0, top)) * px, f.count / 4 / w), by: 3).flatMap { y in stride(from: 0, to: w, by: 3).map { (y * w + $0) * 4 } }
    var fb = 0.0, bb = 0.0
    for i in at { for c in 0..<3 { fb += Double(f[i + c]) * Double(b[i + c]); bb += Double(b[i + c]) * Double(b[i + c]) } }
    guard bb > 0 else { return (0, 1) }
    let k = fb / bb
    let off = at.filter { i in (0..<3).contains { abs(Double(f[i + $0]) - k * Double(b[i + $0])) > 12 } }.count
    return (k < lo - 0.05 || k > 1.02 ? 1 : Double(off) / Double(at.count), k)
}
// Snapshots back to back for 400 ms.
func frames(_ w: WKWebView, _ t0: Date, _ got: [NSImage], _ done: @escaping ([NSImage]) -> Void) {
    guard Date().timeIntervalSince(t0) < 0.4 else { return done(got) }
    w.takeSnapshot(with: nil) { img, _ in frames(w, t0, got + [img!], done) }
}
func gate(_ w: WKWebView, _ close: String) {
    win.orderFrontRegardless() // off screen, WebKit runs no rendering updates and snapshots go stale
    w.evaluateJavaScript("tab('\(tab)'); mode('\(mode)'); 1") { _, _ in
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
            w.takeSnapshot(with: nil) { b, _ in
                w.evaluateJavaScript("\(extra); 1") { _, e in
                    if let e { print("js error: \(e)"); exit(2) }
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.7) {
                        // Above the open sheet: it only slides down from there. (Its live top is no use: the
                        // composited slide runs behind what JS reads.)
                        w.evaluateJavaScript("document.querySelector('.sh').getBoundingClientRect().top") { t, _ in
                            let top = (t as? Double) ?? 0
                            w.takeSnapshot(with: nil) { o, _ in
                                let base = rgba(b!), lo = judge(base, rgba(o!), top).k
                                w.evaluateJavaScript("\(close); 1") { _, _ in }
                                frames(w, Date(), []) { got in
                                    let r = got.map { judge(base, rgba($0), top, lo) }
                                    let worst = r.indices.max { r[$0].off < r[$1].off }!
                                    if let png {
                                        try! NSBitmapImageRep(data: got[worst].tiffRepresentation!)!.representation(using: .png, properties: [:])!
                                            .write(to: URL(fileURLWithPath: png))
                                    }
                                    print(r.count, r[worst].off, 1 - r.last!.k)
                                    exit(0)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

final class Nav: NSObject, WKNavigationDelegate {
    func webView(_ w: WKWebView, didFinish _: WKNavigation!) {
        if let close { return gate(w, close) }
        let js = tab == "-" ? "\(extra); [document.body.scrollWidth, document.documentElement.clientWidth, document.documentElement.scrollHeight, ...(window.measure || [])]" : """
        tab('\(tab)'); mode('\(mode)'); \(extra);
        \(png == nil ? "document.querySelectorAll('details').forEach(d => d.open = true);" : "document.querySelectorAll('section').forEach(s => { const d = s.querySelector('details'); if (d) d.open = true; });")
        [document.body.scrollWidth, document.documentElement.clientWidth, document.documentElement.scrollHeight, ...(window.measure || [])]
        """
        w.evaluateJavaScript(js) { r, e in
            guard let v = r as? [Double] else { print("js error: \(String(describing: e))"); exit(2) }
            let wide = [Int(v[0]), Int(v[1])].map { "\($0)" }
            guard let png else { print((wide + v.dropFirst(3).map { "\($0)" }).joined(separator: " ")); exit(0) }
            w.setFrameSize(NSSize(width: width, height: extra.isEmpty ? CGFloat(v[2]) : 844))
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                // #315: numbers the js leaves in `window.late` are read after the wait (a sheet's poll has landed) instead.
                w.evaluateJavaScript("window.late || null") { m, _ in
                    print((wide + ((m as? [Double]) ?? Array(v.dropFirst(3))).map { "\($0)" }).joined(separator: " "))
                    w.takeSnapshot(with: nil) { img, _ in
                        let rep = NSBitmapImageRep(data: img!.tiffRepresentation!)!
                        try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: png))
                        exit(0)
                    }
                }
            }
        }
    }
}
let nav = Nav()
web.navigationDelegate = nav
web.loadHTMLString(html, baseURL: ProcessInfo.processInfo.environment["HERDSMAN_BASE"].flatMap(URL.init(string:)))
DispatchQueue.main.asyncAfter(deadline: .now() + 30) { print("timeout"); exit(3) }
app.run()
