// hotkey.rs – system-wide record/stop shortcut
//
// `RegisterHotKey` delivers WM_HOTKEY to the queue of the thread that
// registered it. iced owns the main window's message loop and gives us no way
// to see messages it doesn't recognise, so registration happens on a dedicated
// thread with its own `GetMessage` loop instead. That thread forwards presses
// into an iced subscription, and re-binds when the user changes the shortcut.

use std::sync::atomic::{AtomicU64, Ordering};

use iced::futures::{SinkExt, Stream, StreamExt};

use crate::config::Hotkey;

/// The binding the listener should hold, packed by [`Hotkey::to_bits`].
static DESIRED: AtomicU64 = AtomicU64::new(Hotkey::NONE_BITS);

/// What the listener reports back to the UI.
#[derive(Debug, Clone)]
pub enum Event {
    /// The shortcut was pressed.
    Pressed,
    /// Windows refused the combination, which in practice means another
    /// application already owns it.
    Rejected,
}

/// Publishes the shortcut the listener should hold.
///
/// Safe to call before the listener exists: it reads [`DESIRED`] on startup, so
/// the wake-up is only needed to interrupt an already-running message loop.
pub fn apply(hotkey: Option<Hotkey>) {
    DESIRED.store(Hotkey::to_bits(hotkey), Ordering::Relaxed);
    wake_listener();
}

/// Stream of hotkey events, for `Subscription::run`.
///
/// The listener thread is spawned the first time the subscription is built and
/// lives for the rest of the process.
pub fn listen() -> impl Stream<Item = Event> {
    iced::stream::channel(4, |mut output| async move {
        let (sender, mut receiver) = iced::futures::channel::mpsc::unbounded();

        std::thread::spawn(move || run(&sender));

        while let Some(event) = receiver.next().await {
            if output.send(event).await.is_err() {
                break;
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Windows listener
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod platform {
    use std::sync::atomic::{AtomicU32, Ordering};

    use iced::futures::channel::mpsc::UnboundedSender;

    use windows::Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        System::Threading::GetCurrentThreadId,
        UI::{
            Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS},
            WindowsAndMessaging::{GetMessageW, PostThreadMessageW, MSG, WM_APP, WM_HOTKEY},
        },
    };

    use super::{Event, DESIRED};
    use crate::config::Hotkey;

    /// Only one hotkey is ever registered, so a constant id is enough – it just
    /// has to be unique within the thread.
    const HOTKEY_ID: i32 = 0x0C15;

    /// Private message asking the listener to re-read `DESIRED`.
    const WM_REBIND: u32 = WM_APP + 1;

    /// Holding the keys down must not restart the recording, so ask Windows not
    /// to auto-repeat the hotkey.
    const MOD_NOREPEAT: u32 = 0x4000;

    /// Thread id of the listener, or 0 before it starts.
    static LISTENER: AtomicU32 = AtomicU32::new(0);

    pub fn wake_listener() {
        let listener = LISTENER.load(Ordering::Relaxed);
        if listener == 0 {
            return;
        }

        unsafe {
            let _ = PostThreadMessageW(listener, WM_REBIND, WPARAM(0), LPARAM(0));
        }
    }

    /// Message loop of the listener thread.
    pub fn run(sender: &UnboundedSender<Event>) {
        LISTENER.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);

        let mut bound = false;
        rebind(&mut bound, sender);

        let mut message = MSG::default();

        // A null window handle picks up thread messages, which is where both
        // WM_HOTKEY and WM_REBIND land. GetMessageW returns 0 only on WM_QUIT
        // and -1 on error, neither of which happens here.
        while unsafe { GetMessageW(&mut message, HWND::default(), 0, 0) }.0 > 0 {
            match message.message {
                WM_HOTKEY => {
                    if sender.unbounded_send(Event::Pressed).is_err() {
                        break;
                    }
                }
                WM_REBIND => rebind(&mut bound, sender),
                _ => {}
            }
        }

        if bound {
            unsafe {
                let _ = UnregisterHotKey(HWND::default(), HOTKEY_ID);
            }
        }
    }

    /// Drops the current registration and takes out the one `DESIRED` asks for.
    fn rebind(bound: &mut bool, sender: &UnboundedSender<Event>) {
        if *bound {
            unsafe {
                let _ = UnregisterHotKey(HWND::default(), HOTKEY_ID);
            }
            *bound = false;
        }

        let Some(hotkey) = Hotkey::from_bits(DESIRED.load(Ordering::Relaxed)) else {
            return;
        };

        let modifiers = HOT_KEY_MODIFIERS(hotkey.modifier_bits() | MOD_NOREPEAT);
        let result = unsafe { RegisterHotKey(HWND::default(), HOTKEY_ID, modifiers, hotkey.key) };

        match result {
            Ok(()) => *bound = true,
            Err(_) => {
                let _ = sender.unbounded_send(Event::Rejected);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Non-Windows stub
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
mod platform {
    use iced::futures::channel::mpsc::UnboundedSender;

    use super::Event;

    pub fn wake_listener() {}

    /// No global hotkey support off Windows, and nothing to record either.
    pub fn run(_sender: &UnboundedSender<Event>) {}
}

use platform::{run, wake_listener};
