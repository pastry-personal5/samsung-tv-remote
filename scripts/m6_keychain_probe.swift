// Signed migration experiment. Run only in an isolated macOS test account.
// Never prints, accepts on argv, or writes a real TV pairing token.
import Foundation
import LocalAuthentication
import Security

let legacyService = "dev.samsungtvremote.local.pairing"
let usage = "usage: m6-keychain-probe read|replace-and-delete <device-uuid>"
let arguments = CommandLine.arguments
guard arguments.count == 3,
      ["read", "replace-and-delete"].contains(arguments[1]),
      UUID(uuidString: arguments[2]) != nil else {
    fputs("\(usage)\n", stderr)
    exit(2)
}

// The old GUI's service is fixed. A wrong account UUID must never broaden a
// Keychain query to other items.
let account = arguments[2].lowercased()
guard ProcessInfo.processInfo.environment["M6_ISOLATED_KEYCHAIN_ACCOUNT"] == NSUserName(),
      ProcessInfo.processInfo.environment["M6_SYNTHETIC_DEVICE_UUID"]?.lowercased() == account else {
    fputs("requires matching isolated test-account name and synthetic UUID\n", stderr)
    exit(3)
}
let context = LAContext()
context.interactionNotAllowed = true
let query: [String: Any] = [
    kSecClass as String: kSecClassGenericPassword,
    kSecAttrService as String: legacyService,
    kSecAttrAccount as String: account,
    kSecUseAuthenticationContext as String: context,
]

func read() -> OSStatus {
    var result: CFTypeRef?
    var request = query
    request[kSecReturnData as String] = true
    request[kSecMatchLimit as String] = kSecMatchLimitOne
    let status = SecItemCopyMatching(request as CFDictionary, &result)
    if status == errSecSuccess {
        guard let data = result as? Data else {
            fputs("read returned a non-data result\n", stderr)
            return errSecDecode
        }
        // The value is deliberately not logged or returned from this program.
        print("read: status=0 bytes=\(data.count)")
    } else {
        print("read: status=\(status)")
    }
    return status
}

let initial = read()
guard arguments[1] == "replace-and-delete" else {
    exit(initial == errSecSuccess ? 0 : 1)
}

// Destructive operations are restricted to a deliberate isolated-account run.
// The old GUI must first have created this *synthetic* item through a fake TV.
guard initial == errSecSuccess else {
    fputs("mutation requires a readable old item\n", stderr)
    exit(3)
}

let replacement = Data("m6-synthetic-replacement".utf8)
let updateStatus = SecItemUpdate(
    query as CFDictionary,
    [kSecValueData as String: replacement] as CFDictionary
)
print("replace: status=\(updateStatus)")
guard updateStatus == errSecSuccess else { exit(1) }

var verification: CFTypeRef?
var request = query
request[kSecReturnData as String] = true
request[kSecMatchLimit as String] = kSecMatchLimitOne
let verifyStatus = SecItemCopyMatching(request as CFDictionary, &verification)
let matched = verifyStatus == errSecSuccess && (verification as? Data) == replacement
print("verify: status=\(verifyStatus) matches-synthetic=\(matched)")
guard matched else { exit(1) }

let deleteStatus = SecItemDelete(query as CFDictionary)
print("delete: status=\(deleteStatus)")
guard deleteStatus == errSecSuccess else { exit(1) }
let finalStatus = read()
guard finalStatus == errSecItemNotFound else { exit(1) }
