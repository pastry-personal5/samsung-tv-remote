// Registration tool for the signed LaunchAgent comparison bundle.
// Run only in an isolated macOS test account; register affects later logins.
import Foundation
import ServiceManagement

guard CommandLine.arguments.count == 2,
      ["status", "register", "unregister"].contains(CommandLine.arguments[1]) else {
    fputs("usage: m6-agent-controller status|register|unregister\n", stderr)
    exit(2)
}

let service = SMAppService.agent(plistName: "dev.samsungtvremote.local.m6agent.plist")
do {
    switch CommandLine.arguments[1] {
    case "status":
        let plistURL = Bundle.main.bundleURL.appendingPathComponent(
            "Contents/Library/LaunchAgents/dev.samsungtvremote.local.m6agent.plist"
        )
        print("bundle: \(Bundle.main.bundleIdentifier ?? "unknown")")
        print("agent plist present: \(FileManager.default.fileExists(atPath: plistURL.path))")
        let label: String
        switch service.status {
        case .notRegistered: label = "notRegistered"
        case .enabled: label = "enabled"
        case .requiresApproval: label = "requiresApproval"
        case .notFound: label = "notFound"
        @unknown default: label = "unknown(\(service.status.rawValue))"
        }
        print("agent status: \(label)")
    case "register":
        try service.register()
        print("agent registered; inspect status and starts.log in the test account")
    case "unregister":
        try service.unregister()
        print("agent unregistered; inspect status and starts.log in the test account")
    default:
        fatalError("argument checked above")
    }
} catch {
    fputs("LaunchAgent operation failed: \(error)\n", stderr)
    exit(1)
}
