// The app's own reading of a link in a text (`Linked` of QueueWords.swift), built for this Mac: every line it is
// given is a text, and for each it prints the text's pieces as JSON, [words, address or null] each.
// `tests/phone-tests.py` holds it against what `hitl check` reads in the same texts.
import Foundation

@main
struct LinkedWords {
    static func main() {
        while let line = readLine() {
            let pieces: [[Any]] = Linked.pieces(line).map { [$0.text, $0.link as Any? ?? NSNull()] }
            let data = (try? JSONSerialization.data(withJSONObject: pieces, options: [.withoutEscapingSlashes])) ?? Data("[]".utf8)
            FileHandle.standardOutput.write(data + Data("\n".utf8))
        }
    }
}
