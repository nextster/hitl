/// The mark a tester gives an item of a checklist. The raw values are what
/// the host reads in the report.
public enum CheckMark: String, Codable, CaseIterable, Sendable {
    case ok
    case problem
    case skipped
}
