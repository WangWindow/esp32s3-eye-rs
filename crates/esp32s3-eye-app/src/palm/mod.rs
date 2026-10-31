//! Palm ROI and palmprint HOG descriptor/database.
//!
//! Face detection/keypoints are kept separate in `crate::face`.

use crate::Rect;
use crate::detect::{largest_skin_blob, luma_at, screen_to_source, skin_grid};

#[derive(Default)]
struct PalmTracker {
    rect: Option<Rect>,
    missing: u8,
}

impl PalmTracker {
    fn update(&mut self, candidate: Option<Rect>) -> Option<Rect> {
        match candidate {
            Some(new) => {
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
                self.missing = self.missing.saturating_add(1);
                if self.missing > 4 {
                    self.rect = None;
                }
            }
        }
        self.rect
    }
}

// ── Palmprint descriptor ─────────────────────────────────────────────────────

const PALM_SIZE: usize = 48;
const PALM_CELL: usize = 8;
const PALM_CELLS: usize = PALM_SIZE / PALM_CELL; // 6
const PALM_BINS: usize = 8;
pub const PALM_DESC_LEN: usize = PALM_CELLS * PALM_CELLS * PALM_BINS; // 288
const PALM_MAX_TEMPLATES: usize = 8;
const PALM_MATCH_THRESHOLD: u32 = 700;

#[derive(Clone, Copy)]
pub struct PalmDescriptor {
    pub data: [u8; PALM_DESC_LEN],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PalmMatch {
    pub id: u8,
    /// 0..=1000 histogram-intersection score.
    pub score: u32,
}

#[derive(Clone, Copy)]
struct PalmTemplate {
    id: u8,
    data: [u8; PALM_DESC_LEN],
}

pub struct PalmDatabase {
    templates: [Option<PalmTemplate>; PALM_MAX_TEMPLATES],
    next_id: u8,
}

impl Default for PalmDatabase {
    fn default() -> Self {
        Self::new()
    }
}

impl PalmDatabase {
    pub const fn new() -> Self {
        Self {
            templates: [None; PALM_MAX_TEMPLATES],
            next_id: 1,
        }
    }

    pub fn count(&self) -> usize {
        self.templates.iter().flatten().count()
    }

    pub fn enroll(&mut self, descriptor: PalmDescriptor) -> Option<u8> {
        let slot = self.templates.iter().position(Option::is_none)?;
        let id = self.next_id.max(1);
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.templates[slot] = Some(PalmTemplate {
            id,
            data: descriptor.data,
        });
        Some(id)
    }

    pub fn recognize(&self, descriptor: PalmDescriptor) -> Option<PalmMatch> {
        let mut best: Option<PalmMatch> = None;
        for template in self.templates.iter().flatten() {
            let score = palm_score(&descriptor.data, &template.data);
            if best.map(|current| score > current.score).unwrap_or(true) {
                best = Some(PalmMatch {
                    id: template.id,
                    score,
                });
            }
        }
        best.filter(|result| result.score >= PALM_MATCH_THRESHOLD)
    }

    pub fn delete_last(&mut self) -> usize {
        let mut last: Option<(usize, u8)> = None;
        for (index, template) in self.templates.iter().enumerate() {
            if let Some(template) = template
                && last.map(|(_, id)| template.id >= id).unwrap_or(true)
            {
                last = Some((index, template.id));
            }
        }
        if let Some((index, _)) = last {
            self.templates[index] = None;
        }
        self.count()
    }
}

/// Find a stable palm ROI from the largest skin-coloured region.
pub fn palm_roi(frame: &[u8]) -> Option<Rect> {
    let mask = skin_grid(frame);
    let rect = largest_skin_blob(&mask)?;
    let side = (rect.w.min(rect.h) * 7 / 10)
        .max(32)
        .min(rect.w.min(rect.h));
    let x = rect.x + (rect.w - side) / 2;
    let y = rect.y + (rect.h - side) / 2;
    Some(Rect::new(x, y, side, side))
}

/// Build a 288-byte HOG-style palmprint descriptor from the ROI.
pub fn palm_descriptor(frame: &[u8], roi: Rect) -> Option<PalmDescriptor> {
    if roi.w < 24 || roi.h < 24 {
        return None;
    }

    let side = roi.w.min(roi.h) as usize;
    let mut gray = [0u8; PALM_SIZE * PALM_SIZE];
    for y in 0..PALM_SIZE {
        let screen_y = roi.y as usize + y * side / PALM_SIZE;
        for x in 0..PALM_SIZE {
            let screen_x = roi.x as usize + x * side / PALM_SIZE;
            let (source_x, source_y) = screen_to_source(screen_x as u16, screen_y as u16);
            gray[y * PALM_SIZE + x] = luma_at(frame, source_x, source_y);
        }
    }

    let mut hist = [[0u32; PALM_BINS]; PALM_CELLS * PALM_CELLS];
    for y in 1..PALM_SIZE - 1 {
        for x in 1..PALM_SIZE - 1 {
            let p00 = gray[(y - 1) * PALM_SIZE + x - 1] as i32;
            let p01 = gray[(y - 1) * PALM_SIZE + x] as i32;
            let p02 = gray[(y - 1) * PALM_SIZE + x + 1] as i32;
            let p10 = gray[y * PALM_SIZE + x - 1] as i32;
            let p12 = gray[y * PALM_SIZE + x + 1] as i32;
            let p20 = gray[(y + 1) * PALM_SIZE + x - 1] as i32;
            let p21 = gray[(y + 1) * PALM_SIZE + x] as i32;
            let p22 = gray[(y + 1) * PALM_SIZE + x + 1] as i32;

            let gx = (p02 + 2 * p12 + p22) - (p00 + 2 * p10 + p20);
            let gy = (p20 + 2 * p21 + p22) - (p00 + 2 * p01 + p02);
            let magnitude = (gx.abs() + gy.abs()) as u32;
            let bin = orientation_bin(gx, gy);
            let cell = (y / PALM_CELL) * PALM_CELLS + x / PALM_CELL;
            hist[cell][bin] += magnitude;
        }
    }

    let mut data = [0u8; PALM_DESC_LEN];
    for (cell, bins) in hist.iter().enumerate() {
        let sum: u32 = bins.iter().sum();
        if sum > 0 {
            for (bin, value) in bins.iter().enumerate() {
                data[cell * PALM_BINS + bin] = (*value * 255).checked_div(sum).unwrap_or(0) as u8;
            }
        }
    }

    Some(PalmDescriptor { data })
}

fn orientation_bin(gx: i32, gy: i32) -> usize {
    let ax = gx.unsigned_abs();
    let ay = gy.unsigned_abs();
    if ax == 0 {
        return if gy >= 0 { 3 } else { 4 };
    }

    let major = ax.max(ay);
    let minor = ax.min(ay);
    let sub = if minor * 5 < major * 2 {
        0
    } else if minor < major {
        1
    } else if minor * 2 < major * 5 {
        2
    } else {
        3
    };

    if (gx >= 0) == (gy >= 0) { sub } else { 7 - sub }
}

fn palm_score(a: &[u8; PALM_DESC_LEN], b: &[u8; PALM_DESC_LEN]) -> u32 {
    let intersection: u32 = a
        .iter()
        .zip(b.iter())
        .map(|(left, right)| (*left as u32).min(*right as u32))
        .sum();
    let total_a: u32 = a.iter().map(|value| *value as u32).sum();
    let total_b: u32 = b.iter().map(|value| *value as u32).sum();
    intersection * 1000 / total_a.max(total_b).max(1)
}

#[derive(Default)]
pub struct PalmAnalyzer {
    tracker: PalmTracker,
}

impl PalmAnalyzer {
    pub fn analyze(&mut self, frame: &[u8]) -> Option<Rect> {
        self.tracker.update(palm_roi(frame))
    }
}
