// Render the SVG through WebKit's transparent canvas, preserving edge alpha.
// Run from the repository root: swift scripts/render-icons.swift
import AppKit
import WebKit

let app = NSApplication.shared
let svg = try String(contentsOfFile: "assets/icon.svg", encoding: .utf8)
let encoded = Data(svg.utf8).base64EncodedString()
class Renderer: NSObject, WKScriptMessageHandler {
    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        guard let images = message.body as? [String: String] else { exit(1) }
        do {
            for (size, base64) in images {
                guard let data = Data(base64Encoded: base64) else { exit(1) }
                try data.write(to: URL(fileURLWithPath: "assets/icon-\(size).png"))
            }
            exit(0)
        } catch { fputs("\(error)\n", stderr); exit(1) }
    }
}
let renderer = Renderer()
let config = WKWebViewConfiguration()
config.websiteDataStore = .nonPersistent()
config.userContentController.add(renderer, name: "icons")
let web = WKWebView(frame: NSRect(x: 0, y: 0, width: 1024, height: 1024), configuration: config)
web.loadHTMLString("""
<script>
const img = new Image();
img.onload = () => {
  const result = {};
  for (const size of [256, 512]) {
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = size;
    canvas.getContext('2d').drawImage(img, 0, 0, size, size);
    result[size] = canvas.toDataURL('image/png').split(',')[1];
  }
  window.webkit.messageHandlers.icons.postMessage(result);
};
img.src = 'data:image/svg+xml;base64,\(encoded)';
</script>
""", baseURL: nil)
DispatchQueue.main.asyncAfter(deadline: .now() + 30) { exit(1) }
app.run()
