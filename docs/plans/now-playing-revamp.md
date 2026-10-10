# Now playing revamp: plan (#40 mobile, #47 desktop)

Written Sat 10 Oct 2026; implementation starts Mon 12 Oct, 6:10 am Canberra time.
Branch: `claude/funny-brahmagupta-9ggj7k`. The prototypes are attached to the issues. They are for structure only: keep the app's colours, copy, blurred moving background and Material Symbols Rounded icons.

**Hard constraint:** the immersive artwork from #59 (`lib/immersive.ts`, `ImmersiveArtwork.kt`, `display.immersiveArtwork`, the scene crossfade, and the flying artwork on Android) keeps working. The fullscreen layout follows the immersive notes in #47's comments, not the plain prototypes.

---

## 0. Environment

Set up and verified on Sat 10 Oct. If `~/android-sdk` and `desktop/node_modules` still exist, skip to step 5.

**Baselines on Sat 10 Oct (all green):**
- desktop: typecheck and lint clean, 152/152 unit tests passing, build done, `playback` e2e 3/3 under xvfb;
- Android: app 132, core 77, playback 65, with 14 opt-in screenshot tests skipped. `AppScreenshotTest` renders.

To rebuild a recycled container:

1. `apt-get install -y libasound2-dev pkg-config` (the native addon needs it), then `cargo install typeshare-cli cargo-ndk`, then `rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android`.
2. `cd desktop && pnpm install --frozen-lockfile && pnpm gen`. `pnpm gen` builds the bindings and the native addon. Electron downloads itself in postinstall.
3. Android SDK:
   - Unzip the latest `commandlinetools-linux-*_latest.zip` from dl.google.com into `~/android-sdk/cmdline-tools/latest`.
   - Run `sdkmanager --licenses`, then `sdkmanager "platform-tools" "platforms;android-37.1" "build-tools;36.0.0" "ndk;27.2.12479018"`.
   - Write `android/local.properties` with `sdk.dir=/root/android-sdk` (gitignored).
   - Run `scripts/build-android-core.sh`; without it the app shows a "Core not built" banner.
4. **Maven Central returns 429 through the container proxy.** Add `~/.gradle/init.d/central-mirror.gradle.kts`, which:
   - adds `https://maven-central.storage-download.googleapis.com/maven2/` to both `pluginManagement` and `dependencyResolutionManagement` in `beforeSettings`;
   - sets `systemProperty("robolectric.dependency.repo.url", mirror)` on every `Test` task, since Robolectric downloads its android-all jars itself at test time.

   This is environment only; nothing in the repo changes.
5. `git fetch origin main && git merge origin/main`, then re-run `scripts/gen-bindings.sh` if `api.rs` changed.
6. Before screenshots:
   - Android: `HOCKET_SCREENSHOT_DIR=… ./gradlew :app:testDebugUnitTest --tests '*AppScreenshotTest*' --rerun`. Saturday's set is in the session scratchpad `before/android/`.
   - Desktop: run the new screenshot spec (section 5) against `main` before the UI changes land.

## 1. Shared: a setting to hide the rating in the player (#40)

- New core registry setting `display.playerRating` (bool, default true): "Show rating in player".
  - The core registry is used rather than DataStore so it syncs and the desktop honours it too.
  - Files it touches: `crates/hocket-core/src/settings/{registry.rs,strings.rs}`, `desktop/src/shared/{settings-keys.ts,strings.ts}`, `desktop/src/main/fake-core/index.ts`, Android `SettingKeys.kt`, `FakeCore.kt` and `strings.xml`, plus `SettingsCategoriesTest`.
- Appearance section on both platforms, next to Immersive artwork.
- When it is off, the stars leave the player only. They stay in the song menu and the info sheet.

## 2. Android: mobile now playing (#40)

Main files: `ui/player/NowPlayingPage.kt`, `NowPlayingSheet.kt`, `Transport.kt`, `ui/queue/QueuePanel.kt`, `ui/components/RatingStars.kt`.

**Layout, top to bottom (Artwork mode)**
- Drag handle.
- Hero art. Keep `HeroGeometry`, `FlyingArtwork` and the immersive continuation exactly as they are. Only the slots below the art move.
- Title row:
  - title, then `artist · album`, then stars (when the setting is on);
  - a tonal round **add-to-playlist** button on the right, keeping the current `playlistAdd` glyph rather than a plus;
  - **tapping the title opens an info sheet**, a ModalBottomSheet hosting today's `PlayerAbout` content.
- Wavy seek: elapsed on the left, remaining (`-0:47`) on the right.
  - The centre shows **"Playing on {device}"** in the accent colour, only when another device holds the lease (`!ownsTransport`).
  - The current notice lines (resume offer, autoplay reason, sleep) go in that same centre slot when there's no remote device, so nothing is lost.
- Expressive transport: a large pill play/pause flanked by asymmetric-corner previous/next buttons (M3 Expressive button group shapes, with press-morph like today).
- **Bottom action row:**
  1. A split pill. The **queue name** (`contextLabel`, or "Autoplay") opens Queue mode. A divider, then a chevron that opens `QueueSwitcherSheet`. The pill is filled with the accent while Queue mode is on.
  2. **Device switcher**: the current device's icon (phone, laptop, speaker…, from the lease owner's device kind). Tonal normally; accent when controlling another device, as Cast is today. Tap opens `HandoffSheet`; long-press takes over, as it does today.
  3. **Lyrics** toggle, filled accent while Lyrics mode is on.
  4. **More**, which opens the song menu (`LocalSongMenu.open(...)`). Sleep timer joins its extras, because the About pill and header that held it go away.

**Lyrics and Queue modes**
- The art area becomes a rounded tonal panel (keep the existing `LyricsPage embedded` and `QueueTimeline`; the issue says keep them mostly as they are). The title row gains a small thumbnail on the left, as the existing `FlyingArtwork` already does for non-Artwork modes. Tapping the thumbnail returns to Artwork.
- In Queue mode, the shuffle/repeat/autoplay pills (`QueueModeHeader`) move to the **bottom** of the queue panel. They sit on a gradient that fades from transparent to the panel colour, with the list scrolling underneath.
- The old header ("Playing from", Cast, collapse) and the Lyrics/Queue/About `ModeBar` are removed. Collapse stays available by dragging the handle and with system back.

**Swipeable stars**
- `RatingStars` gains horizontal drag: the rating follows the finger, rounded to whole stars, committed on release, with a haptic tick per star.
- Tap keeps its current behaviour, including tap-again-to-clear. Accessibility stays a slider-like semantic with custom actions.

**Tests to update:** `FullPlayerLayoutTest`, `NowPlayingSheetTest`, `PlayerAccessibilityTest`, `LargeFontLayoutTest`, `SongMenuTest`. New tests cover the title tap opening the info sheet, the star drag, the "Playing on" visibility, and the setting hiding the stars.

## 3. Desktop: bottom bar, sidebar, fullscreen (#47)

### 3a. Bottom bar (`components/PlayerBar.tsx`, `styles/global.css`)

- **Left:** art, title and artist, stars (setting-aware), then **add to playlist** and **more** (`openMenuFromButton`).
  - Love moves into the more menu, so it is still there, just not in the bar.
- **Centre:** shuffle, previous, a large expressive play/pause pill, next and repeat. Seek below, with times on either side.
  - Autoplay moves to the queue view header, as in the prototype.
- **Right, in order:**
  1. toggle sidebar
  2. lyrics
  3. queue
  4. **connect pill**: the device icon, plus the device name **only when playing elsewhere**, with an accent fill then
  5. miniplayer
  6. fullscreen

  Volume is not in the prototype, but it stays (compact, before the toggles); removing it would be a regression. The sleep and battery badges stay as they are.
- Keep every existing `data-testid`. Narrow (<900px) two-row behaviour stays.

### 3b. Right sidebar (`components/RightPanel.tsx` is replaced by a new `NowPlayingSidebar`)

**Overview** is one scrollable column:
- **Art**, which crossfades on track change.
- **Title and artist**, with add-to-playlist and more.
- **Lyrics card**: "Lyrics" plus a "Show lyrics" link, and three synced lines (previous, current, next). Hidden when the track has no lyrics.
- **Up next card**: "Up next" plus an "Open queue" link, and the next 3 rows.
- **About card**: "About" plus a "More" link, and Album, Genre, Plays, Format.

**Drill-in views** have a back arrow and a title:
- **Lyrics** hosts the existing `LyricsPane`/AMLL view.
- **Queue**:
  - The header row has the **queue selector dropdown** (queue name + chevron). It lists the saved queues and replaces the old "Recent" tab, keeping restore, pin, save and delete through each row's menu, and links to the full Recent list.
  - Next to it is the **Autoplay** pill.
  - Below that are the existing `QueuePanel` timeline (Now playing, Up next, drag reorder) and the "Autoplay continues with similar songs" footer.
- **About this song**: art, title, a two-column grid, then the path and "Show in folder". This shares a component with fullscreen `About`.

**Behaviour**
- The bar's lyrics and queue buttons open the sidebar straight into that view; pressing the same button again closes it. The toggle-sidebar button opens the overview.
- The state (open, view, width) lives in `store/app.ts` panels (localStorage `hocket.panels`). The old `splitRatio`/divider and the `display.queuePanelSplit` setting are retired; tests that use `panel-divider` are updated.
- Width stays resizable. Below 900px it remains a drawer.

### 3c. Fullscreen (`views/FullscreenPlayer.tsx`, `styles/now-playing.css`)

**Prototype layout** (card covers, and windows narrower than 3:2):
- Art-only mode: centred art with the controls below.
- Lyrics, queue and about modes: art and controls on the left, the panel on the right.
- Controls:
  - title, then `artist — album`;
  - round add and more buttons;
  - stars;
  - wavy seek with elapsed and remaining;
  - shuffle, previous, a big play/pause, next, repeat.
- **Top-right:** a collapse button. The existing window-fullscreen toggle sits beside it.
- **Bottom-right pill:** the connect device (with a ▾ dropdown and the name only when remote), a divider, then toggles for lyrics, queue and info. Clicking the active toggle returns to art only.
- Volume and sleep go into the pill as compact icon buttons (volume opens a popover slider), so nothing is lost.

**Immersive layout** (per #47's comment): wide windows (3:2 or wider) with mirror or extend covers.
- The art runs full height on the left, flush to the edges, with the existing right-edge continuation.
- Song details and controls sit in a floating **glass card** (backdrop blur over the art) at the bottom of the art.
- The bottom-right pill is as above.
- **Art only:** the continuation fills the right side.
  - While playing, with the mouse idle for about 3 s, the card and pills fade out. Mouse movement or a key brings them back.
  - They never hide while paused, while focus is inside them, or while a menu is open.
  - New setting: "Hide controls when idle in fullscreen" (local, default on), in Appearance.
- **Lyrics and queue:** the continuation shortens and dissolves into the Kawarp blur, and the panel sits on the blur.
  - The controls card does **not** move between modes.
  - This is an extra fade mask on the continuation canvas, driven by mode. The scene crossfade from #59 is left untouched.
- **Tall windows:** the mobile layout, which is the existing `TALL_IMMERSIVE` bottom continuation, with the same controls stacked.

**Mode memory:**
- The default mode is **queue**, not art.
- `hocket.nowPlayingMode` already persists the last used mode, so `loadNowPlayingMode()` only changes its fallback.
- Q, L and Escape keep working.

**Tests and docs:**
- Keep `fullscreen-player`, `np-artwork`, `np-thumb`, `np-about`, `np-source`, `fs-*` and `toggle-fullscreen`. Add `fs-pill-*` and `sidebar-*`.
- Update `panels.spec.ts`, `chrome.spec.ts`, `exit-motion.spec.ts`, `motion-contrast.spec.ts`, `a11y.spec.ts`, `keyboard-a11y.spec.ts` and `lyrics.spec.ts`.
- Update `docs/design.md` "Desktop layout" and the fullscreen bullet.

## 3d. Visual fidelity: the prototypes' shapes and spacing are the spec

The user wants the prototypes' vibe kept: shapes, proportions and spacing.
- The colours, copy, background and font stay the app's own.
- The prototype images are on this branch in `docs/plans/prototypes/`. The GitHub attachment URLs need a login, so a new container can't fetch them.
- While building, compare each screen side by side with its prototype, and include those pairs with the final screenshots.

Measurements were taken off the images (both are 2×) and rounded to the app's spacing scale:

**Mobile** (`issue40-*`; a 390 dp-wide phone)

| Part | Size |
|---|---|
| Side margins | 20 dp. Everything aligns to them. |
| Art | Full width (350 dp square), about 26 dp corners, 53 dp below the top of the sheet (under the drag handle) |
| Title block | Title 24 sp extra-bold, then artist · album 16 sp muted, then 5 stars at about 14 dp with 6 dp gaps |
| Add-to-playlist | A 48 dp tonal circle, right-aligned with the title |
| Seek | Wavy, the full content width. Times and "Playing on" are 13 sp on the line below. |
| Transport | One row, **84 dp tall**, 10 dp gaps. Previous and next are 84 dp squares with about 30 dp outer and about 14 dp inner corners (the expressive "leaf" shape). Play/pause fills the rest (about 161 dp) with about 28 dp corners, filled accent. Previous and next are tonal. |
| Gap from transport to bottom row | About 38 dp |
| Bottom row | 44 dp tall, 18 dp from the bottom. The queue pill is a full-radius tonal pill about 206 dp wide: name at 16 sp bold, a thin divider, then the chevron. Next comes a 44 dp tonal circle (device), then two 44 dp icon buttons with no fill (lyrics, more). An active toggle becomes a filled accent circle or pill. |
| Lyrics and queue panels | Fill the art's slot (same 350 dp square, same corners), tonal fill. The title row gets a 52 dp thumbnail with about 14 dp corners. The queue's mode chips are about 40 dp full-radius pills, and the bottom strip is a gradient. |

**Desktop** (`issue47-*`; a 1440 px window)

| Part | Size |
|---|---|
| Window gutters | 8 px everywhere: around and between the main panel and the sidebar, and above the bar. Panels have **20 px** corners on a slightly darker shell. |
| Sidebar | 360 px wide, 16 px inner padding, art 328 px square with about 16 px corners. Cards are tonal with **16 px** corners and 16 px padding, 12 px apart. Card titles are 15 px bold, and the action link ("Show lyrics", "Open queue", "More") is 13 px accent. |
| Bottom bar | **92 px** tall, no top border, on the shell colour. Left: art 64 px with 12 px corners, then title 15 px bold, artist 13 px, stars 12 px, then add and more as plain 36 px icon buttons. Centre: 36 px circular icon buttons; play/pause is a **52×44 px rounded rectangle (about 14 px corners)** in filled accent. An active shuffle or repeat gets a light circle. The seek sits under the transport, about 340 px wide, with times either side. Right: 36 px icon buttons, an active toggle on a tonal circle. The connect pill is **40 px tall**, full radius, filled accent when remote. |
| Fullscreen (card layout) | Art 440 px square with about 28 px corners and a soft shadow, 120 px from the left. Below it the title is 22 px extra-bold and the artist line 16 px. The round add and more buttons are 36 px tonal. Then stars, then the wavy seek (art width), then the transport row: 44 px shuffle and repeat circles, and play/pause as an **80 px rounded square (about 24 px corners)**. The panel starts about 80 px right of the art. The collapse button is a 44 px tonal circle, 28 px from the top-right. The bottom-right pill sits 40 px from the edges, about 52 px tall, full radius, tonal; connect sits inside it as a filled pill, then a divider and 44 px toggle circles. |

## 4. Order of work

1. Environment and baseline (section 0).
2. The shared setting (section 1).
3. **Desktop:**
   1. bar
   2. sidebar overview
   3. drill-ins and queue selector
   4. fullscreen prototype layout
   5. immersive variants and auto-hide

   Run typecheck, lint and unit tests after each step, and the e2e suite at the end.
4. **Android:**
   1. bottom row and title row
   2. transport and the "Playing on" line
   3. info sheet
   4. Queue and Lyrics mode panels with the gradient pills
   5. swipe stars

   Run the unit and Robolectric tests after each step.
5. Commit in logical steps and push to `claude/funny-brahmagupta-9ggj7k`. No PR unless asked.

## 5. Screenshots (deliverable)

**Desktop:** a new `e2e/now-playing-screenshots.spec.ts` modelled on `immersive-screenshots.spec.ts` (FakeCore, `HOCKET_SCREENSHOT_DIR`, waits for animations). At 1600×1000, light and dark:
- the bar with the sidebar closed;
- the sidebar overview, plus lyrics, queue (with the selector open) and about;
- the connect pill local and remote;
- fullscreen in art, lyrics, queue and about, in card layout and immersive wide (mirror and extend), including the idle-hidden state;
- a 900×1400 tall fullscreen.

**Android:** extend `AppScreenshotTest`. At 411×891 dp, light and dark:
- Artwork mode, local and remote ("Playing on" plus the accent device button);
- Lyrics mode and Queue mode (gradient pills);
- the queue selector sheet;
- the info sheet;
- the song menu;
- the stars hidden by the setting;
- one immersive cover via `ImmersiveScreenshotTest`.

Put them together in one contact sheet per platform and send them with `SendUserFile`, along with before/after pairs.

## 6. Defaults taken (shout before Monday to change)

- Love leaves the desktop bar and goes into the more menu; it stays on the fullscreen controls.
- Volume and sleep stay on the desktop: in the bar, and in the fullscreen pill.
- Android's About mode becomes the info sheet, opened by tapping the title. Sleep timer moves into the more menu.
- The rating setting is a synced core setting that applies to both platforms. The idle auto-hide setting is desktop-local.
- The desktop's Recent tab is folded into the queue selector dropdown.
