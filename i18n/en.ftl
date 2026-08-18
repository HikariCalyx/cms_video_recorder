# Toolbar
btn-record = Record
btn-stop = Stop { $clock }
btn-preparing = Preparing…
btn-saving = Saving…

# Window picker
picker-choose-window = Select window
picker-window-count = { $count } windows
picker-show-others = Show other windows
picker-no-windows = No capturable windows found
picker-no-maplestory = No MapleStory window in the foreground
picker-minimized = minimized
picker-clear-selection = Clear selection
btn-refresh = Refresh

# Settings panel
panel-settings = Settings
btn-done = Done
field-output-dir = Save location
field-max-duration = Maximum recording time
field-record-hotkey = Record / Stop shortcut
field-language = Language
btn-browse = Browse…
hint-duration-capped = seconds, then stops automatically
hint-duration-unlimited = seconds; 0 means no limit
hint-hotkey-capturing = Press the new combination; Esc cancels, Backspace clears
hint-hotkey-global = Works globally, even while the game is in the foreground
btn-change = Change
btn-clear = Clear
btn-cancel = Cancel
hotkey-waiting = Waiting for keys…
hotkey-unset = Not set
label-config-file = Config file: { $path }

# Recordings manager
videos-count = { $count } videos
videos-empty = No recordings yet
btn-play = Play
btn-compress = Compress
btn-show-folder = Folder
btn-delete = Delete
confirm-delete = Delete this clip?
status-compressing = Compressing…
status-saved = Saved { $name }
status-deleted = Deleted { $name }

# Status messages
status-pick-window = Select a window first
status-stop-before-compress = Stop recording before compressing
status-already-compressing = A clip is already being compressed
status-compress-cancelled = Compression cancelled
status-compressed = Compressed to { $size }
status-compressed-partial = Compressed to { $size } (5000 KB target not met)
word-hotkey = shortcut
word-unknown-error = unknown error

# Native dialogs
dialog-pick-folder = Choose save location
dialog-save-compressed = Save compressed video

# Trim dialog
panel-trim = Trim clip
trim-start = Start { $time }
trim-end = End { $time }
trim-loading = Reading duration…
trim-ready = Drag the handles to pick a segment, then preview it
trim-no-duration = Could not read the clip duration
trim-preview-error = Preview failed: { $error }
btn-preview = Preview
btn-stop-preview = Stop

# MapleStory server aliases
alias-live = MapleStory
alias-test = MapleStory Test
alias-m = MapleStory M
alias-n = MapleStory N
alias-classic = MapleStory Classic
alias-worlds = MapleStory Worlds

# Errors
error-no-appdata = Could not locate APPDATA
error-io = File write failed: { $msg }
error-encoder = Encoding failed: { $msg }
error-capture = Capture failed: { $msg }
error-audio = Audio capture failed: { $msg }
error-unsupported-platform = Not supported on this platform
error-unsupported-recording = Recording is not supported on this platform
error-record-thread = Recording thread ended unexpectedly
error-settings-save = Settings not saved: { $error }
error-play = Playback failed: { $error }
error-open = Open failed: { $error }
error-delete = Delete failed: { $error }
error-compress = Compression failed: { $error }
error-compress-thread = Compression thread ended unexpectedly
error-ffmpeg-missing = ffmpeg.exe not found; make sure it sits next to the program
error-hotkey-taken = { $combination } is already in use
error-copy-file = Copy failed: { $error }
error-read-result = Reading compression result failed: { $error }
error-replace-file = Replacing the original file failed: { $error }
error-ffmpeg-start = Could not start ffmpeg: { $error }
error-ffmpeg-wait = Waiting for ffmpeg failed: { $error }
error-ffmpeg-encode = ffmpeg encoding failed: { $error }
error-compress-state = Compression state corrupted
error-shellexecute = ShellExecute returned { $code }
error-target-closed = Target window has closed
error-target-size = Target window has an invalid size
error-audio-thread = Audio thread ended unexpectedly
error-com-init = COM initialisation failed
error-enum-devices = Enumerating audio devices failed
error-default-device = No default playback device found
error-activate-client = Activating the audio client failed
error-mix-format = Reading the audio format failed
error-init-capture = Initialising audio capture failed
error-capture-interface = Getting the audio capture interface failed
error-start-capture = Starting audio capture failed
error-read-packet = Reading audio packet failed: { $error }
error-get-buffer = Getting audio buffer failed: { $error }
error-release-buffer = Releasing audio buffer failed: { $error }
error-audio-format = Invalid audio format
error-float-bit-depth = Unsupported float bit depth: { $bytes }
