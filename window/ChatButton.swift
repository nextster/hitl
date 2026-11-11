// The «Чат» button knows no program. What it runs for an address is in the setting the tool's setup wrote, by the
// address's scheme (`rust/hitl/src/chatbutton.rs` says the file and the same rule on the other side); an address
// whose scheme the setting does not list opens as any address does.
import Foundation

enum ChatButton {
    /// The file: named by `CONVEYOR_CHAT_BUTTON`, else `chat-button.json` in the tool's state folder.
    static func file(_ environment: [String: String]) -> String {
        if let named = environment["CONVEYOR_CHAT_BUTTON"], !named.isEmpty { return named }
        let home = environment["CONVEYOR_HOME"].flatMap { $0.isEmpty ? nil : $0 } ?? NSHomeDirectory() + "/.conveyor"
        return home + "/chat-button.json"
    }

    /// The scheme of an address, lowercase, when it has one.
    static func scheme(_ link: String) -> String? {
        let text = link.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let colon = text.firstIndex(of: ":") else { return nil }
        let scheme = text[..<colon]
        let proper = !scheme.isEmpty && scheme.allSatisfy { $0.isASCII && ($0.isLetter || $0.isNumber || "+-.".contains($0)) }
        return proper && text.distance(from: colon, to: text.endIndex) > 1 ? scheme.lowercased() : nil
    }

    /// What to run for the address: the setting's words with `{link}`, `{scheme}`, `{host}` and `{path}` put in; nil
    /// when the setting says nothing for its scheme.
    static func command(for link: String, environment: [String: String]) -> [String]? {
        let text = link.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let scheme = scheme(text),
              let data = FileManager.default.contents(atPath: file(environment)),
              let setting = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let open = setting["open"] as? [String: Any] else { return nil }
        guard let words = open.first(where: { $0.key.lowercased() == scheme })?.value as? [String],
              !words.isEmpty, !words.contains("") else { return nil }
        var rest = Substring(text.dropFirst(scheme.count + 1))
        if rest.hasPrefix("//") { rest = rest.dropFirst(2) }
        let halves = rest.split(separator: "/", maxSplits: 1, omittingEmptySubsequences: false).map(String.init)
        let host = halves.first ?? "", path = halves.count > 1 ? halves[1] : ""
        return words.map {
            $0.replacingOccurrences(of: "{link}", with: text).replacingOccurrences(of: "{scheme}", with: scheme)
                .replacingOccurrences(of: "{host}", with: host).replacingOccurrences(of: "{path}", with: path)
        }
    }
}
