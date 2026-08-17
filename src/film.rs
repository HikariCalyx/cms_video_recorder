// film.rs – the recordings manager's film-strip icon, rasterised at startup
//
// Same hand-drawn approach as cog.rs: a rounded frame with two rows of
// sprocket holes, rendered once into an RGBA buffer that every frame reuses.

use std::sync::OnceLock;

use iced::widget::image;
use iced::Color;

/// Rasterised size in pixels, drawn well above the ~15px display size.
const SIZE: u32 = 64;

/// Supersampling factor per axis.
const SAMPLES: u32 = 3;

// Film body, in centered canvas coordinates: 48 wide, 40 tall.
const LEFT: f32 = -24.0;
const TOP: f32 = -20.0;
const RIGHT: f32 = 24.0;
const BOTTOM: f32 = 20.0;
const RADIUS: f32 = 7.0;

// Sprocket holes: half side length, the two rows, and the x positions.
const HOLE_HALF: f32 = 2.4;
const HOLE_ROW_TOP: f32 = -14.5;
const HOLE_ROW_BOTTOM: f32 = 14.5;
const HOLE_XS: [f32; 4] = [-12.0, -4.0, 4.0, 12.0];

/// The film strip, decoded once and shared by every frame.
pub fn handle() -> image::Handle {
    static HANDLE: OnceLock<image::Handle> = OnceLock::new();

    HANDLE
        .get_or_init(|| {
            let color = Color::from_rgb8(0xDC, 0xDC, 0xE4);
            image::Handle::from_rgba(SIZE, SIZE, render(color))
        })
        .clone()
}

/// Tightly packed RGBA8: a flat-coloured film strip on a transparent field,
/// with coverage in the alpha channel.
fn render(color: Color) -> Vec<u8> {
    let [red, green, blue, _] = color.into_rgba8();

    let center = SIZE as f32 / 2.0;
    let step = 1.0 / SAMPLES as f32;
    let total = (SAMPLES * SAMPLES) as f32;

    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);

    for y in 0..SIZE {
        for x in 0..SIZE {
            let mut hits = 0.0;

            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let px = x as f32 + (sx as f32 + 0.5) * step - center;
                    let py = y as f32 + (sy as f32 + 0.5) * step - center;

                    if covers(px, py) {
                        hits += 1.0;
                    }
                }
            }

            let alpha = (hits / total * 255.0).round() as u8;
            pixels.extend_from_slice(&[red, green, blue, alpha]);
        }
    }

    pixels
}

/// Whether a point lies on the film strip: inside the rounded frame but
/// outside every sprocket hole.
fn covers(x: f32, y: f32) -> bool {
    if !rounded_rect(x, y, LEFT, TOP, RIGHT, BOTTOM, RADIUS) {
        return false;
    }

    for x_center in HOLE_XS {
        for y_center in [HOLE_ROW_TOP, HOLE_ROW_BOTTOM] {
            if rounded_rect(
                x,
                y,
                x_center - HOLE_HALF,
                y_center - HOLE_HALF,
                x_center + HOLE_HALF,
                y_center + HOLE_HALF,
                1.0,
            ) {
                return false;
            }
        }
    }

    true
}

/// Point-in-rounded-rectangle test.
///
/// A point is inside when it lies within `radius` of the core rectangle
/// (the bounds shrunk by the corner radius), which reproduces the corner
/// arcs exactly.
fn rounded_rect(x: f32, y: f32, left: f32, top: f32, right: f32, bottom: f32, radius: f32) -> bool {
    if x < left || x > right || y < top || y > bottom {
        return false;
    }

    let nearest_x = x.clamp(left + radius, right - radius);
    let nearest_y = y.clamp(top + radius, bottom - radius);
    let dx = x - nearest_x;
    let dy = y - nearest_y;

    dx * dx + dy * dy <= radius * radius
}
