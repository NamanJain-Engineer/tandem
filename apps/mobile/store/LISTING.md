# Vibezeo Remote: store listing

Every claim here must stay true of the build being submitted (OPERATING §9).
Re-check before each submission.

## Name and short texts
- **App name:** Vibezeo Remote
- **Subtitle (App Store, 30 chars):** Your coding agents, remotely
- **Short description (Play, 80 chars):** Check on the coding agents on your computer, answer approvals and reply to threads.
- **Category:** Developer Tools (App Store) · Tools (Play)
- **Price:** Free. No purchases in the app.

## Full description
Vibezeo Remote is the phone companion to Vibezeo, the desktop workspace for
coding-agent command-line tools. Your agents keep running on your computer;
the phone shows you what they're doing and lets you answer them.

- See each agent's status: working, needs you, done or stopped with an error.
- Answer approvals. Deny always applies. Allow applies at once only while your
  computer allows approvals from the phone; otherwise it waits there for you.
- Read threads and teammate chats, and reply to them.
- Type into an agent's terminal. Never into a plain shell.
- Run a routine now.
- Each of these is off until you turn it on for this phone on your computer.

Private by design: the phone and computer pair with a QR code, and everything
between them is end-to-end encrypted (X25519, XChaCha20-Poly1305). Pair over
your own Wi-Fi with no account, or through the Vibezeo relay, which only
passes sealed messages along. The app collects no data and shows no ads.

Try it without a computer: the demo runs a made-up computer inside the app.

Requires Vibezeo for desktop (Linux today) on your computer.

## Review notes (App Review / Play review)
- Open the app and choose **Try the demo**. Every screen works against a
  simulated computer inside the app; nothing is sent anywhere.
- Real use needs the desktop app. Pairing is a QR code from Vibezeo ›
  Settings › Remote on the computer.
- No account, sign-in or purchase exists in the app.
- Encryption: standard algorithms (X25519, HKDF-SHA256, XChaCha20-Poly1305,
  ChaCha20-Poly1305) protect the app's own messages to the user's computer.

## Privacy answers
- **App Store privacy label:** Data Not Collected.
- **Play Data safety:** no data collected or shared; data is encrypted in
  transit; push notifications (when turned on) send the device's push token to
  the user's own computer over the encrypted channel, and the Vibezeo API
  forwards sealed alerts to Google or Apple without storing the token.
- **Privacy policy:** https://vibezeo.com/privacy/

## Screenshots
`node scripts/mobile/screenshots.mjs` renders demo-mode previews
(react-native-web). Capture the final store screenshots on real devices:
phone (6.7" and 6.1" iPhone; Android phone) and tablet (13" iPad).
