# Manual spikes (need a person, a Google account or real hardware)

These couldn't be done on the dev host. Each item says what to do and what to record. Put the results in this file.

## 1. Sign-in: Android WebView cookie capture

**What we infer.** Metrolist, InnerTune and SimpMusic sign in by loading `https://accounts.google.com/ServiceLogin?continue=https://music.youtube.com` in a WebView. They then read `CookieManager.getCookie("https://music.youtube.com")`. Google's WebView block (`disallowed_useragent`) applies to OAuth consent pages, not to plain web sign-in, so this is expected to work. Not verified here.

**Test:**
1. In the emulator (`android-dev` template) or on a phone, run a throwaway WebView activity:
   - JavaScript and DOM storage enabled
   - default UA, then the UA with `; wv` removed
2. Sign in with a test Google account, including 2FA.
3. Record:
   - whether sign-in completes
   - the cookie names present: need `SAPISID`, `__Secure-3PAPISID`, `__Secure-3PSID`, `__Secure-3PSIDTS`, `LOGIN_INFO`
   - whether `music_liked_tracks` and `music_saved_albums` work with that cookie string (the Rust spike binary or a unit test)
   - whether the cookie still works after 24 h and after 7 days without the WebView being reopened. rustypipe never rotates cookies, and its docs say the source browser session must not keep being used.

## 2. Sign-in: Electron BrowserWindow

**What we infer.** Google blocks Chromium-embedded browsers it detects ("This browser or app may not be secure") mainly by user agent and automation signals. Electron wrappers of music.youtube.com work around it by setting a plain Chrome UA, without the `Electron/x` and app-name tokens, on the login window's session. Loopback can't hand back web cookies (that only works for OAuth, and Google blocks the TV/OAuth client for InnerTube). So the realistic fallback is pasting cookies or a `Cookie:` header from a normal browser's devtools.

**Test:**
1. In the desktop app, open a `BrowserWindow` with its own `partition` on the same accounts URL:
   - (a) default Electron UA
   - (b) `session.setUserAgent(<current stable Chrome UA>)`
2. Record:
   - whether sign-in completes, including 2FA and passkeys
   - the same cookies and checks as in section 1
3. If both fail, time how long the paste-cookies flow takes for a non-technical user.

## 3. Premium and signed-in stream formats (itag 141)

**What we infer.**
- Anonymous clients never offered 141 (see [streams.md](streams.md)).
- yt-dlp's Premium client set (`web_creator`, `tv_samsung`, `web`) needs player-JS signature/`n` solving, and the web clients often need PO tokens or are SABR-only.
- rustypipe's deobfuscator is broken. So 141 may be unreachable without a working JS solver.
- No source showed 141 ever appearing anonymously.

**Test:** run the probe with Premium cookies for each client, using `probe.py` from the spike (or an equivalent) with a `Cookie` header and `SAPISIDHASH` `Authorization`:
- `VISIONOS`
- `IOS`
- `ANDROID_VR`
- `WEB_REMIX` (needs `signatureTimestamp`)
- `TVHTML5`

Record:
- the audio itags listed
- whether the URLs are plain, ciphered or SABR-only
- whether a full ranged download works
- the same, with a free signed-in account

## 4. Cast IP binding (physical Chromecast)

**Known:** googlevideo URLs carry `ip=<public IP of the resolving device>` (IPv4 on the dev host). The receiver must fetch from the same public address.

**Test:** resolve itag 140 on the phone and hand the URL (`audio/mp4`) to the Default Media Receiver in each case:
1. Phone and Chromecast on home Wi-Fi, phone resolving over IPv4. Force it with the resolve request going to an IPv4-only address.
2. The same, phone resolving over IPv6 (if the LAN has IPv6). The Chromecast may fetch over IPv4.
3. Phone on mobile data, Chromecast on Wi-Fi.
4. Phone on a VPN.

For each, record whether playback starts and the HTTP status, if visible, from the receiver debug log.

## 5. On-device first-resolve latency (pass < 1.5 s on the emulator)

1. Build an APK with the Phase 4 resolver, or a test hook that calls it.
2. In the `android-dev` emulator, time a cold start through to the first audio bytes:
   - the visitorData fetch
   - the `player` call
   - the first ranged GET
3. Repeat 5 times. Host reference: 0.70–0.93 s.

## 6. Media3 priming check

1. Play `4D7u5KF7SP8` itag 140 through ExoPlayer.
2. Capture the first output buffers (an `AudioProcessor` tap or `AnalyticsListener`).
3. Check whether output starts at media sample 1600 (exact trim) or 2048 (whole negative-timestamp buffers dropped).
4. Compare against desktop after the `elst` trim fix lands.
