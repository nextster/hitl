// A made-up phone that is not at home, in the phone's own code: `RelayLink` and `RelaySeal` of the app,
// built for this Mac. `tests/phone-tests.py` starts it against its made-up server and the real process of
// `hitl phone`, and so sees that what the app seals the Mac opens, and the other way.
//
//     far-phone URL ROOM WORD KEY
//
// Every line it is given is a word to say to the Mac, sealed. Every line it prints is what it heard:
// {"mac": "here"} from the server, or a word of the Mac as it came, the bytes of a file told by their count.
import CryptoKit
import Foundation

@main
struct FarPhone {
    @MainActor
    static func main() async {
        let arguments = CommandLine.arguments
        guard arguments.count == 5,
              let link = RelayLink(word: RelayWord(url: arguments[1], room: arguments[2], token: arguments[3], key: arguments[4])) else {
            FileHandle.standardError.write(Data("far-phone URL ROOM WORD KEY\n".utf8))
            exit(64)
        }
        func printed(_ word: [String: Any]) {
            guard let line = try? JSONSerialization.data(withJSONObject: word, options: [.sortedKeys]) else { return }
            FileHandle.standardOutput.write(line + Data("\n".utf8))
        }
        link.macChanged = { state in
            printed(["mac": state == .here ? "here" : state == .resting ? "resting" : "gone"])
        }
        link.line = { data in
            guard var word = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return }
            if let text = word["data"] as? String, let bytes = Data(base64Encoded: text) {
                word["data"] = nil
                word["bytes"] = bytes.count
                word["print"] = SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
            }
            printed(word)
        }
        link.ended = { why in
            printed(["ended": why])
            exit(0)
        }
        link.start()
        do {
            for try await line in FileHandle.standardInput.bytes.lines {
                if let word = try? JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any] { link.say(word) }
            }
        } catch {}
        link.close()
    }
}
