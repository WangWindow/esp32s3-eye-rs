//! Face-candidate and five-point keypoint heuristics.
//!
//! This module is intentionally independent from palmprint code. Shared
//! Shared skin-mask/blob helpers live in `crate::detect`.

use crate::detect::{largest_skin_blob, luma_at, screen_to_source, skin_grid};
use crate::{Point, Rect};
use esp32s3_eye_board::STATUS_BAR_HEIGHT;

#[derive(Default)]
struct FaceTracker {
    rect: Option<Rect>,
    missing: u8,
    hits: u8,
}

impl FaceTracker {
    fn update(&mut self, candidate: Option<Rect>) -> Option<Rect> {
        match candidate {
            Some(new) => {
                self.hits = self.hits.saturating_add(1);
                // Require two consecutive detections before showing a new box.
                // This suppresses one-frame skin-coloured false positives.
                if self.rect.is_none() && self.hits < 2 {
                    return None;
                }

                // Light temporal smoothing makes the box much less jittery.
                let smoothed = if let Some(old) = self.rect {
                    Rect::new(
                        ((old.x as u32 * 3 + new.x as u32) / 4) as u16,
                        ((old.y as u32 * 3 + new.y as u32) / 4) as u16,
                        ((old.w as u32 * 3 + new.w as u32) / 4) as u16,
                        ((old.h as u32 * 3 + new.h as u32) / 4) as u16,
                    )
                } else {
                    new
                };
                self.rect = Some(smoothed);
                self.missing = 0;
            }
            None => {
                // Hold the box briefly through a missed detection, then drop it.
                self.hits = 0;
                self.missing = self.missing.saturating_add(1);
                if self.missing > 5 {
                    self.rect = None;
                }
            }
        }
        self.rect
    }
}

#[derive(Default)]
struct KeypointTracker {
    points: Option<[Point; 5]>,
}

impl KeypointTracker {
    fn update(&mut self, candidate: Option<[Point; 5]>) -> Option<[Point; 5]> {
        match candidate {
            Some(new) => {
                let smoothed = if let Some(old) = self.points {
                    let mut out = new;
                    for (old, new) in old.iter().zip(out.iter_mut()) {
                        new.x = ((old.x as u32 * 3 + new.x as u32) / 4) as u16;
                        new.y = ((old.y as u32 * 3 + new.y as u32) / 4) as u16;
                    }
                    out
                } else {
                    new
                };
                self.points = Some(smoothed);
                Some(smoothed)
            }
            None => {
                self.points = None;
                None
            }
        }
    }
}

fn dark_centroid(frame: &[u8], rect: Rect) -> Option<(u16, u16)> {
    let x0 = rect.x as usize;
    let x1 = (rect.x as usize + rect.w as usize).min(esp32s3_eye_board::LCD_WIDTH);
    let y0 = (rect.y as usize).max(STATUS_BAR_HEIGHT);
    let y1 = (rect.y as usize + rect.h as usize).min(esp32s3_eye_board::LCD_HEIGHT);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }

    let mut minimum = 255u8;
    let mut y = y0;
    while y < y1 {
        let mut x = x0;
        while x < x1 {
            let (sx, sy) = screen_to_source(x as u16, y as u16);
            minimum = minimum.min(luma_at(frame, sx, sy));
            x += 2;
        }
        y += 2;
    }

    let threshold = minimum.saturating_add(18);
    let mut sum_x = 0u32;
    let mut sum_y = 0u32;
    let mut count = 0u32;
    y = y0;
    while y < y1 {
        let mut x = x0;
        while x < x1 {
            let (sx, sy) = screen_to_source(x as u16, y as u16);
            if luma_at(frame, sx, sy) <= threshold {
                sum_x += x as u32;
                sum_y += y as u32;
                count += 1;
            }
            x += 2;
        }
        y += 2;
    }

    if count < 4 {
        None
    } else {
        Some(((sum_x / count) as u16, (sum_y / count) as u16))
    }
}

fn region(x: u16, y: u16, w: u16, h: u16) -> Rect {
    Rect::new(x, y, w, h)
}

/// Estimate five facial keypoints from the tracked face box.
///
/// This is a fast, model-free approximation: eyes and mouth are the darkest
/// local regions, the nose is the box centre. It is intentionally cheap enough
/// to run at the display frame rate.
fn estimate_keypoints(frame: &[u8], face: Rect) -> Option<[Point; 5]> {
    if face.w < 20 || face.h < 20 {
        return None;
    }

    let left_eye_box = region(
        face.x + face.w * 12 / 100,
        face.y + face.h * 22 / 100,
        face.w * 28 / 100,
        face.h * 30 / 100,
    );
    let right_eye_box = region(
        face.x + face.w * 60 / 100,
        face.y + face.h * 22 / 100,
        face.w * 28 / 100,
        face.h * 30 / 100,
    );
    let mouth_box = region(
        face.x + face.w * 25 / 100,
        face.y + face.h * 65 / 100,
        face.w * 50 / 100,
        face.h * 25 / 100,
    );

    let left_eye = dark_centroid(frame, left_eye_box).unwrap_or((
        left_eye_box.x + left_eye_box.w / 2,
        left_eye_box.y + left_eye_box.h / 2,
    ));
    let right_eye = dark_centroid(frame, right_eye_box).unwrap_or((
        right_eye_box.x + right_eye_box.w / 2,
        right_eye_box.y + right_eye_box.h / 2,
    ));
    let mouth = dark_centroid(frame, mouth_box)
        .unwrap_or((face.x + face.w / 2, mouth_box.y + mouth_box.h / 2));

    Some([
        Point {
            x: left_eye.0,
            y: left_eye.1,
        },
        Point {
            x: right_eye.0,
            y: right_eye.1,
        },
        Point {
            x: face.x + face.w / 2,
            y: face.y + face.h * 55 / 100,
        },
        Point {
            x: face.x + face.w * 35 / 100,
            y: mouth.1,
        },
        Point {
            x: face.x + face.w * 65 / 100,
            y: mouth.1,
        },
    ])
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FaceResult {
    pub face: Option<Rect>,
    pub keypoints: Option<[Point; 5]>,
}

#[derive(Default)]
pub struct FaceAnalyzer {
    face: FaceTracker,
    keypoints: KeypointTracker,
}

impl FaceAnalyzer {
    pub fn analyze(&mut self, frame: &[u8]) -> FaceResult {
        let face = self.face.update(largest_skin_blob(&skin_grid(frame)));
        let keypoints = self
            .keypoints
            .update(face.and_then(|rect| estimate_keypoints(frame, rect)));
        FaceResult { face, keypoints }
    }
}
