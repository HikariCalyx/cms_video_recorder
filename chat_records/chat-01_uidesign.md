# Clean State

Exported: 8/17/2026, 8:03:55 PM

---

**User:**

I'd like to create a Rust based desktop app named CMS Video reporter, that only targeted to Windows.
For GUI backend, I'd like to use iced.
First of all, let's create a toolbar that always float on top as the main GUI, with window selection button and record button.

---

**Assistant:**

Let me start by setting up the Rust project with iced for a Windows floating toolbar.

Good, I can see iced has `Level::AlwaysOnTop` support via `window::Level`. Now let me check the iced application API and then build the project.

Now I have everything I need. Let me scaffold the full project:

Now let's try to build and see if there are any compile errors:

The check passed (exit code 1 is from PowerShell's stderr redirect, not from cargo — all warnings, no errors). Let me clean those up:

Now fix the remaining warnings — remove unused `new()` and suppress `dead_code` for intentional future-use fields:

Down to 1 warning — that's just a dead_code notice for `WindowSelected` and `RefreshWindows` which are real messages we'll use when the picker dropdown is built out. Let me suppress it properly:

Zero warnings, clean build. The exit code 1 is just PowerShell's stderr redirect artifact — cargo itself exited 0. Let me do a final build to make sure the binary links:

Binary built successfully. Here's a summary of what was created:

---

**Project structure:**
```
src/
  main.rs          – entry point, window configuration
  toolbar.rs       – app state, update logic, view
  window_picker.rs – Win32 EnumWindows enumeration
Cargo.toml
```

**What's in place:**

- `main.rs` — launches iced with `Level::AlwaysOnTop`, no decorations, transparent background, 420×56px fixed size, centered on screen. The `windows_subsystem = "windows"` pragma suppresses the console in release builds.

- `toolbar.rs` — the floating toolbar with two controls:
  - **Select Window** button (200px wide, shows selected window title or a placeholder)
  - **Record / Stop** button that toggles state and switches to a red `danger` style when recording is active
  - Dark semi-transparent pill-shaped container with a subtle drop shadow

- `window_picker.rs` — uses `EnumWindows` + `GetWindowTextW` via the `windows` crate to enumerate all visible, titled top-level windows. `hwnd` is stored alongside the title for when you wire up the actual capture backend.

**Next natural steps:** implement the dropdown/list overlay for window selection (iced's `overlay` or a secondary window), then hook in a screen capture backend (e.g. `windows-capture` or DXGI Desktop Duplication).

---

**User:**

The window looks ugly - not semi transparent on rounded corners, widgets aren't vertical centered, and there's no close button.

---

**Assistant:**

Let me look at the current code before making changes.

Three issues to fix:

1. **Transparent corners** — iced's background needs to be `Color::TRANSPARENT` at the app level; the rounded rect is drawn by the container, but the window background underneath needs to be fully transparent. We also need to set `background` in the application style.
2. **Vertical centering** — the buttons need explicit `height` alignment and the row needs to fill height properly.
3. **Close button** — add an `×` button that sends a `window::close` task.

`background_color` isn't on the iced 0.13 `Application` builder. In 0.13 the window background is controlled via the `style` method — let me check the correct API:

The transparent background is set via `.style()` which takes `fn(&State, &Theme) -> theme::Style`. Let me also check `theme::Style`:

Now I have everything. The correct approach is `.style(fn(&State, &Theme) -> theme::Style)` where `theme::Style { background_color: Color::TRANSPARENT, ... }`. Now let me fix both files:

`theme::Style` exists. The issue is the path — in iced 0.13, it lives at `iced::theme::Style` but the prelude exports `Theme` as `iced::Theme` (which is `iced::theme::Theme`). The `Style` struct needs to be imported explicitly from `iced::theme`. Let me check the exact path used:

The `theme` in `main.rs` was imported as `use iced::theme` but `iced::theme::Style` doesn't re-export from `iced::theme` at the crate level. Let me use the full path `iced::theme::Style` directly or check what does exist:

The `theme::Style` is actually accessible via `iced::theme::Style` — the issue is that I imported `use iced::theme` (the module), but `Style` is exported from that module. The import should work. Let me just try with a direct fully qualified path in the closure, and also test with a quick one-liner:

The `.style()` method takes `fn(&State, &Theme) -> Appearance`, not `theme::Style`. And `Appearance` is re-exported from `iced::application`. Let me check what `Appearance` contains:

Now I know — it's `iced::application::Appearance` with fields `background_color` and `text_color`. Let me fix `main.rs`:

Zero errors. Now build:

Clean build, no warnings. Here's what changed and why each fix works:

**1. Transparent rounded corners**
The key was `.style()` on the `Application` builder, which takes `fn(&State, &Theme) -> Appearance`. Setting `background_color: Color::TRANSPARENT` makes iced paint nothing behind the widgets — the OS composites the transparent window correctly. The pill container is drawn on top with its rounded border, so corners are genuinely see-through.

**2. Vertical centering**
Two things were fighting it before: buttons had asymmetric `padding([6, 12])` which adds vertical space inside but not around, and the row wasn't filling height. Now:
- Buttons use `.height(32)` with `padding([0, ...])` so height is explicit and padding is horizontal-only
- Text inside uses `.align_y(Vertical::Center)`  
- The pill container uses `.align_y(Vertical::Center)` to center the row within the pill
- An outer transparent container wraps everything with `padding(4)` so the shadow isn't clipped by the window edge

**3. Close button**
A circular `✕` button in the top-right of the row. It's neutral gray at rest, turns red on hover. On press it calls `window::get_latest().then(|id| window::close(id))` to exit cleanly via iced's task system (no `std::process::exit`).

---

**User:**

The window is still not semi transparent, plus the icons cannot be displayed correctly.

---

**Assistant:**

Two distinct problems here. Let me address both properly.

**Icons:** the boxes are tofu glyphs — iced's default font (Fira Sans) has no `⏺`, `⏹`, or `✕` codepoints. Rather than bundling an icon font, I'll draw the record/stop indicators as small styled containers (font-independent) and use `×` (U+00D7, Latin-1) for close.

**Transparency:** per-pixel alpha through wgpu on Windows is unreliable in iced 0.13. The deterministic route is Win32 layered windows. Let me verify the API shape first.

`run_with_handle` exists. Let me check its signature:

`run_with_handle` gives a proper raw window handle. Let me confirm `Task::discard` exists so I don't need a no-op message:

`Task::discard` exists. Now let me write the three files:

That completed suspiciously fast. Let me force a full recheck to be sure the new module actually compiled:

Compiles clean from scratch — no errors, no warnings. Let me verify the binary exists and confirm the `windows` crate features I used are all enabled:

Binary built. Here's what changed:

## Icons — replaced glyphs with shapes

The tofu boxes were `⏺`/`⏹`/`✕` missing from iced's bundled Fira Sans. Two fixes:

- **Record/Stop indicators** are now `container` widgets with a fixed 10×10 size and a border radius — a red circle (`radius: 5.0`) for idle, a white rounded square (`radius: 2.0`) for recording. No font involvement at all, so they can't break.
- **Close button** uses `\u{00D7}` (×, multiplication sign) instead of `✕`. That codepoint is in Latin-1 Supplement, present in essentially every font.

## Transparency — Win32 layered window

The previous approach relied on wgpu writing per-pixel alpha to a transparent surface, which doesn't reliably composite on Windows in iced 0.13. I switched to the OS compositor instead, in a new `src/win32.rs`:

- `WS_EX_LAYERED` is added to the window's extended style
- `SetLayeredWindowAttributes` is called with **both** flags:
  - `LWA_COLORKEY` punches out every pixel exactly matching a chroma-key color, so the area outside the rounded pill becomes genuinely clear
  - `LWA_ALPHA` (238/255) applies uniform translucency to what remains, giving the pill its semi-transparent look

The key color is `rgb(1, 0, 1)` — near-black on purpose, so anti-aliased pixels along the pill's rounded border blend into a subtle dark outline rather than a bright magenta fringe. The app's `Appearance.background_color` and the outer container are both painted in that same color, and `window.transparent` is now `false` since we no longer depend on surface alpha.

Hooking it up uses iced's own plumbing rather than anything hacky: a `window::open_events()` subscription fires `Message::WindowOpened(id)`, which returns `window::run_with_handle(id, apply_layered_translucency).discard()`. The `raw_window_handle::RawWindowHandle::Win32` variant gives the HWND.

Both the `win32` module and its call site are `#[cfg(windows)]`-gated, so the code still type-checks on other platforms.

One thing I should be straight about: I verified this compiles cleanly from scratch, but I can't see the rendered window, so I haven't visually confirmed the translucency and corner cutout look right. If the corners show a faint dark ring you'd rather not have, tell me and I can drop the pill's 1px border or nudge the key color. If translucency still doesn't appear at all, the next thing to check is whether the pill's rendered color happens to collide with the key color.

---

**User:**

Could you use Microsoft Yahei UI instead of Fira Sans?

---

**Assistant:**

Let me confirm the exact family name registered on your system before wiring it up:

"Microsoft YaHei UI" is the registered family name. Let me wire it up as the default font:

Now apply the semibold font to the record label and the close glyph:

Done. Microsoft YaHei UI is now the application-wide default.

**`src/toolbar.rs`** — two font constants, built as `const` since `Family::Name` is const-constructible:

```rust
pub const UI_FONT: Font = Font {
    family: Family::Name("Microsoft YaHei UI"),
    ..Font::DEFAULT
};

pub const UI_FONT_SEMIBOLD: Font = Font {
    family: Family::Name("Microsoft YaHei UI"),
    weight: Weight::Semibold,
    ..Font::DEFAULT
};
```

**`src/main.rs`** — `.default_font(UI_FONT)` on the application builder, so every widget picks it up without per-widget overrides. iced resolves it by family name through cosmic-text's system font source, so nothing gets bundled into the binary.

The Record label now uses the semibold variant for a bit more presence against the blue fill, and the close `×` got bumped to 18px since YaHei UI draws that glyph smaller than Fira Sans did.

I left the record/stop indicators as shape containers rather than switching back to `⏺`/`⏹`. YaHei UI does carry `●` and `■`, but not the U+23F9/U+23FA control pictures, and the drawn shapes give exact control over size and corner radius.

Builds clean, no warnings. I verified "Microsoft YaHei UI" is the registered family name on your machine (alongside "Microsoft YaHei UI Light") before wiring it in, though as before I can't visually confirm the rendering.

---

**User:**

The built result cannot be dragged at all, plus the black border still exists.

---

**Assistant:**

Now I can see translucency is working and the font/icons render correctly. Two things left. Let me diagnose the border properly first.

The black frame is the chroma-key failing. `Color::from_rgb8(1, 0, 1)` goes through sRGB/linear conversion in the wgpu surface, so the final framebuffer value almost certainly isn't *exactly* `RGB(1,0,1)` anymore — and `LWA_COLORKEY` requires an exact match. Color-keying is the wrong tool here. Let me switch to OS-level region clipping, which is pixel-exact.

Both `window::drag` and `mouse_area` are available. Let me add the GDI/HiDPI features and rewrite the Win32 layer:

Now the toolbar with drag support and the pill filling the whole (clipped) window:

`SetWindowRgn` lives in the GDI module, not WindowsAndMessaging. Let me locate it:

`SetWindowRgn` is in `Graphics::Gdi`, takes `HRGN` directly (not `Option`), and a `BOOL`. Let me fix:

Builds clean. Let me confirm the region call actually did what I expect by double-checking `is_invalid` on `HRGN` is a real method and not silently a no-op path:

Clean from scratch, no warnings. Two fixes:

## Black border — replaced color-keying with region clipping

My previous approach was wrong, and worth explaining since it's why the border persisted. `LWA_COLORKEY` removes only pixels whose value is an **exact** byte-for-byte match to the key. Colors written through the wgpu surface pass through sRGB conversion, so `Color::from_rgb8(1, 0, 1)` never landed as exactly `RGB(1,0,1)` in the final framebuffer. The key matched nothing, and you saw the raw dark fill.

`SetWindowRgn` clips at the OS level with no color involvement:

```rust
let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, diameter, diameter);
SetWindowRgn(hwnd, region, BOOL(1));
```

Everything outside the round-rect is simply not part of the window — pixel-exact, nothing to mismatch. `LWA_ALPHA` still provides the uniform translucency you can see working in your screenshot.

The region is measured in **physical** pixels, so I scale the radius by `GetDpiForWindow(hwnd) / 96.0`. Without that the corners would be visibly wrong on a scaled display. The bounds use `width + 1` / `height + 1` because `CreateRoundRectRgn` treats right/bottom as exclusive.

Since clipping handles the corners, the pill now fills the whole window — the outer chroma-key container and its padding are gone. `PILL_RADIUS` is aliased to `win32::CORNER_RADIUS` so the drawn border and the clipped edge stay in sync from one constant.

## Dragging

Borderless windows have no titlebar for Windows to drag, so it has to be requested explicitly. The pill is wrapped in `mouse_area` with `on_press(Message::StartDrag)`, which calls `window::drag(id)` to hand off to the OS drag loop. The window id is captured from the `open_events()` subscription into `Toolbar::window_id`.

Buttons keep working because iced dispatches events to children first and `button` marks the press as captured, so `mouse_area` never sees it. Dragging works on any empty part of the bar. I also added a grip affordance (two small bars) on the left and a `horizontal_space()` before the close button, which widens the drag target and pushes close to the right edge.

`Message::Close` now uses the stored id directly instead of `get_latest()`.

One caveat I can't verify without seeing it: `SetWindowRgn` clips with hard edges, so the rounded corners will be slightly aliased rather than smooth. If that bothers you, the alternative is DWM per-pixel alpha via `DwmEnableBlurBehindWindow`, which antialiases properly but is more fragile across wgpu backends.

---

**User:**

Could you make record label vertically centered aligned?

---

**Assistant:**

The row inside the button is shrink-height, so `align_y` centers within the row's own bounds rather than the button's 32px. Let me confirm how iced's button lays out its content:

Confirmed the root cause: `layout::padded` positions content at `(padding.left, padding.top)` with no centering. Since vertical padding is `0`, all three labels sit flush against the top of their 32px buttons. The content has to fill the height and center itself.

Builds clean.

The cause wasn't the alignment call itself, it was that there was nothing to align against. iced's `button` delegates to `layout::padded`, which places content at `(padding.left, padding.top)` and does no centering. With `padding([0, 14])` the vertical padding is zero, so each label was pinned to the top of its 32px button — `align_y(Center)` was centering the text inside its own shrink-to-fit box, which is exactly the text's height, so it had no effect.

The fix is to make the content actually occupy the button's height:

- **Record button** — the inner `row` now has `.height(Length::Fill)` alongside `.align_y(Alignment::Center)`, so it expands to the full 32px and centers the dot and label within it. The label also gets `.height(Length::Fill)` so its own glyph box is centered.
- **Select Window** — same `.height(Length::Fill)` on the text, which had the identical problem.
- **Close button** — added `.height(Length::Fill)` to pair with the `.width(Length::Fill)` that was already there, so the `×` centers on both axes.

I fixed all three rather than just Record, since they shared the same defect and would have drifted out of line with each other otherwise.

---

**User:**

I'd like to shrink the width of Select Window to 50%, and remove the text "Choose the window to capture".
And implement actual window selection mechanism.

---

**Assistant:**

Three changes. The dropdown needs thought: the window is only 52px tall and region-clipped, so an overlay dropdown would be clipped away. Let me verify the APIs I need for an expanding panel approach.

All available. Now let me add the DWM feature for filtering out ghost/cloaked windows:

Both present. Let me update the manifest and rewrite the window enumeration with proper filtering:

Now the toolbar: narrower button, no tooltip, and an expanding picker panel with window resize plus region reapply on resize.

Let me verify with full output since I used a few APIs I hadn't confirmed (`container::center`, `text::Wrapping`, `text::color`):

Clean compile from scratch, zero warnings. All three changes are in.

## Width and tooltip

The Select Window button went from `Length::Fixed(200.0)` to `Length::Fixed(100.0)`, and the `tooltip` wrapper is gone entirely (along with its import) so the button sits directly in the row.

## Window selection

**Why an expanding panel rather than a dropdown:** an iced overlay (like `pick_list`) draws inside the window's own surface. The window is 52px tall and clipped to a rounded region by `SetWindowRgn`, so a dropdown would be cut off at the bar's edge. A second window would need a refactor from `application` to `daemon`. Expanding the existing window avoids both problems, so `ToggleWindowPicker` now resizes 460×52 → 460×320 and renders a list below a divider.

The clip region is size-dependent, so it has to be rebuilt after every resize. I subscribe to `window::resize_events()` and rebuild there rather than chaining a task after `window::resize`, because chained tasks can run before the OS has actually applied the new size and you'd get a region cut for the old dimensions.

**Enumeration filtering** — `EnumWindows` alone reports a lot of junk, so `is_capturable` rejects:

- non-visible windows (`IsWindowVisible`)
- child windows, via `GetAncestor(hwnd, GA_ROOT) != hwnd`
- tool windows (`WS_EX_TOOLWINDOW`) — tray helpers and palettes
- our own process, compared through `GetWindowThreadProcessId` against `GetCurrentProcessId`, so the toolbar can't target itself
- DWM-cloaked windows via `DwmGetWindowAttribute(DWMWA_CLOAKED)` — these are the invisible UWP `ApplicationFrameHost` shells that otherwise pad the list with unusable entries

Results are sorted case-insensitively by title. `WindowInfo` now also carries `width`/`height` from `GetWindowRect` and a `minimized` flag from `IsIconic`, shown as a dimmed `1920×1080` (or "minimised") on the right of each row.

**Panel contents:** a count header with a Refresh button, a scrollable list where the selected entry is highlighted blue, an empty state when nothing matches, and a Clear selection button that appears only once something is picked. Selecting an entry stores it and collapses the panel, and the toolbar button label switches to the chosen window's title.

Selection is stored as a raw `HWND` in `WindowInfo.hwnd`, which is what a capture backend will need. Worth noting the list is a snapshot — windows that close after enumeration will leave a stale handle, so the capture path should validate with `IsWindow` before use.

---

**User:**

I'd like to change labels to Simplified Chinese. Additionally, I only want it list window from following processes: MapleStory.exe, MapleStoryT.exe, MapleStoryN.exe, Maplestory_Classic.exe, MapleStoryTA.exe

---

**Assistant:**

Two changes. Let me verify the process-name lookup API first:

Both confirmed. Let me update the window picker with the process allowlist:

Now the enumeration callback and the process filter:

Now remove the own-process check from `is_capturable`, since the allowlist supersedes it and `GetCurrentProcessId` is no longer imported:

Now the remaining UI labels in the toolbar:

Let me do a clean rebuild to make sure warnings aren't being suppressed by incremental compilation:

Builds clean. One thing I want to improve: the `process` field is captured but not surfaced, and since several MapleStory variants can run at once with similar titles, showing which executable owns each window is genuinely useful. Let me display it:

Builds clean, no warnings.

## Simplified Chinese labels

| Before | After |
|---|---|
| Select Window | 选择窗口 |
| Record / Stop | 录制 / 停止 |
| `N windows` | `N 个窗口` |
| Refresh | 刷新 |
| No capturable windows found | 未找到冒险岛窗口 |
| Clear selection | 清除选择 |
| minimised | 已最小化 |

Microsoft YaHei UI was already the default font, so all of these render without fallback. The toolbar button also got `.wrapping(text::Wrapping::None)` — at 100px wide a selected window's title would otherwise wrap to a second line and break the 32px button height.

I left the window title as "CMS Video Recorder" since it's the product name, not a UI label. Say the word if you want that localized too.

## Process allowlist

Added to `window_picker.rs`:

```rust
pub const ALLOWED_PROCESSES: &[&str] = &[
    "maplestory.exe",
    "maplestoryt.exe",
    "maplestoryn.exe",
    "maplestory_classic.exe",
    "maplestoryta.exe",
];
```

Resolution goes through `owning_process_name`: `GetWindowThreadProcessId` for the PID, then `OpenProcess` + `QueryFullProcessImageNameW`, taking only the filename component. Matching is `eq_ignore_ascii_case`, so the real-world casing (`MapleStoryT.exe`) matches regardless of how it appears on disk.

Two details worth flagging:

- I used `PROCESS_QUERY_LIMITED_INFORMATION` rather than `PROCESS_QUERY_INFORMATION`. The limited right is enough for the image name and succeeds against processes the full right would be denied on without elevation — which matters here, since games often run with different integrity levels. If MapleStory runs elevated and the toolbar doesn't, `OpenProcess` can still fail; the window is then skipped rather than crashing. If you find windows missing, running the toolbar as administrator is the fix.
- The handle is closed via `CloseHandle` on both the success and failure paths, before the `?` on the query result, so a failed query can't leak it.

I also dropped the separate own-process check from `is_capturable` — the allowlist strictly supersedes it.

## List rows

Since the allowlist means multiple MapleStory variants may appear together with near-identical titles, each row is now two lines: the window title, then a dimmer `MapleStoryT · 1920×1080`. The detail line lightens when the row is selected so it stays legible against the blue fill.

---

**User:**

I noticed some labels still can't display correctly. Also I'd like to filter out the window with width less than 800px.

---

**Assistant:**

Good diagnostic detail in that screenshot. The failing labels (`录制`, `2 个窗口`) are exactly the ones using `UI_FONT_SEMIBOLD` — the working ones use the default font. Microsoft YaHei UI has no Semibold face, and cosmic-text renders tofu instead of falling back. Let me confirm which weights actually exist:

Bold exists, Semibold doesn't. Let me fix the font and add the width filter:

Now the width filter:

Builds clean, no warnings.

## Font — the real cause

Your screenshot was the giveaway: `选择窗口` and `刷新` rendered fine while `录制` and `2 个窗口` came out as boxes. The working labels used the default font; the broken ones were the only two using `UI_FONT_SEMIBOLD`.

Microsoft YaHei UI ships Regular, Bold and Light faces — no Semibold. I confirmed this against your system:

```
Regular = True    Bold = True    Italic = True
```

When the requested weight has no matching face, cosmic-text fails to substitute a nearby weight for the CJK glyphs and emits tofu instead. Latin text survived because it fell back elsewhere, which is why the Latin labels never showed the problem and masked the bug until you used Chinese.

So `UI_FONT_SEMIBOLD` is now `UI_FONT_BOLD` with `Weight::Bold`, and the constant carries a comment explaining why the weight can't be changed casually.

## Width filter

`MIN_WINDOW_WIDTH: i32 = 800` in `window_picker.rs`, applied right after `GetWindowRect` so narrow windows are rejected before any allocation. I also tightened the rect handling — a failed `GetWindowRect` now skips the window rather than recording it as `0×0`, which would previously have slipped a bogus entry into the list.

This drops the `410×806` window from your screenshot and keeps the `1936×1119` one.

One consequence worth knowing about: minimised windows are now filtered out too. `GetWindowRect` on a minimised window reports a small off-screen placeholder (roughly 160px wide), not its restored size, so it falls below the threshold. That means the `已最小化` label is effectively unreachable now. I left the `minimized` field in place since the capture backend will want it, but if you'd rather minimised game windows still appear in the list, the fix is to read the restored bounds from `GetWindowPlacement` and filter on those instead — say the word and I'll wire that up.

---

**User:**

Before implementing actual video recording mechanism, I'd like to map each process to an alias name:
- maplestory.exe - 冒险岛正式服
- maplestoryt.exe, maplestoryta.exe - 冒险岛测试服
- maplestoryn.exe - 冒险岛N
- maplestory_classic.exe - 冒险岛怀旧服
And they should be displayed when selected. If possible, display the process icon on the left.

---

**Assistant:**

Two features here. Let me check the icon-extraction APIs available in `windows` 0.58:

`ExtractIconExW` wasn't found in Shell — let me locate it and confirm the GDI pieces:

Let me confirm the window-icon route instead, which stays inside the already-enabled `WindowsAndMessaging` feature:

All present, no new feature needed. Let me build the icon extraction module:

Now the alias table and icon wiring in `window_picker.rs`:

Now update the state, refresh logic, and the enumeration to use the alias table:

Now register the module and update the UI to show icons and aliases:

Clean from scratch, no warnings.

## Aliases

The allowlist and the alias table are now one structure, so a process can't be capturable without a name:

```rust
pub const PROCESS_ALIASES: &[(&str, &str)] = &[
    ("maplestory.exe",          "冒险岛正式服"),
    ("maplestoryt.exe",         "冒险岛测试服"),
    ("maplestoryta.exe",        "冒险岛测试服"),
    ("maplestoryn.exe",         "冒险岛N"),
    ("maplestory_classic.exe",  "冒险岛怀旧服"),
];
```

`alias_for()` returning `None` is what rejects a window during enumeration — one table to edit when adding a client.

The alias is the primary label everywhere: the toolbar button shows it once a window is selected, and it's the bold first line of each list row. The secondary line stays `MapleStoryTA · 1936×1119`, and that's deliberate — T and TA share the alias 冒险岛测试服, so the executable is what tells them apart.

The button width changed from fixed `100.0` to `Shrink`. Fixed 100px fit "选择窗口" but not a 6-character alias plus a 16px icon (~120px), which would have clipped. Shrink stays compact when nothing is selected and grows only when needed.

## Icons

New `src/icon.rs` decodes icons through GDI. A few choices worth calling out:

**Source** — I take the icon from the *window* (`WM_GETICON`, falling back to `GCLP_HICON`/`GCLP_HICONSM`) rather than from the exe file. `ExtractIconExW` would have needed the `Win32_UI_Shell` feature, and the window icon is what the window actually displays. `WM_GETICON` goes through `SendMessageTimeoutW` with `SMTO_ABORTIFHUNG` and a 120ms cap, so a frozen game can't hang the UI thread.

**Ownership** — icons from `WM_GETICON`/`GCLP_HICON` belong to the window, so they are *not* destroyed. The `HBITMAP`s that `GetIconInfo` hands back *are* ours, and they're released on every exit path via a `cleanup` closure.

**Pixels** — `GetDIBits` with `biHeight` negative for a top-down DIB, then an in-place `swap(0, 2)` to convert GDI's BGRA to the RGBA iced wants. Some 32-bit icons ship an all-zero alpha channel and would render invisible, so if no pixel has non-zero alpha the whole channel is forced opaque.

**Caching** — decoding happens once per distinct executable per refresh, in `attach_icons`, not inside the `EnumWindows` callback and never in `view`. `image::Handle` is reference-counted, so the per-frame clone is just an `Arc` bump.

`refresh()` also now reconciles the current selection: it's re-cloned from the fresh list so its icon and size update, or cleared if the window has closed.

One structural note: `WindowInfo` no longer derives `PartialEq`. Identity is the `hwnd`, which is what the selection check compares, and an image handle has no business participating in equality. The `title` field is still populated and used for sorting; it's just not displayed anymore, since it's near-identical across clients while the alias is what you actually recognize.

---

**User:**

How about making the record button unclickable until a process is selected?

---

**Assistant:**

`Status::Disabled` exists but `on_press_maybe` didn't match. Let me check the exact name:

`on_press_maybe` confirmed. Let me wire it up:

Now the disabled visual state in the styles, plus a guard so recording can't outlive its target:

Now the state guards so recording can't continue without a valid target:

Clean build, no warnings. Good idea — it removes a state where pressing 录制 would have had nothing to record.

**Disabling.** `on_press_maybe(can_record.then_some(Message::ToggleRecording))` — passing `None` is what puts iced's button into `Status::Disabled`, so it stops accepting clicks and hover feedback rather than just looking inert.

**Visuals.** `record_idle_style` returns early for `Disabled` with a muted grey fill and faded label, and the record dot takes an `enabled` flag so it desaturates to a dull red instead of staying bright against a grey button. I added a shared `disabled_style(radius)` helper so other buttons can reuse the same treatment consistently later.

**State guards.** Disabling the button in the view isn't sufficient on its own, because selection can disappear underneath an active recording. So the invariant "recording requires a target" is enforced in `update` too:

- `ToggleRecording` returns early if nothing is selected — defensive, since a hotkey or tray action could route here later without going through the button
- `ClearSelection` stops recording
- `RefreshWindows` and `ToggleWindowPicker` call `stop_recording_without_target()`, which matters because `refresh()` drops windows that have closed — so if the game exits mid-recording, recording stops rather than pointing at a dead `HWND`
- Selecting a *different* window stops recording first, so the capture target can't silently change mid-session

That last group is the part that'll matter once the capture backend is real: it guarantees `is_recording == true` always implies a live selection, which is the precondition the encoder will need.

---
