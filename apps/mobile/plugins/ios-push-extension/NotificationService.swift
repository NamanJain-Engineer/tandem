// Vibezeo Remote's Notification Service Extension (F-RM-06, D-2022).
// APNs delivers a generic alert ("Vibezeo · Something on your computer needs
// you.") with the sealed payload `p`. This opens it with the alert key the app
// keeps in the shared keychain group and shows the real text. The format is
// tandem_core::remote::push (ChaCha20-Poly1305, RFC 8439):
//   0x03 ‖ 0x01 ‖ seq u64 BE ‖ nonce (12) ‖ ciphertext ‖ tag (16)
//   AAD = "tandem-push-v1" ‖ pid ‖ seq u64 BE
// If anything fails, the generic alert is shown as it came.
import CryptoKit
import Foundation
import UserNotifications

final class NotificationService: UNNotificationServiceExtension {
  private var handler: ((UNNotificationContent) -> Void)?
  private var content: UNMutableNotificationContent?

  override func didReceive(
    _ request: UNNotificationRequest,
    withContentHandler contentHandler: @escaping (UNNotificationContent) -> Void
  ) {
    handler = contentHandler
    let mutable = (request.content.mutableCopy() as? UNMutableNotificationContent) ?? UNMutableNotificationContent()
    content = mutable
    if let payload = request.content.userInfo["p"] as? String,
       let secret = PushSecret.load(),
       let alert = PushOpener.open(payload: payload, secret: secret) {
      mutable.title = alert.title
      mutable.body = alert.body
      mutable.threadIdentifier = alert.kind
    }
    contentHandler(mutable)
  }

  override func serviceExtensionTimeWillExpire() {
    if let handler, let content { handler(content) }
  }
}

/// The app's `vibezeo.push` keychain item (expo-secure-store, shared group).
struct PushSecret: Decodable {
  let pid: String
  let key: String

  static func load() -> PushSecret? {
    guard let group = Bundle.main.object(forInfoDictionaryKey: "VZKeychainGroup") as? String else { return nil }
    let account = Data("vibezeo.push".utf8)
    let query: [String: Any] = [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrAccount as String: account,
      kSecAttrAccessGroup as String: group,
      kSecReturnData as String: true,
      kSecMatchLimit as String: kSecMatchLimitOne,
    ]
    var out: CFTypeRef?
    guard SecItemCopyMatching(query as CFDictionary, &out) == errSecSuccess, let data = out as? Data else { return nil }
    return try? JSONDecoder().decode(PushSecret.self, from: data)
  }
}

struct PushAlert: Decodable {
  let kind: String
  let title: String
  let body: String
}

enum PushOpener {
  static func base64url(_ s: String) -> Data? {
    var b = s.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
    while b.count % 4 != 0 { b += "=" }
    return Data(base64Encoded: b)
  }

  static func open(payload: String, secret: PushSecret) -> PushAlert? {
    guard let frame = base64url(payload), let keyData = base64url(secret.key), keyData.count == 32,
          frame.count >= 22 + 16 else { return nil }
    let bytes = [UInt8](frame)
    guard bytes[0] == 0x03, bytes[1] == 0x01 else { return nil }
    let seq = Data(bytes[2..<10])
    var aad = Data("tandem-push-v1".utf8)
    aad.append(Data(secret.pid.utf8))
    aad.append(seq)
    do {
      let nonce = try ChaChaPoly.Nonce(data: Data(bytes[10..<22]))
      let box = try ChaChaPoly.SealedBox(
        nonce: nonce,
        ciphertext: Data(bytes[22..<(bytes.count - 16)]),
        tag: Data(bytes[(bytes.count - 16)...])
      )
      let plain = try ChaChaPoly.open(box, using: SymmetricKey(data: keyData), authenticating: aad)
      return try JSONDecoder().decode(PushAlert.self, from: plain)
    } catch {
      return nil
    }
  }
}
