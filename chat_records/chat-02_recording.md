# Now let's implement actual record mechanism. (checkpoint)

Exported: 8/17/2026, 9:54:27 PM

---

**User:**

Now let's implement actual record mechanism.
I want to limit maximum recording time to 30 seconds, so the stop button would display 30 seconds countdown.
Record result should be MP4 format, by default it should be stored to Video/cms_video_recorder directory - I want it being configurable later.

---

**Assistant:**

I'll start by understanding the existing codebase.

Now let me check the recording approach options. I'll look at what's available for Windows window capture in Rust.

`Window::from_raw_hwnd` exists. Now let me check the handler trait, capture control, and encoder settings API.

Now I'll wire up the dependency features and write the recorder module.

Now the recorder module:

`iced::futures` is re-exported. Now let me update the toolbar.

Now the subscription and recording lifecycle helpers:

Now the update handler:

Now the view — record button with countdown, plus the status readout:

Now the status text helper and the truncation utility:

Now register the module, add the Win32 feature for `GetLocalTime`, and handle the disabled state on the active-record style:

Builds clean. Let me remove the two dead-code paths:

Now let me verify the pipeline actually produces a playable MP4 with a temporary smoke test:

Capture → encode → finalise works. Now let me verify the auto-stop cap path:

Both paths verified. Let me check one edge case in the backend before cleaning up:

Now clean up the temporary test and do a final verification:

Recording is wired up end to end. Here's what changed.

**New `src/recorder.rs` + `src/recorder/backend.rs`**

Capture goes through Windows.Graphics.Capture and encodes via the Media Foundation transcoder that [`windows-capture`](https://github.com/NiiightmareXD/windows-capture) wraps. Frames stay on the GPU from the compositor to the encoder, so recording a 1080p game window costs very little. Output is H.264 in an MP4 container (the crate defaults to HEVC, which needs a codec install to play back — H.264 plays everywhere, which matters for clips that get shared).

Capture runs on its own thread. `Recording::start` returns as soon as that thread is up; `Recording::finish` stops it and writes the MP4 index, which is the step that makes the file playable.

The encoder is built on the *first* frame rather than at startup, using `frame.width()`/`height()`. That sidesteps guessing the capture size from the window rect, which is off by the invisible resize border and gets rounded by the compositor.

**Files land in `%USERPROFILE%\Videos\cms_video_recorder`**, named like `冒险岛正式服_20260817-203015.mp4`. `RecorderConfig` holds the output directory, duration cap, frame rate and bitrate (12 Mbps default); it's built from `Default` at startup and lives on `Toolbar`, so a settings panel can swap it out later without touching capture code.

**Toolbar**

`is_recording: bool` became `recording: Option<Recording>`. The Stop button now reads `停止 30s` and counts down, zero-padded so the label doesn't twitch crossing ten seconds. A 100 ms tick subscription drives it, and only exists while recording or while a status message is up — an idle toolbar still doesn't redraw.

The 30 second cap is enforced on both sides. The UI stop is what the user sees; the encoder also closes the file on the capture thread when the cap hits, so a recording can't run long even if the UI is busy. Finalising is idempotent, so whichever fires first wins.

Stopping shows `保存中` and is disabled while the encoder flushes. That work happens on a plain thread reporting back through a one-shot channel, so a multi-megabyte flush doesn't stall the UI. The result surfaces as a short `已保存 <file>` or a red error in the toolbar's drag area, clearing after six seconds.

The existing "cancel recording on target change" paths now actually stop and save rather than just flipping a flag. Each tick also checks the target window still exists, so closing the game mid-recording ends the countdown instead of letting it run against an already-closed file.

**Verification:** I ran a temporary end-to-end test that captured a real window, confirmed a 4.8 MB MP4 with a valid `ftyp` box came out, and confirmed the cap path also produces a valid file when the handler finalises on its own. Both passed; I removed the test afterward since it depends on a live desktop window. Release build is clean — the two remaining clippy warnings are pre-existing ones in `icon.rs` and `window_picker.rs`.

One note on dependencies: `windows-capture` brings its own newer `windows` crate (0.62) alongy your existing 0.58. They coexist fine because HWNDs cross the boundary as raw pointers, so the two versions never have to agree on types. I also enabled iced's `tokio` feature — without an async backend, `iced::time::every` silently resolves to a no-op stream and the countdown would never tick.

---

**User:**

Could you exclude the window border from record result?

---

**Assistant:**

Let me look at what the frame API offers for cropping.

Let me measure the actual capture geometry before choosing a mechanism, and check how the buffer path consumes samples.

Let me probe the actual geometry to determine the exact crop, since the encoder's built-in crop only works from the top-left.

The picked window is borderless with a custom title bar. Let me survey all windows to find bordered ones:

Every window on this machine draws its own title bar inside the client area (Win11 style), so I need a classic-framed window like the game has. Let me create one:

Now I have the exact geometry: chrome inside the WGC frame is 1px left/right/bottom and 31px on top. Let me check the buffer API and whether I can verify results.

---

**User:**

I‘ve placed ffmpeg.exe to D:\user_profile\Documents\GitHub\cms_video_recorder\target\release.

---

**Assistant:**

ffmpeg gives me a way to verify this properly. Let me confirm it runs, then implement.

ffmpeg works. Now let me implement the crop:

Now wire the crop into the session:

Now a verification test that records a window with a known pattern and checks the result with ffmpeg:

Frame size is exactly the client area (1264x680, not the 1266x713 window frame) and the quadrants are in the right places — red top-left, blue bottom-left confirms the row order is correct. The colour drift is H.264 chroma subsampling. Let me fix the tolerance and also measure the cost of the crop path:

Correct crop, correct orientation, and 180 frames in 3s — full 60 fps on the crop path. Now the cost comparison:

Throughput is identical (300 vs 301 frames), so the crop isn't costing frames. One robustness gap to close: a mid-recording resize can shrink the frame below the rect fixed on the first frame.

Still correct. Cleaning up the probe and doing a final check:

Done. Recordings now contain only the client area.

**How the crop is worked out.** A WGC window capture covers the DWM *extended frame bounds* — the visible window including title bar and 1px borders, but excluding the invisible resize margin. The client area is the game viewport, so the difference between the two is exactly the chrome to strip. `client_crop` measures it as `ClientToScreen(0,0) − extended_frame_bounds.top_left` with `GetClientRect` for the size. I probed a standard `WS_OVERLAPPEDWINDOW` at 96 DPI and got a 1266x713 capture frame with a 1264x681 client area at offset (1, 31) — 31px of title bar on top, 1px on the other three sides.

There's a guard for when the reported bounds don't match the actual capture surface, which happens if the target process is being DPI-virtualised. In that case the offsets would be in the wrong coordinate space, so it falls back to the uncropped frame rather than cropping blindly.

**One tradeoff worth knowing about.** Cropping has to route frames through system memory. The encoder only accepts a GPU surface for the frame *exactly* as captured, and its internal resize path can only trim from the top-left — which would keep the title bar and shift the picture down. So cropped frames go `buffer_crop` → reused buffer → `send_frame_buffer`.

I measured what that costs by recording the same window both ways for 5 seconds: **301 frames uncropped vs 300 cropped**, so it isn't dropping frames at this resolution. Two things keep it cheap: the row-order flip Media Foundation needs for uncompressed BGRA happens during the copy out of the mapped staging texture rather than as a second pass, and the destination buffer is reused so only the first frame allocates.

Windows with no chrome to strip — borderless or fullscreen games — are detected and stay on the original zero-copy GPU path. There's also a new `exclude_window_border` flag on `RecorderConfig` (default `true`) alongside the output directory, so it's switchable when you add the settings panel.

**Verification.** I painted a test window's client area in four quadrants and recorded it, then pulled a frame back with your ffmpeg. Decoded size was 1264x680 — the client area, not the 1266x713 window frame — with red top-left and blue bottom-left, which pins down both the crop origin and the row order. Had the rows come out flipped, those two would have swapped. Colours land a few counts off pure saturation (232,0,1 for red) from H.264 chroma subsampling, so the check compares loosely.

I also closed a gap I found while testing: resizing the window mid-recording can shrink the capture surface below the rect fixed on the first frame. Those frames now fall back to the uncropped path and get padded rather than reading out of bounds — the picture shifts, but the recording survives.

Release build is clean; the two remaining clippy warnings are the pre-existing ones in `icon.rs` and `window_picker.rs`. The temporary probe is removed. Your `ffmpeg.exe` is still in `target/release` — worth moving somewhere outside the build directory, since `cargo clean` would take it with it.

---

**User:**

Could you resize the video to half of the size if the captured game window size is larger than 1920x1200? For example, 2560x1440 would be resized to 1280x720.

---

**Assistant:**

On it.

---

**User:**

For now there's no resolution higher than 3840x2160 available for the game.
These are possible resolutions:
1024x768, 1280x720, 1366x768, 1920x1080, 1920x1200, 2560x1440, 2560x1600, 2732x1536, 3840x2160.

---

**Assistant:**

The crate ties encoder input size to output size, so Media Foundation won't scale for us — I'll have to resample. Since cropping already copies through system memory, I can fuse the crop, the row flip, and a 2×2 box average into the same single pass. Let me restructure:

Now the fused repack pass:

Now update the session to use the layout:

Now the config option:

Now let me verify the halving math and the end-to-end result. Since `plan_layout` is private, the probe goes under `backend`:

All nine resolutions map correctly. Now the end-to-end check with an oversized window:

60 fps sustained while averaging a 2584x1461 source down every frame. Let me add a regression check that below-threshold windows still record natively, since I restructured `Crop` into `Layout`:

All four pass. Cleaning up the probe and doing a final build:

Done. Captures larger than 1920x1200 now record at half size.

**Why it needed resampling rather than a setting.** The encoder ties its input stream descriptor to the same width and height as its output profile, so Media Foundation never sees a size change to scale across. Asking for a smaller output just makes its resize path trim from the top-left. So the downscale had to happen in our own pass.

That turned out cheap, because cropping already copies frames through system memory. The crop, the bottom-up row flip, and a 2×2 box average now all happen in one pass over the mapped staging texture, so the scaling rides along with a copy that was already being made. It also cuts the work downstream by 4x, since everything after the resample handles a quarter of the pixels.

The source region is sized to exactly twice the output, so every output pixel is a full 2×2 average with no partial block left over. All nine of your resolutions divide evenly, so nothing gets clipped:

| Capture | Records at |
|---|---|
| 1024x768, 1280x720, 1366x768, 1920x1080, 1920x1200 | native |
| 2560x1440 | 1280x720 |
| 2560x1600 | 1280x800 |
| 2732x1536 | 1366x768 |
| 3840x2160 | 1920x1080 |

The threshold is `max_native_size` on `RecorderConfig`, defaulting to `Some((1920, 1200))`. Setting it to `None` records everything natively. It sits alongside `output_dir` and `exclude_window_border` for the settings panel later.

One knock-on benefit: since output now tops out at 1920x1200, the fixed 12 Mbps bitrate is appropriate across every mode. Previously a native 4K recording would have looked starved at that rate.

**Verification.** The layout table covers all nine resolutions plus the `None` case, and asserts halved layouts consume exactly double what they emit. End to end, a 2584x1461 client area recorded at 1292x730 with the quadrant colours in the right corners — which confirms the crop origin, the row order, and the averaging together, since a flip would swap red and blue. That held **60 fps with no dropped frames** (239 frames in 4s) while averaging a 3.8-megapixel source every frame. I also re-checked the below-threshold path after the refactor: a 1264x681 client still records at 1264x680 with the title bar gone.

Release build is clean; the two remaining clippy warnings are the pre-existing ones in `icon.rs` and `window_picker.rs`. Probe files removed.

One caveat I should flag: I verified against a synthetic `WS_OVERLAPPEDWINDOW`, not MapleStory itself. The geometry maths is measured rather than assumed, but it's worth a quick sanity recording against the real game window to confirm the client rect lands where expected there too.

---

**User:**

I found one more problem. in recorded result, the first second would always freeze, then rest of frames work properly.
plus, could you include audio that's currently playing?

---

**Assistant:**

Two separate issues. Let me first check the current project state, since there's a new `app.rc`.

Release profile now uses `opt-level = 'z'`, which I'll need to re-check against the resample loop. First, let me diagnose the freeze:

Good — that rules out my first hypothesis: the encoded timeline is perfectly even (largest gap 0.017s, one frame interval). But `VideoEncoder::new` costs ~370-520ms on the capture thread, which means frames queue up during startup. With a 1-buffer WGC frame pool, queued *surface references* would all alias the same recycled texture. Let me test frame content rather than timing:

Confirmed, and it's not surface aliasing — the *repacked* path freezes too (only 8 distinct images in the first 60 frames, a 25-frame identical run). So the source frames are genuinely identical: the ~400ms encoder setup runs inside `on_frame_arrived` for the first frame, stalling frame delivery, and Media Foundation pads the resulting timeline gap with duplicates. That's why the PTS looked even.

The fix is to build the encoder before capture starts, which needs the frame size up front:

Root cause identified. Now let me build the encoder before capture starts, which requires knowing the frame size up front:

Now restructure the session to build the encoder in `new`:

Now add the target accessor and make the UI start asynchronously:

Now the update arms and the button states:

Compiles. Now let me verify the freeze is gone:

Improved at the head but duplicates remain. I need to know whether that's my test window's paint rate or the recorder — let me measure distinctness per second and the window's actual paint rate:

Source paints at 20k fps yet only 5-24 distinct frames per second land — and that's uniform across all seconds, not just the first. I need ground truth on what WGC actually delivers here, independent of the encoder's constant-frame-rate padding:

---

**User:**

Could you just add a latency before actual recording begins?

---

**Assistant:**

Right — simpler and it targets the actual symptom. Let me add a warm-up window where frames are discarded while the pipeline settles.

Now hold `Recording::start` through the warm-up so the countdown matches what's actually in the file:

Second 0 is no longer the worst (repacked: 18/60 vs 16, 13, 19 after) — the head freeze is fixed. But notice passthrough now shows 28-29 frame runs *throughout*, while repacked never exceeds 18. That's the 1-buffer frame pool aliasing I suspected. Let me test it decisively by always repacking:

---

**User:**

I think this is good to go. Could you check audio recording part?

---

**Assistant:**

Let me check the encoder's audio API before designing this.

Now for audio. Let me confirm the WASAPI symbols available in `windows` 0.58.

All the WASAPI pieces are available. Let me add the features and write the loopback capture module.

Now wire it into the recorder config, error type, and module tree:

Now hook audio into the capture session:

Now start audio in `Recording::start` and pump it from the frame handler:

Now the session side — arm audio with the first kept frame and feed it alongside video:

Compiles. Now let me verify audio actually lands in the file, in sync. I'll play a known tone through the default device while recording:

MP4 now has a proper AAC stereo track with duration matching video exactly (6.00s). But it's pure silence — I need to check whether the tone actually played on this machine.

ffmpeg on this box has no SDL output device, so the tone never played — my fallback never triggered because `spawn()` succeeded. Let me fix playback and test the WASAPI layer directly to isolate it:

WASAPI layer confirmed working: 48kHz stereo, silence before playback, **-12.1 dBFS during playback**, and byte counts exactly match wall clock (134400 bytes = 0.7s, 288000 = 1.5s), so the pacing is precise. Now the end-to-end test:

End-to-end audio works — the RMS pattern tracks the tone file's 2s-on/1s-off structure cleanly. Let me check for drift over a longer recording, since audio and video use different clocks:

Drift over 15 seconds is +31ms — well under one perceptible threshold. Cleaning up:

Audio recording works. Here's what went in.

**Capture path.** A new `recorder/audio.rs` opens the default playback endpoint in WASAPI loopback mode (`eRender` + `AUDCLNT_STREAMFLAGS_LOOPBACK`), so it records the system mix — game sound plus anything else playing. The endpoint is opened on the capture thread so every COM call stays in the apartment that thread initialises, and the format is reported back because the encoder's audio stream has to be built to match it.

Samples are converted to 16-bit stereo PCM. Shared-mode mix formats are essentially always 32-bit float, but integer widths and mono endpoints are handled too. Surround endpoints get their front pair taken rather than mixed — a proper downmix needs per-layout coefficients to not sound wrong.

**The part that needed care.** The encoder times audio purely by how many samples it has been handed, ignoring any timestamp you pass. That makes a continuous stream mandatory, and loopback works against you here: an idle endpoint delivers *nothing at all* rather than silence. Left alone, audio would run progressively ahead of video. So the capture thread tops up with silence to keep the sample count tracking wall clock, only filling gaps bigger than 10ms so normal jitter isn't patched.

Both streams are zeroed at the same instant: the session arms audio when it encodes its first kept frame, which is after the warm-up. Audio captured before that is discarded.

Audio is pumped from `on_frame_arrived` rather than its own thread, so the encoder is only ever touched from the capture thread and needs no extra locking. At 60fps that's a few milliseconds of sound per call. The buffer is capped at 5 seconds in case frame delivery stops and nothing drains it.

**Verification.** Three checks, all with your ffmpeg:

- WASAPI layer alone: reported 48kHz stereo, read silence with nothing playing, then **-12.1 dBFS** with a tone playing. Byte counts matched wall clock exactly (134400 bytes = 0.700s, 288000 = 1.500s), so the pacing is accurate rather than approximately right.
- End to end: MP4 carries an AAC-LC 48kHz stereo track alongside the H.264 video. Per-second RMS traced the test tone's 2s-on/1s-off structure cleanly and repeated on period without slipping.
- Drift over 15 seconds: video 14.97s, audio 15.00s, **+31ms**. Audio and video run on different clocks (sample count vs the compositor's QPC), so this was the one worth measuring.

`capture_audio` on `RecorderConfig` defaults to `true`, alongside the other settings for the panel later.

Two things worth flagging:

If the endpoint can't be opened, the recording continues **silently** as video-only. That's deliberate — losing sound beats losing the clip — but right now nothing tells you it happened. Surfacing it as a toolbar warning is easy to add if you want it; say the word.

Also, this captures the whole system mix, so Discord, music and notifications land in the clip. If you'd rather have only the game's audio, Windows 10 2004+ supports per-process loopback via `ActivateAudioInterfaceAsync` with `VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK`. It's more involved (a COM completion-handler callback) but it would key off the PID you already resolve in `window_picker.rs`.

I also removed the now-dead `is_passithrough` helper left over from the aliasing fix. Release build is clean; the two remaining clippy warnings are the pre-existing ones in `icon.rs` and `window_picker.rs`.

---
