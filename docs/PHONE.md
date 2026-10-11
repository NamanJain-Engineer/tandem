# Vibezeo on your phone

How the phone companion works today, at no cost, and what each piece needs.
Phones can't run the coding CLIs (iOS forbids starting processes, and Google
Play forbids downloaded executables), so a phone is a remote control for the
agents on your computer: **Vibezeo Remote** (F-RM-03, F-RM-04, F-RM-05).

## What the phone can do

- See your computer's agents and their status (working, needs you, done, error).
- Answer approvals. **Deny** always applies. **Allow** applies at once only while
  your computer has "Allow approvals from phone for the next 8 hours" on;
  otherwise it waits for you to confirm at the computer (F-TL-06 rule 5).
- Read threads and teammate chats, and send to them (with Chat control).
- Type into agent panes (with Terminal control). Never into plain shells.
- Run a routine now (with the Routines permission).
- Unpair.

Each permission is off until you turn it on for that phone in
Settings › Remote on the computer.

Everything between the phone and the computer is end-to-end encrypted
(X25519 + HKDF pairing, XChaCha20-Poly1305 frames with counters, hourly
rekeys; ARCHITECTURE §13.3). Whatever relays the frames sees only sealed bytes.

## Pairing without an account: LAN mode (F-RM-04)

The free download has no hosted relay, so your computer can relay for itself
on your own network.

1. On the computer: Settings › Remote › Local network › turn on
   **Pair over your local network**. It's off until you do this.
2. Choose **Pair a phone on this network**. A code appears.
3. On the phone (same Wi-Fi as the computer): scan the code with the
   Vibezeo Remote app, or with the camera (it opens the phone web app,
   served by your computer).

What it listens on: only your computer's private addresses (home or office
networks: 10.x, 172.16–31.x, 192.168.x; and 100.64.0.0/10, which VPNs such as
Tailscale use), on port 47800. Never on all interfaces, never on a public
address. Connections from outside those ranges are closed unread. Turning the
switch off stops listening and disconnects phones.

### Away from home: Tailscale (free plan)

Tailscale puts your computer and phone on one private network wherever they
are, without opening anything to the internet.

1. Install Tailscale on the computer and the phone and sign in to the same
   account (the free Personal plan is enough).
2. On the computer, Settings › Remote › Local network lists its Tailscale
   address (100.x.y.z, marked VPN). Choose it under "Address in the code".
3. Pair with that code. The phone reaches the computer through Tailscale at
   home and away.

Vibezeo doesn't build or run a VPN; Tailscale is a separate product with its
own terms.

## iPhone today: the phone web app

Until the iOS app is published (it needs a paid Apple Developer account), an
iPhone uses the web app:

- **LAN mode:** scan the code with the Camera app. Safari opens
  `http://<your computer's address>:47800/remote`, served by your computer.
  Share › **Add to Home Screen** keeps it one tap away. (It's a plain-HTTP page
  on your own network, so iOS can't install it offline; the messages inside are
  still end-to-end encrypted.)
- **Hosted relay** (Pro, once the hosted service is live): the code opens
  `https://app.vibezeo.com/remote`.

If your computer's address changes (a new Wi-Fi network or router), make a new
code. A Tailscale address doesn't change.

## Android: the Vibezeo Remote app

Built from `apps/mobile` (Expo / React Native) with
`bash scripts/mobile/build-android.sh`: a signed APK for direct download and
an AAB for Google Play. To install an APK directly, allow your browser or file
manager to install unknown apps when Android asks.

## Notifications

- **In the app (default on):** while the app is open, it tells you about new
  approvals and agents that finish or need you. These never leave the phone.
- **Push (off by default, F-RM-06):** for alerts while the app is closed. Needs a
  pairing through the hosted relay; your computer seals each alert so the push
  service carries bytes it can't read. Not available over LAN mode.

## Unpairing

From the phone (Settings › Unpair this phone) or the computer
(Settings › Remote › Revoke). Either way the computer forgets the phone's keys
and the relay refuses it; signing out on the computer revokes every phone.
