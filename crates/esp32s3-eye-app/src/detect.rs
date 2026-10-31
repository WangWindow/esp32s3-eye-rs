//! Shared low-level skin-mask/blob and pixel helpers.
//!
//! Algorithm-specific code lives in `crate::face` and `crate::palm`.

use esp32s3_eye_board::{
    CAM_HEIGHT, CAM_ROW_BYTES, CAM_VFLIP, CAM_VIEW_HEIGHT, CAM_WIDTH, CAM_Y_OFFSET, CROP_X,
    STATUS_BAR_HEIGHT,
};

use crate::Rect;

/// Grid used by the skin-colour/blob detector.
const GRID_CELL: usize = 16;
const GRID_W: usize = CAM_WIDTH / GRID_CELL; // 20
const GRID_H: usize = CAM_HEIGHT / GRID_CELL; // 15
const GRID_CELLS: usize = GRID_W * GRID_H; // 300
/// Analyse every other pixel in both axes: 64 samples per 16×16 cell.
const SAMPLE_STEP: usize = 2;

/// Skin-colour mask in YCbCr (Cb 77..=127, Cr 133..=173), sampled 2×2.
pub(crate) fn skin_grid(frame: &[u8]) -> [bool; GRID_CELLS] {
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
pub(crate) fn largest_skin_blob(mask: &[bool; GRID_CELLS]) -> Option<Rect> {
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
pub(crate) fn screen_to_source(x: u16, y: u16) -> (usize, usize) {
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
pub(crate) fn luma_at(frame: &[u8], x: usize, y: usize) -> u8 {
    let index = y * CAM_ROW_BYTES + x * 2;
    let pixel = u16::from_be_bytes([frame[index], frame[index + 1]]);
    let green = ((pixel >> 5) & 0x3F) as u8;
    (green << 2) | (green >> 4)
}
