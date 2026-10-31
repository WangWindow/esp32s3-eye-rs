//! Application state machine: preview, menu, overlays and SD screen.

use embassy_time::{Duration, Instant, Timer};
use esp_println::println;

use esp32s3_eye_board::Board;
use esp32s3_eye_board::buttons::Button;
use esp32s3_eye_board::storage::SelfTest;
#[cfg(feature = "palm")]
use static_cell::StaticCell;

use crate::Mode;
#[cfg(feature = "palm")]
use crate::display::PalmNotice;
use crate::display::{Overlay, Ui};
#[cfg(feature = "face")]
use crate::face::FaceAnalyzer;
#[cfg(feature = "palm")]
use crate::palm::{PalmAnalyzer, PalmDatabase, palm_descriptor, palm_roi};

#[cfg(feature = "palm")]
static PALM_DB: StaticCell<PalmDatabase> = StaticCell::new();

#[embassy_executor::task]
pub async fn task(mut board: Board) {
    let mut ui = Ui::new();
    ui.show_standby(&mut board.display);

    println!("Initialising OV2640…");
    if let Err(error) = board.init_camera().await {
        println!("OV2640 init failed: {:?}", error);
        ui.show_error(&mut board.display, "NO OV2640 / SCCB");
        loop {
            Timer::after(Duration::from_secs(1)).await;
        }
    }
    println!("OV2640 ready (QVGA RGB565)");

    board.init_storage().await;
    #[cfg(feature = "palm")]
    let palm_db = PALM_DB.init(PalmDatabase::new());
    #[cfg(feature = "palm")]
    let mut palm_notice: Option<(PalmNotice, Instant)> = None;
    let mut storage_test: Option<SelfTest> = None;
    let mut storage_error: Option<&'static str> = None;
    let mut storage_tested = false;

    #[cfg(feature = "face")]
    let mut face_analyzer = FaceAnalyzer::default();
    #[cfg(feature = "palm")]
    let mut palm_analyzer = PalmAnalyzer::default();

    let mut mode = Mode::Camera;
    let mut paused = false;
    let mut last_scan = Instant::now();
    let mut last_frame_time = Instant::now();
    let mut last_fps_update = Instant::now();
    let mut first_frame = true;
    let mut fps = 0u32;
    let mut displayed_fps = 0u32;
    let mut frame_count = 0u32;

    Timer::after(Duration::from_millis(300)).await;
    ui.draw_status_bar(&mut board.display, mode, 0);

    loop {
        if last_scan.elapsed() >= Duration::from_millis(25) {
            last_scan = Instant::now();

            if let Some(button) = board.poll_button() {
                #[cfg(feature = "palm")]
                let palm_mode = mode == Mode::Palm;
                #[cfg(not(feature = "palm"))]
                let palm_mode = false;

                #[cfg(feature = "palm")]
                if palm_mode {
                    match button {
                        Button::Boot => {
                            mode = mode.next();
                            paused = false;
                            palm_notice = None;
                        }
                        Button::Up => {
                            let notice = enroll_palm(board.frame, palm_db);
                            println!("palm enroll: {:?}", notice);
                            palm_notice = Some((notice, Instant::now() + Duration::from_secs(2)));
                            board.camera_stop();
                            ui.show_palm_notice(&mut board.display, notice);
                        }
                        Button::Play => {
                            let notice = recognize_palm(board.frame, palm_db);
                            println!("palm recognize: {:?}", notice);
                            palm_notice = Some((notice, Instant::now() + Duration::from_secs(2)));
                            board.camera_stop();
                            ui.show_palm_notice(&mut board.display, notice);
                        }
                        Button::Down => {
                            let count = palm_db.delete_last();
                            let notice = PalmNotice::Deleted(count);
                            println!("palm delete: {} templates", count);
                            palm_notice = Some((notice, Instant::now() + Duration::from_secs(2)));
                            board.camera_stop();
                            ui.show_palm_notice(&mut board.display, notice);
                        }
                    }
                }

                if !palm_mode {
                    match button {
                        Button::Up | Button::Boot => {
                            mode = mode.next();
                            paused = false;
                        }
                        Button::Down => {
                            mode = mode.previous();
                            paused = false;
                        }
                        Button::Play => {
                            if mode != Mode::Standby && mode != Mode::Storage {
                                paused = !paused;
                                if paused {
                                    ui.show_pause(&mut board.display);
                                }
                            }
                        }
                    }
                }

                println!(
                    "button={:?} mode={:?} paused={} adc={}",
                    button,
                    mode,
                    paused,
                    board.buttons.last_raw()
                );

                #[cfg(feature = "face")]
                {
                    face_analyzer = FaceAnalyzer::default();
                }
                #[cfg(feature = "palm")]
                {
                    palm_analyzer = PalmAnalyzer::default();
                    if mode != Mode::Palm {
                        palm_notice = None;
                    }
                }
                if mode == Mode::Storage {
                    storage_tested = false;
                }

                #[cfg(feature = "palm")]
                let notice_active = palm_notice.is_some();
                #[cfg(not(feature = "palm"))]
                let notice_active = false;

                if mode == Mode::Standby || mode == Mode::Storage || paused || notice_active {
                    board.camera_stop();
                }

                match mode {
                    Mode::Standby => ui.show_standby(&mut board.display),
                    #[cfg(feature = "palm")]
                    Mode::Palm => {
                        if palm_notice.is_none() {
                            ui.draw_status_bar(&mut board.display, mode, fps);
                            displayed_fps = fps;
                        }
                    }
                    Mode::Storage => {
                        let capacity = board.sd_capacity_mib().unwrap_or(0);
                        let test = storage_test.unwrap_or_default();
                        let error = storage_error.or(if board.storage.is_none() {
                            Some("NO SD CARD")
                        } else {
                            None
                        });
                        ui.show_storage(
                            &mut board.display,
                            capacity,
                            test.read_ok,
                            test.write_ok,
                            error,
                        );
                    }
                    _ => {
                        ui.draw_status_bar(&mut board.display, mode, fps);
                        displayed_fps = fps;
                    }
                }
            }
        }

        #[cfg(feature = "palm")]
        if let Some((_, until)) = palm_notice {
            if Instant::now() < until {
                board.led_off();
                Timer::after(Duration::from_millis(50)).await;
                continue;
            }
            palm_notice = None;
            board.camera_start();
            ui.draw_status_bar(&mut board.display, mode, fps);
            displayed_fps = fps;
        }

        if mode == Mode::Standby {
            board.led_off();
            Timer::after(Duration::from_millis(50)).await;
            continue;
        }

        if paused {
            board.led_off();
            Timer::after(Duration::from_millis(50)).await;
            continue;
        }

        if mode == Mode::Storage {
            board.led_off();
            board.camera_stop();

            if !storage_tested {
                match board.sd_self_test().await {
                    Some(result) => {
                        println!("SD test: read={} write={}", result.read_ok, result.write_ok);
                        storage_test = Some(result);
                        storage_error = None;
                    }
                    None => {
                        println!("SD test unavailable");
                        storage_error = Some("NO SD CARD / ERROR");
                    }
                }

                let capacity = board.sd_capacity_mib().unwrap_or(0);
                let test = storage_test.unwrap_or_default();
                ui.show_storage(
                    &mut board.display,
                    capacity,
                    test.read_ok,
                    test.write_ok,
                    storage_error,
                );
                storage_tested = true;
            }

            Timer::after(Duration::from_millis(50)).await;
            continue;
        }

        board.camera_start();
        let capture_start = Instant::now();

        if board.capture_frame().await.is_some() {
            let capture_ms = capture_start.elapsed().as_millis();
            frame_count = frame_count.wrapping_add(1);

            let now = Instant::now();
            if first_frame {
                first_frame = false;
            } else {
                let elapsed_ms = (now - last_frame_time).as_millis().max(1);
                fps = (1000 / elapsed_ms).min(999) as u32;
            }
            last_frame_time = now;

            if frame_count.is_multiple_of(60) {
                println!("frames={} fps={} mode={:?}", frame_count, fps, mode);
            }

            if last_fps_update.elapsed() >= Duration::from_secs(1) {
                if fps != displayed_fps {
                    ui.update_fps(&mut board.display, fps);
                    displayed_fps = fps;
                }
                last_fps_update = Instant::now();
            }

            let analyse_start = Instant::now();
            let (face, keypoints, palm) = match mode {
                #[cfg(feature = "face")]
                Mode::Face => {
                    let result = face_analyzer.analyze(board.frame);
                    (result.face, result.keypoints, None)
                }
                #[cfg(feature = "palm")]
                Mode::Palm => (None, None, palm_analyzer.analyze(board.frame)),
                _ => (None, None, None),
            };
            let analyse_ms = analyse_start.elapsed().as_millis();
            #[cfg(feature = "face")]
            if mode == Mode::Face && frame_count.is_multiple_of(60) {
                println!("  face={:?} keypoints={:?}", face, keypoints);
            }
            let overlay = Overlay {
                face,
                keypoints,
                palm,
            };

            let draw_start = Instant::now();
            ui.draw_rgb565_frame(&mut board.display, board.frame, overlay);
            let draw_ms = draw_start.elapsed().as_millis();

            if frame_count.is_multiple_of(60) {
                println!(
                    "  capture={}ms analyse={}ms draw={}ms restarts={}",
                    capture_ms,
                    analyse_ms,
                    draw_ms,
                    board.camera_restarts()
                );
            }

            board.led_on();
        } else {
            Timer::after(Duration::from_millis(10)).await;
        }
    }
}

#[cfg(feature = "palm")]
fn enroll_palm(frame: &[u8], database: &mut PalmDatabase) -> PalmNotice {
    let Some(roi) = palm_roi(frame) else {
        return PalmNotice::NoPalm;
    };
    let Some(descriptor) = palm_descriptor(frame, roi) else {
        return PalmNotice::NoPalm;
    };
    match database.enroll(descriptor) {
        Some(id) => PalmNotice::Enrolled(id),
        None => PalmNotice::NoSpace,
    }
}

#[cfg(feature = "palm")]
fn recognize_palm(frame: &[u8], database: &PalmDatabase) -> PalmNotice {
    let Some(roi) = palm_roi(frame) else {
        return PalmNotice::NoPalm;
    };
    let Some(descriptor) = palm_descriptor(frame, roi) else {
        return PalmNotice::NoPalm;
    };
    match database.recognize(descriptor) {
        Some(result) => PalmNotice::Match {
            id: result.id,
            score: result.score,
        },
        None => PalmNotice::NoMatch,
    }
}
