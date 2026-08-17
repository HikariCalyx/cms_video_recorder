// cog.rs – the settings button's gear, rasterised at startup
//
// Drawn rather than loaded: an SVG or an icon font would either pull in another
// iced feature or depend on a Windows symbol font being present, and a gear is
// two radii and a hole. The app already feeds RGBA buffers to the image widget
// for process icons, so this reuses that path.

use std::sync::OnceLock;

use iced::widget::image;
use iced::Color;

/// Rasterised size in pixels. Drawn well above the ~16px display size so
/// downscaling does the anti-aliasing work the sampler is good at.
const SIZE: u32 = 64;

/// Number of teeth.
const TEETH: f32 = 8.0;

/// Fraction of each tooth period that is tooth rather than gap.
const TOOTH_DUTY: f32 = 0.46;

/// Radii as a fraction of the canvas, from tooth tip inward.
const TIP: f32 = 0.47;
const ROOT: f32 = 0.345;
const HOLE: f32 = 0.165;

/// Supersampling factor per axis.
const SAMPLES: u32 = 3;

/// The gear, decoded once and shared by every frame.
pub fn handle() -> image::Handle {
    static HANDLE: OnceLock<image::Handle> = OnceLock::new();

    HANDLE
        .get_or_init(|| {
            let color = Color::from_rgb8(0xDC, 0xDC, 0xE4);
            image::Handle::from_rgba(SIZE, SIZE, render(color))
        })
        .clone()
}

/// Tightly packed RGBA8: a flat-coloured gear on a transparent field, with
/// coverage in the alpha channel.
fn render(color: Color) -> Vec<u8> {
    let [red, green, blue, _] = color.into_rgba8();

    let center = SIZE as f32 / 2.0;
    let tip = SIZE as f32 * TIP;
    let root = SIZE as f32 * ROOT;
    let hole = SIZE as f32 * HOLE;

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

                    if covers(px, py, tip, root, hole) {
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

/// Whether a point lies on the gear.
///
/// The outline is a step function of the angle: full radius across a tooth,
/// the root radius across the gap between two teeth.
fn covers(x: f32, y: f32, tip: f32, root: f32, hole: f32) -> bool {
    let distance = x.hypot(y);

    if distance < hole || distance > tip {
        return false;
    }
    if distance <= root {
        return true;
    }

    // Phase within the current tooth, in 0..1. Offsetting by half a duty cycle
    // centres a tooth on straight up, which reads as upright at small sizes.
    let turns = y.atan2(x) / std::f32::consts::TAU + 0.5;
    let phase = (turns * TEETH + TOOTH_DUTY / 2.0).rem_euclid(1.0);

    phase < TOOTH_DUTY
}
