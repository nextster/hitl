import Foundation

/// What the host passed to the launch it made: the checklist of the hand
/// check the build was installed for, and the tag that says whose build it is.
public struct DeviceCheckLaunch: Equatable, Sendable {
    /// Carries the checklist; takes a value. The host finds out that a build
    /// can show one by looking for this name in the app's binaries.
    public static let checklistArgument = "--ios-run-checklist"
    /// Carries the label the host gives every launch it makes: who the build
    /// came from. The host serves other apps too, hence its own prefix. Takes
    /// a value.
    public static let tagArgument = "--ios-run-tag"

    /// The hand check the host installed this build for, on the launch that
    /// brings it; `DeviceCheckStore` keeps it for the launches after.
    public let check: DeviceCheck?
    /// Where the host says this launch came from; nil once the app is
    /// reopened by hand.
    public let tag: String?

    public init(check: DeviceCheck? = nil, tag: String? = nil) {
        self.check = check
        self.tag = tag
    }

    /// Reads the arguments of a launch, as `ProcessInfo.processInfo.arguments`
    /// has them.
    public init(arguments: [String]) {
        check = Self.value(of: Self.checklistArgument, in: arguments).flatMap(DeviceCheck.init(launchArgument:))
        tag = Self.value(of: Self.tagArgument, in: arguments).flatMap { $0.isEmpty ? nil : $0 }
    }

    /// The argument that follows a value-taking one, if any.
    private static func value(of name: String, in arguments: [String]) -> String? {
        guard let index = arguments.firstIndex(of: name) else { return nil }
        let valueIndex = arguments.index(after: index)
        return valueIndex < arguments.endIndex ? arguments[valueIndex] : nil
    }
}
