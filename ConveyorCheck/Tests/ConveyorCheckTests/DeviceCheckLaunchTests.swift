import Foundation
import Testing
@testable import ConveyorCheck

/// What the host passes to the launch it makes, read from the arguments.
@Suite struct DeviceCheckLaunchTests {
    private static let argument = Data(#"{"id":"t-1","tag":"r/38","title":"Dock","items":[{"label":"1","text":"Swipe"}]}"#.utf8)
        .base64EncodedString()

    /// The host looks for these names in a built app and passes them to it.
    @Test func theNamesAreTheOnesTheHostUses() {
        #expect(DeviceCheckLaunch.checklistArgument == "--ios-run-checklist")
        #expect(DeviceCheckLaunch.tagArgument == "--ios-run-tag")
        #expect(DeviceCheckReport.relativePath == "ios-run/check.json")
    }

    @Test func theTagIsTheValueTheHostPasses() {
        #expect(DeviceCheckLaunch(arguments: ["App", "--ios-run-tag", "nextster/demo"]).tag == "nextster/demo")
        #expect(DeviceCheckLaunch(arguments: ["App"]).tag == nil)
        #expect(DeviceCheckLaunch(arguments: ["App", "--ios-run-tag"]).tag == nil)
        #expect(DeviceCheckLaunch(arguments: ["App", "--ios-run-tag", ""]).tag == nil)
    }

    @Test func theChecklistComesAmongTheAppsOwnArguments() {
        let launch = DeviceCheckLaunch(arguments: ["App", "--ios-run-tag", "r/38 · nextster/dock", "--ios-run-checklist", Self.argument,
                                                   "--app-flag"])
        #expect(launch.tag == "r/38 · nextster/dock")
        #expect(launch.check?.id == "t-1")
        #expect(launch.check?.items.map(\.label) == ["1"])
        #expect(DeviceCheckLaunch(arguments: ["App", "--ios-run-checklist", "not a checklist"]).check == nil)
        #expect(DeviceCheckLaunch(arguments: ["App", "--app-flag"]) == DeviceCheckLaunch())
    }
}
