// Signed migration experiment. Run only in an isolated macOS test account.
// Never prints, accepts on argv, or writes a real TV pairing token.
import Foundation
import LocalAuthentication
import Security

let legacyService = "dev.samsungtvremote.local.pairing"
let usage = "usage: m6-keychain-probe read|replace-and-delete|validate-input <device-id>"
let arguments = CommandLine.arguments
guard arguments.count == 3,
      ["read", "replace-and-delete", "validate-input"].contains(arguments[1]) else {
    fputs("\(usage)\n", stderr)
    exit(2)
}

// Match DeviceId's persisted record key, which is also the exact Keychain
// account. It is a decimal u128 with a dev_ prefix, not a formatted UUID.
let account = arguments[2]
let digits = account.dropFirst(4)
let maximum = "340282366920938463463374607431768211455"
guard account.hasPrefix("dev_"), !digits.isEmpty,
      digits.utf8.allSatisfy({ $0 >= 48 && $0 <= 57 }),
      (digits.count == 1 || digits.first != "0"),
      digits.count < maximum.count ||
        (digits.count == maximum.count && String(digits) <= maximum) else {
    fputs("invalid device ID: copy the exact dev_<decimal> ID from devices.json; do not convert it to a UUID\n", stderr)
    exit(2)
}

// The old GUI's service is fixed. A wrong device ID must never broaden a
// Keychain query to other items. Preserve the account string exactly.
let environment = ProcessInfo.processInfo.environment
guard environment["M6_ISOLATED_KEYCHAIN_ACCOUNT"] == NSUserName() else {
    fputs("M6_ISOLATED_KEYCHAIN_ACCOUNT must match the logged-in isolated test account\n", stderr)
    exit(3)
}
guard environment["M6_SYNTHETIC_DEVICE_ID"] == account else {
    if environment["M6_SYNTHETIC_DEVICE_ID"] == nil,
       environment["M6_SYNTHETIC_DEVICE_UUID"] != nil {
        fputs("M6_SYNTHETIC_DEVICE_UUID is obsolete; export M6_SYNTHETIC_DEVICE_ID with the exact saved dev_<decimal> ID\n", stderr)
        exit(3)
    }
    fputs("M6_SYNTHETIC_DEVICE_ID must exactly match the disposable dev_<decimal> argument\n", stderr)
    exit(3)
}
// This preflight deliberately stops before any Keychain API or LAContext.
if arguments[1] == "validate-input" {
    print("input: valid (Keychain not accessed)")
    exit(0)
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
