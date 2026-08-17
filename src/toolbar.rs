// toolbar.rs – main floating toolbar state and view

use iced::{
    font::{Family, Weight},
    widget::{button, column, container, horizontal_space, mouse_area, row, scrollable, text},
    window, Alignment, Background, Border, Color, Element, Font, Length, Shadow, Size, Subscription,
    Task, Theme,
};

use crate::window_picker::{WindowInfo, WindowPickerState};

/// Default UI font – Microsoft YaHei UI ships with Windows and covers both
/// Latin and CJK, so labels and window titles render without tofu boxes.
pub const UI_FONT: Font = Font {
    family: Family::Name("Microsoft YaHei UI"),
    ..Font::DEFAULT
};

/// Semibold variant for the Record button label
pub const UI_FONT_SEMIBOLD: Font = Font {
    family: Family::Name("Microsoft YaHei UI"),
    weight: Weight::Semibold,
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
    /// Forget the current selection
    ClearSelection,
    /// Toggle recording on / off
    ToggleRecording,
    /// Close the application
    Close,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct Toolbar {
    pub picker: WindowPickerState,
    pub is_recording: bool,
    pub window_id: Option<window::Id>,
}

impl Toolbar {
    // -----------------------------------------------------------------------
    // Subscription
    // -----------------------------------------------------------------------
    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            window::open_events().map(Message::WindowOpened),
            // Reapplying the clip region on resize is more reliable than
            // chaining it after the resize task, which can race the OS.
            window::resize_events().map(|_| Message::WindowResized),
        ])
    }

    /// Resize the window to match the current expanded/collapsed state
    fn sync_window_size(&self) -> Task<Message> {
        let Some(id) = self.window_id else {
            return Task::none();
        };

        let height = if self.picker.is_open {
            EXPANDED_HEIGHT
        } else {
            BAR_HEIGHT
        };

        window::resize(id, Size::new(WINDOW_WIDTH, height))
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
                self.picker.toggle();
                return self.sync_window_size();
            }
            Message::WindowSelected(info) => {
                self.picker.select(info);
                return self.sync_window_size();
            }
            Message::RefreshWindows => {
                self.picker.refresh();
            }
            Message::ClearSelection => {
                self.picker.selected = None;
            }
            Message::ToggleRecording => {
                self.is_recording = !self.is_recording;
                // TODO: wire up actual capture backend here
            }
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

        let body: Element<Message> = if self.picker.is_open {
            column![
                container(bar).height(Length::Fixed(BAR_HEIGHT)),
                separator(),
                self.picker_panel(),
            ]
            .into()
        } else {
            container(bar).height(Length::Fill).into()
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

        // --- Window-selection button (half the previous width) ---
        let window_btn = button(
            text(self.picker.button_label())
                .size(13)
                .height(Length::Fill)
                .align_y(iced::alignment::Vertical::Center),
        )
        .style(if self.picker.is_open {
            window_select_active_style
        } else {
            window_select_style
        })
        .on_press(Message::ToggleWindowPicker)
        .width(Length::Fixed(100.0))
        .padding([0, 10])
        .height(32);

        // --- Record / Stop button ---
        let (indicator, label, style): (
            Element<Message>,
            &str,
            fn(&Theme, button::Status) -> button::Style,
        ) = if self.is_recording {
            (stop_square(), "Stop", record_active_style)
        } else {
            (record_dot(), "Record", record_idle_style)
        };

        let record_btn = button(
            row![
                indicator,
                text(label)
                    .size(13)
                    .font(UI_FONT_SEMIBOLD)
                    .height(Length::Fill)
                    .align_y(iced::alignment::Vertical::Center)
            ]
            .spacing(7)
            .height(Length::Fill)
            .align_y(Alignment::Center),
        )
        .style(style)
        .on_press(Message::ToggleRecording)
        .padding([0, 14])
        .height(32);

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

        // The filler doubles as drag surface.
        let filler = mouse_area(
            container(horizontal_space())
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::StartDrag);

        row![grip, window_btn, record_btn, filler, close_btn]
            .spacing(8)
            .align_y(Alignment::Center)
            .padding([0, 10])
            .into()
    }

    /// The expandable list of capturable windows
    fn picker_panel(&self) -> Element<'_, Message> {
        let header = row![
            text(format!("{} windows", self.picker.windows.len()))
                .size(11)
                .font(UI_FONT_SEMIBOLD)
                .color(Color::from_rgb8(0x8A, 0x8A, 0x96)),
            horizontal_space(),
            small_button("Refresh", Message::RefreshWindows),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        let list: Element<Message> = if self.picker.windows.is_empty() {
            container(
                text("No capturable windows found")
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

                items = items.push(
                    button(
                        row![
                            text(&info.title)
                                .size(12)
                                .width(Length::Fill)
                                .wrapping(text::Wrapping::None),
                            text(info.dimensions())
                                .size(11)
                                .color(Color::from_rgb8(0x7E, 0x7E, 0x8A)),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
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

            scrollable(items).height(Length::Fill).into()
        };

        let mut panel = column![header, list].spacing(8).padding([8, 12]);

        if self.picker.selected.is_some() {
            panel = panel.push(small_button("Clear selection", Message::ClearSelection));
        }

        container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    // -----------------------------------------------------------------------
    // Theme
    // -----------------------------------------------------------------------
    pub fn theme(&self) -> Theme {
        Theme::Dark
    }
}

// ---------------------------------------------------------------------------
// Small building blocks
// ---------------------------------------------------------------------------

fn small_button(label: &str, msg: Message) -> Element<'_, Message> {
    button(text(label).size(11))
        .style(subtle_button_style)
        .on_press(msg)
        .padding([4, 9])
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

/// Red circle used for the idle Record button
fn record_dot<'a>() -> Element<'a, Message> {
    container(text(""))
        .width(10)
        .height(10)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgb8(0xFF, 0x5B, 0x5B))),
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
    let bg = if is_active(status) {
        Color::from_rgb8(0x3D, 0x74, 0xE8)
    } else {
        Color::from_rgb8(0x2F, 0x62, 0xD4)
    };
    rounded(6.0, bg, Color::WHITE)
}

fn record_active_style(_theme: &Theme, status: button::Status) -> button::Style {
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

fn subtle_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let bg = if is_active(status) {
        Color::from_rgb8(0x3C, 0x3C, 0x46)
    } else {
        Color::from_rgb8(0x2A, 0x2A, 0x32)
    };
    rounded(5.0, bg, Color::from_rgb8(0xC8, 0xC8, 0xD2))
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
