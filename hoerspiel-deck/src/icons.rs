//! Key images and their state variants (active, error, dimmed).

use crate::layout::LAYOUT;
use image::{DynamicImage, Rgba};

const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);
const RED: Rgba<u8> = Rgba([230, 20, 20, 255]);

/// All pictures one key can show.
pub struct KeyImages {
    pub normal: DynamicImage,
    /// White frame: this track is selected.
    pub active: DynamicImage,
    /// Red frame: an action failed, or recording is running (blinking).
    pub alert: DynamicImage,
    /// Darkened: Audacity is not connected.
    pub dimmed: DynamicImage,
}

pub fn load_all() -> Vec<KeyImages> {
    LAYOUT
        .iter()
        .map(|key| {
            let normal = image::load_from_memory(key.icon).expect("embedded icon is a valid PNG");
            KeyImages {
                active: with_frame(&normal, WHITE, 12),
                alert: with_frame(&normal, RED, 16),
                dimmed: darkened(&normal, 35),
                normal,
            }
        })
        .collect()
}

fn with_frame(image: &DynamicImage, color: Rgba<u8>, thickness: u32) -> DynamicImage {
    let mut img = image.to_rgba8();
    let (w, h) = img.dimensions();
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        if x < thickness || y < thickness || x >= w - thickness || y >= h - thickness {
            *pixel = color;
        }
    }
    DynamicImage::ImageRgba8(img)
}

fn darkened(image: &DynamicImage, percent: u16) -> DynamicImage {
    let mut img = image.to_rgba8();
    for pixel in img.pixels_mut() {
        for channel in &mut pixel.0[..3] {
            *channel = (*channel as u16 * percent / 100) as u8;
        }
    }
    DynamicImage::ImageRgba8(img)
}
