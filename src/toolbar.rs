// toolbar.rs – main floating toolbar state and view

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use iced::{
    font::{Family, Weight},
    futures::channel::oneshot,
    keyboard,
    widget::{
        button, checkbox, column, container, horizontal_space, image, mouse_area, pick_list, row,
        scrollable, text, text_input,
    },
    window, Alignment, Background, Border, Color, Element, Font, Length, Shadow, Size, Subscription,
    Task, Theme,
};

use crate::config::AppConfig;
use crate::i18n::{tr, tr_arg, Language};
use crate::recorder::{self, Recording};
use crate::settings::{self, Capture, SettingsState};
use crate::videos::VideosState;
use crate::window_picker::{WindowInfo, WindowPickerState};
use iced::widget::scrollable::{Direction, Scrollbar};

/// Default UI font – Microsoft YaHei UI ships with Windows and covers both
/// Latin and CJK, so labels and window titles render without tofu boxes.
pub const UI_FONT: Font = Font {
    family: Family::Name("Microsoft YaHei UI"),
    ..Font::DEFAULT
};

/// Bold variant for emphasised labels.
///
/// Must be `Bold`, not `Semibold`: Microsoft YaHei UI ships Regular, Bold and
/// Light faces only, and requesting an absent weight makes the shaper emit
/// tofu boxes for CJK glyphs instead of falling back to the nearest weight.
pub const UI_FONT_BOLD: Font = Font {
    family: Family::Name("Microsoft YaHei UI"),
    weight: Weight::Bold,
    ..Font::DEFAULT
};

/// Toolbar background. Also used as the app clear colour so no stray pixels
/// show along the clipped edge.
pub fn pill_bg() -> Color {
    Color::from_rgb8(0x1C, 0x1C, 0x21)
}

/// Border radius of the pill, matched to the Win32 clip region.
#[cfg(windows)]
const PILL_RADIUS: f32 = crate::win32::CORNER_RADIUS;
#[cfg(not(windows))]
const PILL_RADIUS: f32 = 12.0;

// --- Window geometry -------------------------------------------------------

pub const WINDOW_WIDTH: f32 = 460.0;
/// Height of just the toolbar strip
pub const BAR_HEIGHT: f32 = 52.0;
/// Height when the picker panel is expanded
pub const EXPANDED_HEIGHT: f32 = 320.0;
/// Height when the settings panel is expanded.
///
/// Sized to the form's natural height – the panel doesn't scroll, so anything
/// shorter squeezes the fields.
pub const SETTINGS_HEIGHT: f32 = 300.0;
/// Height when the recordings manager is expanded.
pub const VIDEOS_HEIGHT: f32 = 320.0;

// --- Timing ----------------------------------------------------------------

/// How often the countdown / status clock is polled while it matters.
///
/// Fine-grained enough that the displayed second flips promptly, and the
/// subscription only exists while recording or while a status is on screen.
const TICK_INTERVAL: Duration = Duration::from_millis(100);

/// How long a "saved" or "failed" message stays on the toolbar.
const STATUS_TTL: Duration = Duration::from_secs(6);

/// Cap on the status text so a long error can't push the buttons around.
const STATUS_MAX_CHARS: usize = 22;

/// A `button::style` callback, as selected at runtime.
type ButtonStyleFn = fn(&Theme, button::Status) -> button::Style;

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Message {
    /// The main window finished opening – time to apply Win32 tweaks
    WindowOpened(window::Id),
    /// The window was resized – the clip region has to be rebuilt
    WindowResized,
    /// Begin an OS-driven window drag
    StartDrag,
    /// Expand / collapse the window picker panel
    ToggleWindowPicker,
    /// User picked a specific window from the list
    WindowSelected(WindowInfo),
    /// Re-enumerate the available windows
    RefreshWindows,
    /// Toggle whether non-MapleStory windows are listed as well
    ToggleOtherWindows(bool),
    /// Forget the current selection
    ClearSelection,
    /// Toggle recording on / off
    ToggleRecording,
    /// The encoder is up and capture has begun (or failed to)
    RecordingStarted(Result<(), recorder::Error>),
    /// Countdown / status-expiry clock
    Tick,
    /// The encoder finished writing the file (or failed trying)
    RecordingFinished(Result<PathBuf, recorder::Error>),
    /// Expand / collapse the settings panel
    ToggleSettings,
    /// Expand / collapse the recordings manager
    ToggleVideos,
    /// Re-scan the output directory for new clips
    RefreshVideos,
    /// Open a clip in the default player
    PlayVideo(PathBuf),
    /// Compress a clip with the bundled ffmpeg
    CompressVideo(PathBuf),
    /// The save dialog closed; `Some` is where the compressed file should go
    CompressTargetPicked(Option<PathBuf>),
    /// The ffmpeg pass finished (or failed, or was cancelled)
    CompressionFinished(crate::videos::CompressionReport),
    /// Cancel the running compression
    CancelCompression,
    /// Reveal a clip in an Explorer window
    BrowseVideo(PathBuf),
    /// Delete a clip from disk
    DeleteVideo(PathBuf),
    /// The user confirmed the pending deletion
    ConfirmDeleteVideo,
    /// The user backed out of the pending deletion
    CancelDeleteVideo,
    /// The output directory field was edited
    OutputDirChanged(String),
    /// The duration-cap field was edited
    MaxDurationChanged(String),
    /// Open the folder picker
    BrowseOutputDir,
    /// The folder picker closed, with a folder or without one
    OutputDirPicked(Option<PathBuf>),
    /// Start reading the next key press as the new shortcut
    CaptureHotkey,
    /// A key press arrived while the shortcut was being read
    HotkeyCaptured(keyboard::Key, keyboard::Modifiers),
    /// Unassign the shortcut
    ClearHotkey,
    /// The global shortcut fired, or couldn't be registered
    Hotkey(crate::hotkey::Event),
    /// Write the edited settings to disk
    SaveSettings,
    /// The display language was chosen in the settings panel
    SelectLanguage(Language),
    /// Close the application
    Close,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Transient outcome of the last recording, shown on the toolbar.
#[derive(Debug, Clone)]
enum Status {
    /// A recording finished and its file is ready.
    Saved(PathBuf),
    /// A clip was deleted from disk.
    Deleted(String),
    /// A clip was compressed in place.
    Compressed(String),
    /// A neutral message, e.g. from a placeholder action.
    Info(String),
    Failed(String),
}

#[derive(Default)]
pub struct Toolbar {
    pub picker: WindowPickerState,
    pub window_id: Option<window::Id>,
    /// Persisted preferences: output directory, duration cap and shortcut.
    pub config: AppConfig,
    /// The settings panel and its edit buffers.
    pub settings: SettingsState,
    /// The recordings manager and its list of clips.
    pub videos: VideosState,
    /// The live capture session, if any.
    recording: Option<Recording>,
    /// True while the encoder is spinning up, before capture begins.
    starting: bool,
    /// Hand-off slot for the session being started on a background thread.
    ///
    /// A `Recording` can't travel inside a `Message`, which has to stay
    /// `Clone`, so the worker parks it here and the message just signals that
    /// it's ready.
    pending: Arc<Mutex<Option<Recording>>>,
    /// True between the stop request and the encoder finishing the file.
    stopping: bool,
    /// Last outcome plus when it was recorded, so it can expire.
    status: Option<(Status, Instant)>,
}

impl Toolbar {
    /// Builds the initial state from the config file.
    pub fn new() -> (Self, Task<Message>) {
        let config = AppConfig::load();
        crate::i18n::init(config.language);

        let mut toolbar = Self {
            config,
            ..Self::default()
        };
        toolbar.settings.reset(&toolbar.config);

        // The listener thread reads this when the subscription first builds it,
        // so the shortcut is live before the window even appears.
        crate::hotkey::apply(toolbar.config.hotkey);

        (toolbar, Task::none())
    }

    // -----------------------------------------------------------------------
    // Subscription
    // -----------------------------------------------------------------------
    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            window::open_events().map(Message::WindowOpened),
            // Reapplying the clip region on resize is more reliable than
            // chaining it after the resize task, which can race the OS.
            window::resize_events().map(|_| Message::WindowResized),
            // The global shortcut, which has to work while the game has focus.
            Subscription::run(crate::hotkey::listen).map(Message::Hotkey),
        ];

        // Only run the clock when something on screen depends on it, so an
        // idle toolbar doesn't redraw at all.
        if self.recording.is_some() || self.status.is_some() {
            subscriptions.push(iced::time::every(TICK_INTERVAL).map(|_| Message::Tick));
        }

        // Reading the next key press only makes sense while the hotkey field is
        // armed, and this must not steal keys from the text inputs otherwise.
        if self.settings.capturing {
            subscriptions.push(keyboard::on_key_press(|key, modifiers| {
                Some(Message::HotkeyCaptured(key, modifiers))
            }));
        }

        Subscription::batch(subscriptions)
    }

    /// Resize the window to match the current expanded/collapsed state
    fn sync_window_size(&self) -> Task<Message> {
        let Some(id) = self.window_id else {
            return Task::none();
        };

        let height = if self.picker.is_open {
            EXPANDED_HEIGHT
        } else if self.settings.is_open {
            SETTINGS_HEIGHT
        } else if self.videos.is_open {
            VIDEOS_HEIGHT
        } else {
            BAR_HEIGHT
        };

        window::resize(id, Size::new(WINDOW_WIDTH, height))
    }

    /// Whether a capture session is currently running.
    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    /// Recording can't continue without a target, e.g. after the selected
    /// window closes and a refresh drops it from the list. Whatever was
    /// captured up to that point is still saved.
    fn stop_recording_without_target(&mut self) -> Task<Message> {
        if self.picker.selected.is_none() {
            return self.stop_recording();
        }
        Task::none()
    }

    /// The clock on the Stop button: time left when there's a cap, time spent
    /// when there isn't.
    ///
    /// The countdown rounds up, so it reads the full budget the instant
    /// recording starts and only reaches zero once the time is really gone.
    fn recording_clock(&self) -> String {
        let Some(recording) = &self.recording else {
            return clock(0);
        };

        let elapsed = recording.elapsed();

        match self.config.max_duration() {
            Some(max) => clock(max.saturating_sub(elapsed).as_millis().div_ceil(1000) as u64),
            None => clock(elapsed.as_secs()),
        }
    }

    fn set_status(&mut self, status: Status) {
        // The toolbar can only hold a one-line, truncated summary, so debug
        // builds also print the full message to the console.
        #[cfg(debug_assertions)]
        if let Status::Failed(reason) = &status {
            eprintln!("[error] {reason}");
        }

        self.status = Some((status, Instant::now()));
    }

    /// Opens a capture session on the selected window.
    ///
    /// Media Foundation takes a few hundred milliseconds to spin up, and that
    /// happens before the first frame is captured so the recording doesn't open
    /// on a run of duplicate frames. Running it on a worker keeps the toolbar
    /// responsive through the wait.
    fn start_recording(&mut self) -> Task<Message> {
        let Some(target) = self.picker.selected.as_ref() else {
            // Reachable from the hotkey, which doesn't know the Record button
            // is disabled.
            self.set_status(Status::Failed(tr("status-pick-window")));
            return Task::none();
        };

        self.status = None;
        self.starting = true;

        let (hwnd, alias) = (target.hwnd, target.alias.clone());
        let config = self.config.recorder_config();
        let slot = self.pending.clone();

        let (sender, receiver) = oneshot::channel();
        std::thread::spawn(move || {
            let outcome = Recording::start(hwnd, &alias, &config).map(|recording| {
                if let Ok(mut slot) = slot.lock() {
                    *slot = Some(recording);
                }
            });
            let _ = sender.send(outcome);
        });

        Task::perform(
            async move {
                receiver.await.unwrap_or_else(|_| {
                    Err(recorder::Error::Capture(tr("error-record-thread")))
                })
            },
            Message::RecordingStarted,
        )
    }

    /// Stops the capture session and finalises the file.
    ///
    /// Finalising blocks until the encoder has flushed and written the MP4
    /// index, so it runs on a plain thread and reports back through a one-shot
    /// channel rather than stalling the UI.
    fn stop_recording(&mut self) -> Task<Message> {
        let Some(recording) = self.recording.take() else {
            return Task::none();
        };

        self.stopping = true;

        let (sender, receiver) = oneshot::channel();
        std::thread::spawn(move || {
            let _ = sender.send(recording.finish());
        });

        Task::perform(
            async move {
                receiver.await.unwrap_or_else(|_| {
                    Err(recorder::Error::Capture(tr("error-record-thread")))
                })
            },
            Message::RecordingFinished,
        )
    }

    /// Adopts whatever is in the settings fields, re-registers the shortcut if
    /// it changed, and writes the config file.
    ///
    /// Called when a setting is confirmed rather than on every keystroke, so a
    /// half-typed number never becomes a real limit and the file isn't
    /// rewritten per character.
    fn commit_settings(&mut self) -> Task<Message> {
        let updated = self.settings.to_config();

        // Show what was actually stored: a blank path resolves to the default
        // output directory, and a blank duration to "no limit".
        self.settings.output_dir = updated.output_dir.to_string_lossy().into_owned();
        self.settings.max_duration = updated.max_duration_secs.to_string();

        if updated == self.config {
            return Task::none();
        }

        let language_changed = updated.language != self.config.language;
        let rebind = updated.hotkey != self.config.hotkey;
        self.config = updated;

        if language_changed {
            crate::i18n::set(self.config.language);

            // Window aliases come from the active locale, so re-enumerate to
            // relabel what's already listed.
            self.picker.refresh();
        }

        if rebind {
            crate::hotkey::apply(self.config.hotkey);
        }

        if let Err(error) = self.config.save() {
            self.set_status(Status::Failed(tr_arg("error-settings-save", "error", error)));
        }

        Task::none()
    }

    /// Collapses the settings panel, committing what's in the fields.
    ///
    /// Does not resize the window – callers batch that in themselves, since
    /// they may be opening the other panel in the same update.
    fn close_settings(&mut self) -> Task<Message> {
        if !self.settings.is_open {
            return Task::none();
        }

        self.settings.is_open = false;
        self.settings.capturing = false;

        self.commit_settings()
    }

    /// Rebuild the rounded clip region for the current window size
    fn refresh_clip_region(&self) -> Task<Message> {
        #[cfg(windows)]
        {
            if let Some(id) = self.window_id {
                return window::run_with_handle(id, crate::win32::apply_window_effects).discard();
            }
        }
        Task::none()
    }

    // -----------------------------------------------------------------------
    // Update
    // -----------------------------------------------------------------------
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowOpened(id) => {
                self.window_id = Some(id);
                return self.refresh_clip_region();
            }
            Message::WindowResized => {
                return self.refresh_clip_region();
            }
            Message::StartDrag => {
                if let Some(id) = self.window_id {
                    return window::drag(id);
                }
            }
            Message::ToggleWindowPicker => {
                // toggle() refreshes on open, which may drop a closed window.
                self.picker.toggle();

                // Only one panel fits in the window at a time.
                if self.picker.is_open {
                    self.videos.is_open = false;
                    self.videos.pending_delete = None;
                }
                let settings = if self.picker.is_open {
                    self.close_settings()
                } else {
                    Task::none()
                };

                return Task::batch([
                    settings,
                    self.stop_recording_without_target(),
                    self.sync_window_size(),
                ]);
            }
            Message::WindowSelected(info) => {
                let is_new_target =
                    self.picker.selected.as_ref().map(|s| s.hwnd) != Some(info.hwnd);
                self.picker.select(info);

                let stop = if is_new_target {
                    // Switching targets mid-recording would silently change
                    // what's being captured, so close out the current clip.
                    self.stop_recording()
                } else {
                    Task::none()
                };

                return Task::batch([stop, self.sync_window_size()]);
            }
            Message::RefreshWindows => {
                self.picker.refresh();
                return self.stop_recording_without_target();
            }
            Message::ToggleOtherWindows(show) => {
                self.picker.include_others = show;
                self.picker.refresh();
                return self.stop_recording_without_target();
            }
            Message::ClearSelection => {
                self.picker.selected = None;
                return self.stop_recording();
            }
            Message::ToggleRecording => {
                // The button is disabled while the encoder starts up or
                // finishes, but a hotkey or tray action could still route here.
                if self.starting || self.stopping {
                    return Task::none();
                }

                return if self.is_recording() {
                    self.stop_recording()
                } else {
                    self.start_recording()
                };
            }
            Message::RecordingStarted(result) => {
                self.starting = false;

                if let Err(error) = result {
                    self.set_status(Status::Failed(error.to_string()));
                    return Task::none();
                }

                self.recording = self.pending.lock().ok().and_then(|mut slot| slot.take());

                // The target can be changed or cleared during the startup
                // wait, in which case this session is already recording the
                // wrong window – close it out immediately.
                let target = self.picker.selected.as_ref().map(|info| info.hwnd);
                if target != self.recording.as_ref().map(Recording::target_hwnd) {
                    return self.stop_recording();
                }
            }
            Message::Tick => {
                if self
                    .status
                    .as_ref()
                    .is_some_and(|(_, at)| at.elapsed() >= STATUS_TTL)
                {
                    self.status = None;
                }

                // The encoder enforces the same cap on its side; this is what
                // makes the UI agree with it. Losing the target window ends
                // the recording too, otherwise the countdown would keep
                // running against a file that's already closed.
                let cap = self.config.max_duration();
                if self.recording.as_ref().is_some_and(|recording| {
                    cap.is_some_and(|max| recording.elapsed() >= max)
                        || !recording.target_is_alive()
                }) {
                    return self.stop_recording();
                }
            }
            Message::RecordingFinished(result) => {
                self.stopping = false;
                self.set_status(match result {
                    Ok(path) => Status::Saved(path),
                    Err(error) => Status::Failed(error.to_string()),
                });

                // A new clip just landed; the manager may be open on it.
                if self.videos.is_open {
                    self.videos.refresh(&self.config.output_dir);
                }
            }
            Message::ToggleSettings => {
                if self.settings.is_open {
                    return Task::batch([self.close_settings(), self.sync_window_size()]);
                }

                self.settings.is_open = true;
                // Discard anything left over from a panel that was closed
                // without saving.
                self.settings.reset(&self.config);

                // Both panels share the window body.
                if self.picker.is_open {
                    self.picker.is_open = false;
                }
                self.videos.is_open = false;
                self.videos.pending_delete = None;

                return self.sync_window_size();
            }
            Message::ToggleVideos => {
                if self.videos.is_open {
                    self.videos.is_open = false;
                    self.videos.pending_delete = None;
                    return self.sync_window_size();
                }

                self.videos.is_open = true;
                self.videos.refresh(&self.config.output_dir);

                // Only one panel fits in the window at a time.
                let settings = if self.settings.is_open {
                    self.close_settings()
                } else {
                    Task::none()
                };
                if self.picker.is_open {
                    self.picker.is_open = false;
                }

                return Task::batch([settings, self.sync_window_size()]);
            }
            Message::RefreshVideos => {
                self.videos.refresh(&self.config.output_dir);
            }
            Message::PlayVideo(path) => {
                if let Err(error) = crate::videos::play(&path) {
                    self.set_status(Status::Failed(tr_arg("error-play", "error", error)));
                }
            }
            Message::CompressVideo(path) => {
                // The encoder is already busy capturing; don't stack a
                // software encode on top of a live recording.
                if self.is_recording() {
                    self.set_status(Status::Info(tr("status-stop-before-compress")));
                    return Task::none();
                }

                // One pass at a time.
                if self.videos.compressing.is_some() {
                    self.set_status(Status::Info(tr("status-already-compressing")));
                    return Task::none();
                }

                // Fail before asking for a location: if ffmpeg was deleted,
                // the user should know there's nothing to compress with.
                if crate::videos::ffmpeg_path().is_none() {
                    self.set_status(Status::Failed(tr("error-ffmpeg-missing")));
                    return Task::none();
                }

                self.videos.compressing = Some(path.clone());
                self.videos.session = Some(crate::videos::CompressionSession::default());

                // Ask where the compressed file should go. The dialog opens
                // on the clip's folder, with the name pre-filled.
                let directory = path
                    .parent()
                    .map(|parent| parent.to_path_buf())
                    .unwrap_or_default();
                let suggested = format!(
                    "{}_compressed",
                    crate::videos::display_name(&file_name(&path))
                );

                let (sender, receiver) = oneshot::channel();
                std::thread::spawn(move || {
                    let _ = sender.send(crate::dialog::pick_save_file(&directory, &suggested));
                });

                return Task::perform(
                    async move { receiver.await.unwrap_or(None) },
                    Message::CompressTargetPicked,
                );
            }
            Message::CompressTargetPicked(destination) => {
                let Some(source) = self.videos.compressing.take() else {
                    return Task::none();
                };

                // Cancelled at the dialog: drop the busy marker. If the
                // user pressed 取消 while it was up, say so.
                let Some(destination) = destination else {
                    let was_cancelled = self
                        .videos
                        .session
                        .as_ref()
                        .is_some_and(|session| session.is_cancelled());
                    self.videos.session = None;

                    if was_cancelled {
                        self.set_status(Status::Info(tr("status-compress-cancelled")));
                    }
                    return Task::none();
                };

                let Some(session) = self.videos.session.clone() else {
                    return Task::none();
                };

                // The save dialog has no owner window, so the toolbar stays
                // clickable while it is up – a cancel pressed then counts.
                if session.is_cancelled() {
                    self.videos.compressing = None;
                    self.videos.session = None;
                    self.set_status(Status::Info(tr("status-compress-cancelled")));
                    return Task::none();
                }

                // Keep the busy marker through the encode itself.
                self.videos.compressing = Some(source.clone());

                let (sender, receiver) = oneshot::channel();
                std::thread::spawn(move || {
                    let _ = sender.send(crate::videos::compress_to(
                        &source,
                        &destination,
                        &session,
                    ));
                });

                return Task::perform(
                    async move {
                        receiver.await.unwrap_or_else(|_| {
                            crate::videos::CompressionReport::Failed(tr("error-compress-thread"))
                        })
                    },
                    Message::CompressionFinished,
                );
            }
            Message::CompressionFinished(report) => {
                self.videos.compressing = None;
                self.videos.session = None;

                match report {
                    crate::videos::CompressionReport::Done(outcome) => {
                        let size = crate::videos::human_size(outcome.size);
                        let message = if outcome.reached_target {
                            tr_arg("status-compressed", "size", size)
                        } else {
                            tr_arg("status-compressed-partial", "size", size)
                        };
                        self.set_status(Status::Compressed(message));
                    }
                    crate::videos::CompressionReport::Failed(error) => {
                        self.set_status(Status::Failed(tr_arg("error-compress", "error", error)));
                    }
                    crate::videos::CompressionReport::Cancelled => {
                        self.set_status(Status::Info(tr("status-compress-cancelled")));
                    }
                }

                if self.videos.is_open {
                    self.videos.refresh(&self.config.output_dir);
                }
            }
            Message::CancelCompression => {
                // The worker (or the pending dialog result) notices the flag
                // and reports back; stay busy until it does.
                if let Some(session) = &self.videos.session {
                    session.cancel();
                }
            }
            Message::BrowseVideo(path) => {
                if let Err(error) = crate::videos::browse(&path) {
                    self.set_status(Status::Failed(tr_arg("error-open", "error", error)));
                }
            }
            Message::DeleteVideo(path) => {
                // Arm the confirmation: the row asks again before anything
                // is removed from disk.
                self.videos.pending_delete = Some(path);
            }
            Message::ConfirmDeleteVideo => {
                let Some(path) = self.videos.pending_delete.take() else {
                    return Task::none();
                };

                let name = crate::videos::display_name(&file_name(&path)).to_string();

                match crate::videos::delete(&path) {
                    Ok(()) => {
                        self.set_status(Status::Deleted(name));

                        if self.videos.is_open {
                            self.videos.refresh(&self.config.output_dir);
                        }
                    }
                    Err(error) => {
                        self.set_status(Status::Failed(tr_arg("error-delete", "error", error)));
                    }
                }
            }
            Message::CancelDeleteVideo => {
                self.videos.pending_delete = None;
            }
            Message::OutputDirChanged(value) => {
                self.settings.output_dir = value;
            }
            Message::MaxDurationChanged(value) => {
                // Digits only, so the field can never hold something that
                // silently parses to "no limit".
                self.settings.max_duration = value.chars().filter(char::is_ascii_digit).collect();
            }
            Message::BrowseOutputDir => {
                if self.settings.browsing {
                    return Task::none();
                }

                self.settings.browsing = true;

                // The shell dialog is modal and runs its own message pump, so
                // it goes on a thread of its own rather than blocking the UI.
                let start = PathBuf::from(self.settings.output_dir.trim());
                let (sender, receiver) = oneshot::channel();
                std::thread::spawn(move || {
                    let _ = sender.send(crate::dialog::pick_folder(&start));
                });

                return Task::perform(
                    async move { receiver.await.unwrap_or(None) },
                    Message::OutputDirPicked,
                );
            }
            Message::OutputDirPicked(picked) => {
                self.settings.browsing = false;

                if let Some(dir) = picked {
                    self.settings.output_dir = dir.to_string_lossy().into_owned();
                    return self.commit_settings();
                }
            }
            Message::CaptureHotkey => {
                self.settings.capturing = !self.settings.capturing;
            }
            Message::ClearHotkey => {
                self.settings.capturing = false;
                self.settings.hotkey = None;
                return self.commit_settings();
            }
            Message::HotkeyCaptured(key, modifiers) => {
                if !self.settings.capturing {
                    return Task::none();
                }

                match settings::capture(&key, modifiers) {
                    Capture::Bound(hotkey) => {
                        self.settings.capturing = false;
                        self.settings.hotkey = Some(hotkey);
                        return self.commit_settings();
                    }
                    Capture::Cleared => {
                        self.settings.capturing = false;
                        self.settings.hotkey = None;
                        return self.commit_settings();
                    }
                    Capture::Cancelled => self.settings.capturing = false,
                    // A modifier on its own, or a key that can't be bound.
                    Capture::Ignored => {}
                }
            }
            Message::SaveSettings => {
                // Closing is what commits, so this is the same path as pressing
                // the cog again – it just reads as a confirmation.
                return Task::batch([self.close_settings(), self.sync_window_size()]);
            }
            Message::SelectLanguage(language) => {
                self.settings.language = language;
                return self.commit_settings();
            }
            Message::Hotkey(event) => match event {
                crate::hotkey::Event::Pressed => {
                    return self.update(Message::ToggleRecording);
                }
                crate::hotkey::Event::Rejected => {
                    // Naming the combination is the whole point of the message:
                    // it tells the user which one to change.
                    let combination = match self.config.hotkey {
                        Some(hotkey) => hotkey.label(),
                        None => tr("word-hotkey"),
                    };
                    self.set_status(Status::Failed(tr_arg(
                        "error-hotkey-taken",
                        "combination",
                        combination,
                    )));
                }
            },
            Message::Close => {
                if let Some(id) = self.window_id {
                    return window::close(id);
                }
            }
        }
        Task::none()
    }

    // -----------------------------------------------------------------------
    // View
    // -----------------------------------------------------------------------
    pub fn view(&self) -> Element<'_, Message> {
        let bar = self.toolbar_row();

        let panel: Option<Element<Message>> = if self.picker.is_open {
            Some(self.picker_panel())
        } else if self.settings.is_open {
            Some(self.settings_panel())
        } else if self.videos.is_open {
            Some(self.videos_panel())
        } else {
            None
        };

        let body: Element<Message> = match panel {
            Some(panel) => column![
                container(bar).height(Length::Fixed(BAR_HEIGHT)),
                separator(),
                panel,
            ]
            .into(),
            None => container(bar).height(Length::Fill).into(),
        };

        // The pill fills the entire window; the OS clips the rounded corners.
        container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(pill_style)
            .into()
    }

    /// The always-visible toolbar strip
    fn toolbar_row(&self) -> Element<'_, Message> {
        // --- Drag grip ---
        let grip = mouse_area(
            container(
                row![grip_bar(), grip_bar()]
                    .spacing(3)
                    .align_y(Alignment::Center),
            )
            .padding([0, 4])
            .height(Length::Fill)
            .align_y(iced::alignment::Vertical::Center),
        )
        .on_press(Message::StartDrag);

        // --- Window-selection button: icon + alias ---
        let mut selection = row![]
            .spacing(6)
            .height(Length::Fill)
            .align_y(Alignment::Center);

        if let Some(handle) = self.picker.selected_icon() {
            selection = selection.push(process_icon(handle, 16));
        }

        selection = selection.push(
            text(self.picker.button_label())
                .size(13)
                .height(Length::Fill)
                .wrapping(text::Wrapping::None)
                .align_y(iced::alignment::Vertical::Center),
        );

        let window_btn = button(selection)
        .style(if self.picker.is_open {
            window_select_active_style
        } else {
            window_select_style
        })
        .on_press(Message::ToggleWindowPicker)
        // Shrink keeps it compact when nothing is selected and grows just
        // enough for an alias plus icon once one is.
        .width(Length::Shrink)
        .padding([0, 10])
        .height(32);

        // --- Record / Stop button ---
        // Recording needs a target, so the button stays disabled until a
        // window has been picked.
        let can_record = self.picker.selected.is_some();

        let (indicator, label, style): (Element<Message>, String, ButtonStyleFn) = if self.starting
        {
            (record_dot(true), tr("btn-preparing"), record_idle_style)
        } else if self.stopping {
            (stop_square(), tr("btn-saving"), record_active_style)
        } else if self.is_recording() {
            // Zero-padding keeps the label the same width all the way down, so
            // the button doesn't twitch as the countdown crosses ten seconds.
            (
                stop_square(),
                tr_arg("btn-stop", "clock", self.recording_clock()),
                record_active_style,
            )
        } else {
            (
                record_dot(can_record),
                tr("btn-record"),
                record_idle_style,
            )
        };

        let record_btn = button(
            row![
                indicator,
                text(label)
                    .size(13)
                    .font(UI_FONT_BOLD)
                    .height(Length::Fill)
                    .wrapping(text::Wrapping::None)
                    .align_y(iced::alignment::Vertical::Center)
            ]
            .spacing(7)
            .height(Length::Fill)
            .align_y(Alignment::Center),
        )
        .style(style)
        // Passing None is what puts the button into Status::Disabled.
        .on_press_maybe(
            (!self.starting && !self.stopping && can_record).then_some(Message::ToggleRecording),
        )
        .padding([0, 14])
        .height(32);

        // --- Recordings button ---
        let videos_btn = button(
            container(process_icon(&crate::film::handle(), 15))
                .width(Length::Fill)
                .height(Length::Fill)
                .center(Length::Fill),
        )
        .style(if self.videos.is_open {
            icon_btn_active_style
        } else {
            icon_btn_style
        })
        .on_press(Message::ToggleVideos)
        .padding(0)
        .width(28)
        .height(28);

        // --- Settings button ---
        let settings_btn = button(
            container(process_icon(&crate::cog::handle(), 15))
                .width(Length::Fill)
                .height(Length::Fill)
                .center(Length::Fill),
        )
        .style(if self.settings.is_open {
            icon_btn_active_style
        } else {
            icon_btn_style
        })
        .on_press(Message::ToggleSettings)
        .padding(0)
        .width(28)
        .height(28);

        // --- Close button ---
        let close_btn = button(
            text("\u{00D7}")
                .size(18)
                .font(UI_FONT)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center),
        )
        .style(close_btn_style)
        .on_press(Message::Close)
        .padding(0)
        .width(28)
        .height(28);

        // The filler doubles as drag surface, and carries the status readout.
        let filler = mouse_area(
            container(self.status_text())
                .width(Length::Fill)
                .height(Length::Fill)
                .clip(true)
                .align_y(iced::alignment::Vertical::Center),
        )
        .on_press(Message::StartDrag);

        row![grip, window_btn, record_btn, videos_btn, filler, settings_btn, close_btn]
            .spacing(8)
            .align_y(Alignment::Center)
            .padding([0, 10])
            .into()
    }

    /// Result of the last recording, or empty space when there's nothing to
    /// say. Doubles as part of the drag surface, so it must stay non-clickable.
    fn status_text(&self) -> Element<'_, Message> {
        let Some((status, _)) = &self.status else {
            return horizontal_space().into();
        };

        let (message, color) = match status {
            Status::Saved(path) => (
                tr_arg("status-saved", "name", file_name(path)),
                Color::from_rgb8(0x7E, 0xC8, 0x8A),
            ),
            Status::Deleted(name) => (
                tr_arg("status-deleted", "name", name),
                Color::from_rgb8(0x7E, 0xC8, 0x8A),
            ),
            Status::Compressed(message) => (message.clone(), Color::from_rgb8(0x7E, 0xC8, 0x8A)),
            Status::Info(message) => (message.clone(), Color::from_rgb8(0x9C, 0xB8, 0xF0)),
            Status::Failed(reason) => (reason.clone(), Color::from_rgb8(0xE8, 0x7A, 0x7A)),
        };

        text(truncate(&message, STATUS_MAX_CHARS))
            .size(10)
            .color(color)
            .wrapping(text::Wrapping::None)
            .into()
    }

    /// The expandable list of capturable windows
    fn picker_panel(&self) -> Element<'_, Message> {
        let header = row![
            text(tr_arg(
                "picker-window-count",
                "count",
                self.picker.windows.len().to_string(),
            ))
            .size(11)
            .font(UI_FONT_BOLD)
            .color(Color::from_rgb8(0x8A, 0x8A, 0x96)),
            horizontal_space(),
            small_button(tr("btn-refresh"), Message::RefreshWindows),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        // The allow-list is the MapleStory family; this checkbox lets other
        // applications through for testing or one-off captures.
        let filter_row = checkbox(tr("picker-show-others"), self.picker.include_others)
            .on_toggle(Message::ToggleOtherWindows)
            .text_size(11)
            .style(picker_checkbox_style);

        let list: Element<Message> = if self.picker.windows.is_empty() {
            container(
                text(if self.picker.include_others {
                    tr("picker-no-windows")
                } else {
                    tr("picker-no-maplestory")
                })
                    .size(12)
                    .color(Color::from_rgb8(0x7A, 0x7A, 0x86)),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .into()
        } else {
            let mut items = column![].spacing(2);

            for info in &self.picker.windows {
                let is_selected = self
                    .picker
                    .selected
                    .as_ref()
                    .is_some_and(|s| s.hwnd == info.hwnd);

                // Two lines: window title, then process name + dimensions.
                // Several MapleStory variants can run at once, so the
                // executable disambiguates similarly-titled windows.
                let detail_color = if is_selected {
                    Color::from_rgb8(0xD2, 0xDF, 0xFF)
                } else {
                    Color::from_rgb8(0x7E, 0x7E, 0x8A)
                };

                let mut entry = row![].spacing(9).align_y(Alignment::Center);

                if let Some(handle) = &info.icon {
                    entry = entry.push(process_icon(handle, 22));
                }

                items = items.push(
                    button(
                        entry.push(
                            column![
                                text(info.alias.as_str())
                                    .size(13)
                                    .font(UI_FONT_BOLD)
                                    .width(Length::Fill)
                                    .wrapping(text::Wrapping::None),
                                text(info.detail()).size(10).color(detail_color),
                            ]
                            .spacing(1),
                        ),
                    )
                    .style(if is_selected {
                        list_item_selected_style
                    } else {
                        list_item_style
                    })
                    .on_press(Message::WindowSelected(info.clone()))
                    .width(Length::Fill)
                    .padding([7, 9]),
                );
            }

            list_scrollable(items)
        };

        let mut panel = column![header, filter_row, list].spacing(8).padding([8, 12]);

        if self.picker.selected.is_some() {
            panel = panel.push(small_button(
                tr("picker-clear-selection"),
                Message::ClearSelection,
            ));
        }

        container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// The expandable settings form
    fn settings_panel(&self) -> Element<'_, Message> {
        let header = row![
            text(tr("panel-settings"))
                .size(11)
                .font(UI_FONT_BOLD)
                .color(Color::from_rgb8(0x8A, 0x8A, 0x96)),
            horizontal_space(),
            small_button(tr("btn-done"), Message::SaveSettings),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        // --- Output directory ---
        let path_row = row![
            text_input(&tr("field-output-dir"), &self.settings.output_dir)
                .on_input(Message::OutputDirChanged)
                .on_submit(Message::SaveSettings)
                .style(field_style)
                .size(12)
                .padding([6, 8])
                .width(Length::Fill),
            small_button(tr("btn-browse"), Message::BrowseOutputDir),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        // --- Duration cap ---
        let duration_row = row![
            text_input("0", &self.settings.max_duration)
                .on_input(Message::MaxDurationChanged)
                .on_submit(Message::SaveSettings)
                .style(field_style)
                .size(12)
                .padding([6, 8])
                .width(Length::Fixed(72.0)),
            text(match self.config.max_duration() {
                Some(_) => tr("hint-duration-capped"),
                None => tr("hint-duration-unlimited"),
            })
            .size(11)
            .color(Color::from_rgb8(0x7E, 0x7E, 0x8A)),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        // --- Hotkey ---
        // The binding is shown as key caps rather than as the label of the
        // button that changes it, so what's assigned reads as a fact about the
        // app and stays legible while the field is armed.
        let mut hotkey_row = row![self.hotkey_display()]
            .spacing(8)
            .align_y(Alignment::Center)
            .push(horizontal_space())
            .push(small_button(
                if self.settings.capturing {
                    tr("btn-cancel")
                } else {
                    tr("btn-change")
                },
                Message::CaptureHotkey,
            ));

        if self.settings.hotkey.is_some() && !self.settings.capturing {
            hotkey_row = hotkey_row.push(small_button(tr("btn-clear"), Message::ClearHotkey));
        }

        let hotkey_hint = text(if self.settings.capturing {
            tr("hint-hotkey-capturing")
        } else {
            tr("hint-hotkey-global")
        })
        .size(10)
        .color(Color::from_rgb8(0x7E, 0x7E, 0x8A))
        .wrapping(text::Wrapping::None);

        let language_row = pick_list(
            Language::ALL,
            Some(self.settings.language),
            Message::SelectLanguage,
        )
        .text_size(11)
        .padding([5, 8])
        .width(Length::Fill);

        let panel = column![
            header,
            field_label(tr("field-output-dir")),
            path_row,
            field_label(tr("field-max-duration")),
            duration_row,
            field_label(tr("field-record-hotkey")),
            hotkey_row,
            hotkey_hint,
            field_label(tr("field-language")),
            language_row,
            horizontal_space(),
            text(tr_arg("label-config-file", "path", crate::config::config_dir_display()))
                .size(9)
                .color(Color::from_rgb8(0x60, 0x60, 0x6C))
                .wrapping(text::Wrapping::None),
        ]
        .spacing(5)
        .padding([8, 12]);

        container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// The recordings manager: every clip in the output directory, newest
    /// first, each with its actions.
    fn videos_panel(&self) -> Element<'_, Message> {
        let header = row![
            text(tr_arg(
                "videos-count",
                "count",
                self.videos.videos.len().to_string(),
            ))
            .size(11)
            .font(UI_FONT_BOLD)
            .color(Color::from_rgb8(0x8A, 0x8A, 0x96)),
            horizontal_space(),
            small_button(tr("btn-refresh"), Message::RefreshVideos),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        let list: Element<Message> = if self.videos.videos.is_empty() {
            container(
                text(tr("videos-empty"))
                    .size(12)
                    .color(Color::from_rgb8(0x7A, 0x7A, 0x86)),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .into()
        } else {
            let mut items = column![].spacing(2);

            for entry in &self.videos.videos {
                let compressing = self
                    .videos
                    .compressing
                    .as_ref()
                    .is_some_and(|current| current == &entry.path);

                let confirming = self
                    .videos
                    .pending_delete
                    .as_ref()
                    .is_some_and(|pending| pending == &entry.path);

                let actions = if compressing {
                    // The ffmpeg pass runs on a worker; other actions are
                    // hidden while the file is being rewritten, and the pass
                    // can be cancelled.
                    row![
                        text(tr("status-compressing"))
                            .size(11)
                            .color(Color::from_rgb8(0x9C, 0xB8, 0xF0)),
                        small_button(tr("btn-cancel"), Message::CancelCompression),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center)
                } else if confirming {
                    // Inline confirmation in place of a modal: the row's
                    // actions are swapped until 确认 or 取消 is pressed.
                    row![
                        text(tr("confirm-delete"))
                            .size(11)
                            .color(Color::from_rgb8(0xE8, 0x7A, 0x7A)),
                        small_danger_button(tr("btn-delete"), Message::ConfirmDeleteVideo),
                        small_button(tr("btn-cancel"), Message::CancelDeleteVideo),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center)
                } else {
                    // While a compression is running, the other rows'
                    // compress buttons dim out instead of queueing.
                    row![
                        small_button(tr("btn-play"), Message::PlayVideo(entry.path.clone())),
                        small_button_maybe(
                            tr("btn-compress"),
                            self.videos
                                .compressing
                                .is_none()
                                .then_some(Message::CompressVideo(entry.path.clone())),
                        ),
                        small_button(
                            tr("btn-show-folder"),
                            Message::BrowseVideo(entry.path.clone()),
                        ),
                        small_danger_button(tr("btn-delete"), Message::DeleteVideo(entry.path.clone())),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center)
                };

                // The name takes whatever room the row leaves over, clipping
                // rather than shoving the actions around.
                let name = container(
                    text(entry.display_name())
                        .size(12)
                        .wrapping(text::Wrapping::None),
                )
                .width(Length::Fill)
                .clip(true)
                .align_y(iced::alignment::Vertical::Center);

                let size = text(crate::videos::human_size(entry.size))
                    .size(11)
                    .color(Color::from_rgb8(0x7E, 0x7E, 0x8A))
                    .width(Length::Fixed(64.0))
                    .align_x(iced::alignment::Horizontal::Right)
                    .align_y(iced::alignment::Vertical::Center);

                items = items.push(
                    row![name, size, actions]
                        .spacing(8)
                        .align_y(Alignment::Center)
                        .padding([6, 2]),
                );
            }

            list_scrollable(items)
        };

        let panel = column![header, list].spacing(8).padding([8, 12]);

        container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// The assigned shortcut, as one cap per key.
    ///
    /// Stays on screen while the field is armed – the caps just dim – so the
    /// user can see what they're about to replace.
    fn hotkey_display(&self) -> Element<'_, Message> {
        let Some(hotkey) = self.settings.hotkey else {
            let (label, color) = if self.settings.capturing {
                (tr("hotkey-waiting"), Color::from_rgb8(0x9C, 0xB8, 0xF0))
            } else {
                (tr("hotkey-unset"), Color::from_rgb8(0x7E, 0x7E, 0x8A))
            };

            return text(label)
                .size(12)
                .color(color)
                // Matches a cap, so the row doesn't change height when the
                // shortcut is cleared.
                .height(Length::Fixed(KEY_CAP_HEIGHT))
                .align_y(iced::alignment::Vertical::Center)
                .into();
        };

        let mut caps = row![].spacing(4).align_y(Alignment::Center);

        for (index, part) in hotkey.parts().iter().enumerate() {
            if index > 0 {
                caps = caps.push(
                    text("+")
                        .size(10)
                        .color(Color::from_rgb8(0x6E, 0x6E, 0x7A)),
                );
            }
            caps = caps.push(key_cap(part, self.settings.capturing));
        }

        caps.into()
    }

    // -----------------------------------------------------------------------
    // Theme
    // -----------------------------------------------------------------------
    pub fn theme(&self) -> Theme {
        // Dark, with one accent swapped: the scrollbar's hover stress colour.
        static THEME: OnceLock<Theme> = OnceLock::new();

        THEME
            .get_or_init(|| {
                let palette = Theme::Dark.palette();

                Theme::custom_with_fn("CMS Video Recorder".to_string(), palette, |palette| {
                    let mut extended = iced::theme::palette::Extended::generate(palette);
                    extended.primary.strong.color = Color::from_rgb8(0xFF, 0x9D, 0x00);
                    extended
                })
            })
            .clone()
    }
}

// ---------------------------------------------------------------------------
// Small building blocks
// ---------------------------------------------------------------------------

/// File name of a path, for display. Falls back to the whole path.
fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// Clips to `max` characters, adding an ellipsis when something was cut.
///
/// Counts chars rather than bytes so CJK text isn't split mid-codepoint.
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }

    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}\u{2026}")
}

fn small_button(label: String, msg: Message) -> Element<'static, Message> {
    button(text(label).size(11))
        .style(subtle_button_style)
        .on_press(msg)
        .padding([4, 9])
        .into()
}

/// A small destructive button, e.g. the clip's "删除".
fn small_danger_button(label: String, msg: Message) -> Element<'static, Message> {
    button(text(label).size(11))
        .style(danger_button_style)
        .on_press(msg)
        .padding([4, 9])
        .into()
}

/// A small button that may be disabled by passing `None`.
fn small_button_maybe(label: String, msg: Option<Message>) -> Element<'static, Message> {
    button(text(label).size(11))
        .style(subtle_button_style)
        .on_press_maybe(msg)
        .padding([4, 9])
        .into()
}

/// A scrollable list with the slim scrollbar used by both panels.
fn list_scrollable<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    scrollable(content)
        .direction(Direction::Vertical(
            Scrollbar::default().width(4.0).scroller_width(4.0),
        ))
        .height(Length::Fill)
        .into()
}

/// Caption above a settings field.
fn field_label(label: String) -> Element<'static, Message> {
    text(label)
        .size(10)
        .color(Color::from_rgb8(0x8A, 0x8A, 0x96))
        .into()
}

/// Height of a key cap, also used by the "not set" placeholder.
const KEY_CAP_HEIGHT: f32 = 22.0;

/// One key of a shortcut, drawn as a keyboard cap.
///
/// `dimmed` is set while a replacement is being read, so the outgoing binding
/// stays readable without looking current.
fn key_cap<'a>(label: &str, dimmed: bool) -> Element<'a, Message> {
    let (fill, edge, ink) = if dimmed {
        (
            Color::from_rgb8(0x26, 0x26, 0x2D),
            Color::from_rgb8(0x38, 0x38, 0x42),
            Color::from_rgb8(0x76, 0x76, 0x82),
        )
    } else {
        (
            Color::from_rgb8(0x33, 0x33, 0x3D),
            Color::from_rgb8(0x4C, 0x4C, 0x5A),
            Color::from_rgb8(0xEC, 0xEC, 0xF2),
        )
    };

    container(
        text(label.to_string())
            .size(11)
            .font(UI_FONT_BOLD)
            .color(ink)
            .wrapping(text::Wrapping::None),
    )
    .padding([0, 7])
    .height(Length::Fixed(KEY_CAP_HEIGHT))
    .align_y(iced::alignment::Vertical::Center)
    .style(move |_: &Theme| container::Style {
        background: Some(Background::Color(fill)),
        border: Border {
            color: edge,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    })
    .into()
}

/// The recording clock, as `05s` under a minute and `01:05` above it.
///
/// Two digits either way, so the Stop button doesn't twitch as the number
/// crosses ten seconds.
fn clock(secs: u64) -> String {
    if secs < 60 {
        format!("{secs:02}s")
    } else {
        format!("{:02}:{:02}", secs / 60, secs % 60)
    }
}

/// A process icon at a fixed square size.
///
/// The handle is reference-counted, so cloning it per frame is cheap – the
/// pixels are decoded once per refresh, not per redraw.
fn process_icon<'a>(handle: &image::Handle, size: u16) -> Element<'a, Message> {
    image(handle.clone())
        .width(Length::Fixed(size as f32))
        .height(Length::Fixed(size as f32))
        .content_fit(iced::ContentFit::Contain)
        .into()
}

/// Thin divider between the toolbar and the panel
fn separator<'a>() -> Element<'a, Message> {
    container(horizontal_space())
        .width(Length::Fill)
        .height(1)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgb8(0x2E, 0x2E, 0x36))),
            ..Default::default()
        })
        .into()
}

/// One vertical bar of the drag grip
fn grip_bar<'a>() -> Element<'a, Message> {
    container(text(""))
        .width(2)
        .height(16)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgb8(0x55, 0x55, 0x60))),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 1.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// Red circle used for the idle Record button, dimmed when unavailable
fn record_dot<'a>(enabled: bool) -> Element<'a, Message> {
    let fill = if enabled {
        Color::from_rgb8(0xFF, 0x5B, 0x5B)
    } else {
        Color::from_rgb8(0x6E, 0x5A, 0x5A)
    };

    container(text(""))
        .width(10)
        .height(10)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(fill)),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 5.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// White rounded square used while recording
fn stop_square<'a>() -> Element<'a, Message> {
    container(text(""))
        .width(10)
        .height(10)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(Color::WHITE)),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        })
        .into()
}

// ---------------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------------

fn rounded(radius: f32, bg: Color, fg: Color) -> button::Style {
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: fg,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: radius.into(),
        },
        shadow: Shadow::default(),
    }
}

fn is_active(status: button::Status) -> bool {
    matches!(status, button::Status::Hovered | button::Status::Pressed)
}

fn is_disabled(status: button::Status) -> bool {
    matches!(status, button::Status::Disabled)
}

/// Muted fill + faded label for a button that can't be pressed
fn disabled_style(radius: f32) -> button::Style {
    rounded(
        radius,
        Color::from_rgb8(0x28, 0x28, 0x30),
        Color::from_rgb8(0x6A, 0x6A, 0x76),
    )
}

/// The visible pill, filling the whole clipped window.
fn pill_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(pill_bg())),
        border: Border {
            color: Color::from_rgb8(0x3A, 0x3A, 0x42),
            width: 1.0,
            radius: PILL_RADIUS.into(),
        },
        shadow: Shadow::default(),
        text_color: None,
    }
}

fn window_select_style(_theme: &Theme, status: button::Status) -> button::Style {
    let bg = if is_active(status) {
        Color::from_rgb8(0x3C, 0x3C, 0x46)
    } else {
        Color::from_rgb8(0x2E, 0x2E, 0x36)
    };
    rounded(6.0, bg, Color::from_rgb8(0xE6, 0xE6, 0xEA))
}

/// Highlighted while the panel is expanded
fn window_select_active_style(_theme: &Theme, status: button::Status) -> button::Style {
    let bg = if is_active(status) {
        Color::from_rgb8(0x4A, 0x4A, 0x58)
    } else {
        Color::from_rgb8(0x42, 0x42, 0x4E)
    };
    rounded(6.0, bg, Color::WHITE)
}

fn record_idle_style(_theme: &Theme, status: button::Status) -> button::Style {
    if is_disabled(status) {
        return disabled_style(6.0);
    }

    let bg = if is_active(status) {
        Color::from_rgb8(0x3D, 0x74, 0xE8)
    } else {
        Color::from_rgb8(0x2F, 0x62, 0xD4)
    };
    rounded(6.0, bg, Color::WHITE)
}

fn record_active_style(_theme: &Theme, status: button::Status) -> button::Style {
    // Reached while the encoder is finalising the file, when the button is
    // showing "保存中" and can't be pressed.
    if is_disabled(status) {
        return disabled_style(6.0);
    }

    let bg = if is_active(status) {
        Color::from_rgb8(0xD2, 0x2E, 0x2E)
    } else {
        Color::from_rgb8(0xB8, 0x22, 0x22)
    };
    rounded(6.0, bg, Color::WHITE)
}

fn close_btn_style(_theme: &Theme, status: button::Status) -> button::Style {
    if is_active(status) {
        rounded(14.0, Color::from_rgb8(0xC7, 0x2C, 0x2C), Color::WHITE)
    } else {
        rounded(
            14.0,
            Color::from_rgb8(0x30, 0x30, 0x38),
            Color::from_rgb8(0xC8, 0xC8, 0xD0),
        )
    }
}

/// Circular icon button in the toolbar strip, e.g. the settings cog
fn icon_btn_style(_theme: &Theme, status: button::Status) -> button::Style {
    let bg = if is_active(status) {
        Color::from_rgb8(0x3C, 0x3C, 0x46)
    } else {
        Color::from_rgb8(0x2A, 0x2A, 0x32)
    };
    rounded(14.0, bg, Color::from_rgb8(0xDC, 0xDC, 0xE4))
}

/// Highlighted while the settings panel or recordings manager is expanded
fn icon_btn_active_style(_theme: &Theme, status: button::Status) -> button::Style {
    let bg = if is_active(status) {
        Color::from_rgb8(0x4A, 0x4A, 0x58)
    } else {
        Color::from_rgb8(0x42, 0x42, 0x4E)
    };
    rounded(14.0, bg, Color::WHITE)
}

/// Checkbox in the window picker: toggles listing non-MapleStory windows.
fn picker_checkbox_style(_theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    let checked = matches!(
        status,
        checkbox::Status::Active { is_checked: true }
            | checkbox::Status::Hovered { is_checked: true }
            | checkbox::Status::Disabled { is_checked: true }
    );

    checkbox::Style {
        background: if checked {
            Background::Color(Color::from_rgb8(0x3D, 0x74, 0xE8))
        } else {
            Background::Color(Color::from_rgb8(0x24, 0x24, 0x2B))
        },
        icon_color: Color::WHITE,
        border: Border {
            color: if checked {
                Color::from_rgb8(0x3D, 0x74, 0xE8)
            } else {
                Color::from_rgb8(0x50, 0x50, 0x5A)
            },
            width: 1.0,
            radius: 4.0.into(),
        },
        text_color: Some(Color::from_rgb8(0x9A, 0x9A, 0xA4)),
    }
}

/// Text field in the settings panel
fn field_style(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let border_color = match status {
        text_input::Status::Focused => Color::from_rgb8(0x3D, 0x74, 0xE8),
        text_input::Status::Hovered => Color::from_rgb8(0x4A, 0x4A, 0x58),
        _ => Color::from_rgb8(0x3A, 0x3A, 0x42),
    };

    text_input::Style {
        background: Background::Color(Color::from_rgb8(0x24, 0x24, 0x2B)),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 5.0.into(),
        },
        icon: Color::from_rgb8(0x8A, 0x8A, 0x96),
        placeholder: Color::from_rgb8(0x6A, 0x6A, 0x76),
        value: Color::from_rgb8(0xE6, 0xE6, 0xEA),
        selection: Color::from_rgb8(0x2F, 0x62, 0xD4),
    }
}

fn subtle_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    // Dimmed while disabled, e.g. the compress button while another clip is
    // being compressed.
    if is_disabled(status) {
        return rounded(
            5.0,
            Color::from_rgb8(0x24, 0x24, 0x2B),
            Color::from_rgb8(0x5E, 0x5E, 0x68),
        );
    }

    let bg = if is_active(status) {
        Color::from_rgb8(0x3C, 0x3C, 0x46)
    } else {
        Color::from_rgb8(0x2A, 0x2A, 0x32)
    };
    rounded(5.0, bg, Color::from_rgb8(0xC8, 0xC8, 0xD2))
}

/// Destructive action, e.g. deleting a clip
fn danger_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let bg = if is_active(status) {
        Color::from_rgb8(0x8E, 0x2E, 0x2E)
    } else {
        Color::from_rgb8(0x50, 0x24, 0x24)
    };
    rounded(5.0, bg, Color::from_rgb8(0xF2, 0xC4, 0xC4))
}

fn list_item_style(_theme: &Theme, status: button::Status) -> button::Style {
    let bg = if is_active(status) {
        Color::from_rgb8(0x32, 0x32, 0x3C)
    } else {
        Color::TRANSPARENT
    };
    rounded(5.0, bg, Color::from_rgb8(0xDC, 0xDC, 0xE4))
}

fn list_item_selected_style(_theme: &Theme, status: button::Status) -> button::Style {
    let bg = if is_active(status) {
        Color::from_rgb8(0x3D, 0x74, 0xE8)
    } else {
        Color::from_rgb8(0x2F, 0x62, 0xD4)
    };
    rounded(5.0, bg, Color::WHITE)
}
