//! Motion, skin-colour face candidate and five-point keypoint analysis.

use crate::geometry::{Point, Rect};
use crate::modes::Mode;
use esp32s3_eye_board::{
    CAM_HEIGHT, CAM_ROW_BYTES, CAM_VFLIP, CAM_VIEW_HEIGHT, CAM_WIDTH, CAM_Y_OFFSET, CROP_X,
    STATUS_BAR_HEIGHT,
};

/// Grid used by the motion and skin-colour detectors.
const GRID_CELL: usize = 16;
const GRID_W: usize = CAM_WIDTH / GRID_CELL; // 20
const GRID_H: usize = CAM_HEIGHT / GRID_CELL; // 15
const GRID_CELLS: usize = GRID_W * GRID_H; // 300
/// Analyse every other pixel in both axes: 64 samples per 16×16 cell.
const SAMPLE_STEP: usize = 2;
const SAMPLES_PER_CELL: usize = (GRID_CELL / SAMPLE_STEP) * (GRID_CELL / SAMPLE_STEP);

/// Result of analysing one camera frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameAnalysis {
    pub motion: bool,
    pub face: Option<Rect>,
    /// Five facial keypoints: left eye, right eye, nose, mouth left, mouth right.
    pub keypoints: Option<[Point; 5]>,
}

pub struct MotionDetector {
    background: [u8; GRID_CELLS],
    primed: bool,
}

impl Default for MotionDetector {
    fn default() -> Self {
        Self {
            background: [0; GRID_CELLS],
            primed: false,
        }
    }
}

impl MotionDetector {
    fn update(&mut self, luma: &[u8; GRID_CELLS]) -> bool {
        let mut changed = 0usize;

        if self.primed {
            for (index, current) in luma.iter().enumerate() {
                // Compare with a slowly adapting background instead of the
                // previous frame. This rejects sensor noise and slow lighting
                // changes while still catching moving objects.
                let background = self.background[index] as i16;
                if (*current as i16 - background).abs() > 20 {
                    changed += 1;
                }
            }
        }

        for (background, current) in self.background.iter_mut().zip(luma.iter()) {
            *background = ((*background as u16 * 3 + *current as u16) / 4) as u8;
        }
        self.primed = true;

        // A handful of changed cells can be sensor noise; require a visible
        // moving region (roughly 3 % of the frame).
        changed >= 8
    }
}

#[derive(Default)]
pub struct FaceTracker {
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

#[derive(Default)]
pub struct Analyzer {
    motion: MotionDetector,
    face: FaceTracker,
    keypoints: KeypointTracker,
}

/// Analyse a frame according to the active mode.
pub fn analyse(frame: &[u8], mode: Mode, analyzer: &mut Analyzer) -> FrameAnalysis {
    match mode {
        Mode::Motion => FrameAnalysis {
            motion: analyzer.motion.update(&luma_grid(frame)),
            face: None,
            keypoints: None,
        },
        Mode::Face => {
            let face = analyzer.face.update(largest_skin_blob(&skin_grid(frame)));
            let keypoints = analyzer
                .keypoints
                .update(face.and_then(|rect| estimate_keypoints(frame, rect)));
            FrameAnalysis {
                motion: false,
                face,
                keypoints,
            }
        }
        _ => FrameAnalysis::default(),
    }
}

/// Average luma of every 16×16 cell, sampled on a 2×2 grid.
fn luma_grid(frame: &[u8]) -> [u8; GRID_CELLS] {
    let mut sums = [0u32; GRID_CELLS];

    for y in (0..CAM_HEIGHT).step_by(SAMPLE_STEP) {
        let row = &frame[y * CAM_ROW_BYTES..(y + 1) * CAM_ROW_BYTES];
        let cell_row = (y / GRID_CELL) * GRID_W;
        for x in (0..CAM_WIDTH).step_by(SAMPLE_STEP) {
            let i = x * 2;
            let pixel = u16::from_be_bytes([row[i], row[i + 1]]);
            let green = ((pixel >> 5) & 0x3F) as u8;
            sums[cell_row + x / GRID_CELL] += ((green << 2) | (green >> 4)) as u32;
        }
    }

    let mut result = [0u8; GRID_CELLS];
    for (result_cell, sum) in result.iter_mut().zip(sums.iter()) {
        *result_cell = (*sum / SAMPLES_PER_CELL as u32) as u8;
    }
    result
}

/// Skin-colour mask in YCbCr (Cb 77..=127, Cr 133..=173), sampled 2×2.
fn skin_grid(frame: &[u8]) -> [bool; GRID_CELLS] {
    let mut counts = [0u16; GRID_CELLS];

    for y in (0..CAM_HEIGHT).step_by(SAMPLE_STEP) {
        let row = &frame[y * CAM_ROW_BYTES..(y + 1) * CAM_ROW_BYTES];
        let cell_row = (y / GRID_CELL) * GRID_W;

        for x in (0..CAM_WIDTH).step_by(SAMPLE_STEP) {
            let i = x * 2;
            let pixel = u16::from_be_bytes([row[i], row[i + 1]]);
            let r5 = ((pixel >> 11) & 0x1F) as u8;
            let g6 = ((pixel >> 5) & 0x3F) as u8;
            let b5 = (pixel & 0x1F) as u8;
            let r = (r5 << 3) | (r5 >> 2);
            let g = (g6 << 2) | (g6 >> 4);
            let b = (b5 << 3) | (b5 >> 2);
            let max = r.max(g).max(b);
            let min = r.min(g).min(b);

            let skin = r > 95
                && g > 40
                && b > 20
                && max - min > 15
                && r.abs_diff(g) > 15
                && r > g
                && r > b;

            if skin {
                counts[cell_row + x / GRID_CELL] += 1;
            }
        }
    }

    let mut mask = [false; GRID_CELLS];
    for index in 0..GRID_CELLS {
        // At least 15 % of the 64 sampled pixels in a cell must be skin colour.
        mask[index] = counts[index] >= 10;
    }

    // 3×3 majority filter: a skin cell needs at least two skin neighbours.
    // This removes isolated speckles without eroding the main face blob.
    let mut filtered = [false; GRID_CELLS];
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            let index = y * GRID_W + x;
            if !mask[index] {
                continue;
            }
            let mut neighbours = 0;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx >= 0
                        && nx < GRID_W as i32
                        && ny >= 0
                        && ny < GRID_H as i32
                        && mask[ny as usize * GRID_W + nx as usize]
                    {
                        neighbours += 1;
                    }
                }
            }
            filtered[index] = neighbours >= 2;
        }
    }
    filtered
}

/// Find the largest 4-connected skin region and return its screen-space box.
fn largest_skin_blob(mask: &[bool; GRID_CELLS]) -> Option<Rect> {
    const MIN_CELLS: usize = 4;
    const MAX_CELLS: usize = 120;

    let mut visited = [false; GRID_CELLS];
    let mut queue = [0u8; GRID_CELLS];
    // score, x0, y0, x1, y1
    let mut best: Option<(i32, usize, usize, usize, usize)> = None;

    for start in 0..GRID_CELLS {
        if !mask[start] || visited[start] {
            continue;
        }

        let mut head = 0usize;
        let mut tail = 0usize;
        queue[tail] = start as u8;
        tail += 1;
        visited[start] = true;

        let mut area = 0usize;
        let mut x0 = GRID_W;
        let mut y0 = GRID_H;
        let mut x1 = 0usize;
        let mut y1 = 0usize;

        while head < tail {
            let cell = queue[head] as usize;
            head += 1;
            area += 1;

            let cx = cell % GRID_W;
            let cy = cell / GRID_W;
            x0 = x0.min(cx);
            y0 = y0.min(cy);
            x1 = x1.max(cx);
            y1 = y1.max(cy);

            let push = |nx: usize,
                        ny: usize,
                        visited: &mut [bool; GRID_CELLS],
                        queue: &mut [u8; GRID_CELLS],
                        tail: &mut usize| {
                let index = ny * GRID_W + nx;
                if !visited[index] && mask[index] {
                    visited[index] = true;
                    queue[*tail] = index as u8;
                    *tail += 1;
                }
            };

            if cx > 0 {
                push(cx - 1, cy, &mut visited, &mut queue, &mut tail);
            }
            if cx + 1 < GRID_W {
                push(cx + 1, cy, &mut visited, &mut queue, &mut tail);
            }
            if cy > 0 {
                push(cx, cy - 1, &mut visited, &mut queue, &mut tail);
            }
            if cy + 1 < GRID_H {
                push(cx, cy + 1, &mut visited, &mut queue, &mut tail);
            }
        }

        if (MIN_CELLS..=MAX_CELLS).contains(&area) {
            let width = x1 - x0 + 1;
            let height = y1 - y0 + 1;

            // Reject very long/thin regions and sparse checkerboard blobs.
            // A face is roughly square-ish and reasonably solid.
            if width * 2 >= height && height * 2 >= width && area * 100 / (width * height) >= 30 {
                let cx = (x0 + width / 2) as i32;
                let cy = (y0 + height / 2) as i32;
                let center_distance =
                    (cx - (GRID_W / 2) as i32).abs() + (cy - (GRID_H / 2) as i32).abs();
                let score = area as i32 * 4 - center_distance;

                if best
                    .map(|(best_score, ..)| score > best_score)
                    .unwrap_or(true)
                {
                    best = Some((score, x0, y0, x1, y1));
                }
            }
        }
    }

    best.map(|(_, x0, y0, x1, y1)| {
        let left = (x0 * GRID_CELL).saturating_sub(CROP_X);
        let right = ((x1 + 1) * GRID_CELL).saturating_sub(CROP_X);

        // Source-frame Y -> screen Y. The preview centre-crops CAM_Y_OFFSET
        // rows, starts below the status bar, and may be vertically flipped.
        let source_top = y0 * GRID_CELL;
        let source_bottom = (y1 + 1) * GRID_CELL;
        let (top, bottom) = if CAM_VFLIP {
            // screen_y = STATUS_BAR_HEIGHT + CAM_Y_OFFSET + CAM_VIEW_HEIGHT - 1 - source_y
            let screen_sum = STATUS_BAR_HEIGHT + CAM_Y_OFFSET + CAM_VIEW_HEIGHT - 1;
            (screen_sum - source_bottom, screen_sum - source_top)
        } else {
            (
                source_top + STATUS_BAR_HEIGHT - CAM_Y_OFFSET,
                source_bottom + STATUS_BAR_HEIGHT - CAM_Y_OFFSET,
            )
        };

        let left = left.min(esp32s3_eye_board::LCD_WIDTH) as u16;
        let right = right.min(esp32s3_eye_board::LCD_WIDTH) as u16;
        let top = top.clamp(STATUS_BAR_HEIGHT, esp32s3_eye_board::LCD_HEIGHT) as u16;
        let bottom = bottom.clamp(STATUS_BAR_HEIGHT, esp32s3_eye_board::LCD_HEIGHT) as u16;

        let width = right.saturating_sub(left);
        let height = bottom.saturating_sub(top);

        // Add a little padding, but keep the box below the status bar.
        let pad = 4u16;
        let x = left.saturating_sub(pad);
        let y = top.saturating_sub(pad).max(STATUS_BAR_HEIGHT as u16);
        let w = (width + pad * 2).min(esp32s3_eye_board::LCD_WIDTH as u16 - x);
        let h = (height + pad * 2).min(esp32s3_eye_board::LCD_HEIGHT as u16 - y);

        Rect::new(x, y, w, h)
    })
}

/// Map a screen-space point to source-frame coordinates.
fn screen_to_source(x: u16, y: u16) -> (usize, usize) {
    let screen_x = (x as usize).min(esp32s3_eye_board::LCD_WIDTH - 1);
    let screen_y = (y as usize).clamp(STATUS_BAR_HEIGHT, esp32s3_eye_board::LCD_HEIGHT - 1);
    let view_row = screen_y - STATUS_BAR_HEIGHT;
    let source_y = if CAM_VFLIP {
        CAM_Y_OFFSET + (CAM_VIEW_HEIGHT - 1 - view_row)
    } else {
        CAM_Y_OFFSET + view_row
    };
    let source_x = (screen_x + CROP_X).min(CAM_WIDTH - 1);
    (source_x, source_y)
}

#[inline]
fn luma_at(frame: &[u8], x: usize, y: usize) -> u8 {
    let index = y * CAM_ROW_BYTES + x * 2;
    let pixel = u16::from_be_bytes([frame[index], frame[index + 1]]);
    let green = ((pixel >> 5) & 0x3F) as u8;
    (green << 2) | (green >> 4)
}

/// Find the darkest small region inside a screen-space rectangle.
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
