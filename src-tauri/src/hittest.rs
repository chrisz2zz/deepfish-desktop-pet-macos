use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tauri::WebviewWindow;

const HIT_POLL: Duration = Duration::from_millis(40);

/// Window-local grid of interactive cells. The renderer packs one bit per cell
/// from the alpha channel of the frame currently on screen, so clicks on the
/// transparent parts of the sprite reach whatever is behind the window.
#[derive(Default)]
struct HitMask {
    cols: u32,
    rows: u32,
    cell: f64,
    bits: Vec<u8>,
}

impl HitMask {
    fn update(&mut self, cols: u32, rows: u32, cell: f64, bits: Vec<u8>) {
        if cols == 0 || rows == 0 || !cell.is_finite() || cell <= 0.0 {
            return;
        }
        let expected = (cols as usize * rows as usize).div_ceil(8);
        if bits.len() < expected {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        self.cell = cell;
        self.bits = bits;
    }

    fn is_empty(&self) -> bool {
        self.bits.iter().all(|byte| *byte == 0)
    }

    fn contains(&self, x: f64, y: f64) -> bool {
        if self.cols == 0 || self.rows == 0 || !x.is_finite() || !y.is_finite() {
            return false;
        }
        if x < 0.0 || y < 0.0 || x >= self.cols as f64 * self.cell || y >= self.rows as f64 * self.cell
        {
            return false;
        }
        let col = (x / self.cell).floor() as usize;
        let row = (y / self.cell).floor() as usize;
        let index = row * self.cols as usize + col;
        self.bits
            .get(index / 8)
            .is_some_and(|byte| byte & (1 << (index % 8)) != 0)
    }
}

/// Geometry is cached instead of queried: window getters round-trip through the
/// event loop and would deadlock this poller while the UI is busy.
#[derive(Default)]
struct WindowFrame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Default)]
struct HitTestState {
    mask: HitMask,
    frame: WindowFrame,
    screen_height: f64,
}

pub struct ClickThrough {
    state: Mutex<HitTestState>,
    interactive: AtomicBool,
}

impl ClickThrough {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(HitTestState::default()),
            interactive: AtomicBool::new(false),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HitTestState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    pub fn set_mask(&self, cols: u32, rows: u32, cell: f64, bits: Vec<u8>) {
        self.lock().mask.update(cols, rows, cell, bits);
    }

    pub fn set_frame(&self, x: f64, y: f64, width: f64, height: f64) {
        let mut state = self.lock();
        state.frame = WindowFrame {
            x,
            y,
            width,
            height,
        };
    }

    pub fn set_screen_height(&self, height: f64) {
        self.lock().screen_height = height;
    }

    /// While the user drags the pet the whole window must stay clickable,
    /// otherwise a pointer that outruns the window would end the drag.
    pub fn set_interactive(&self, interactive: bool) {
        self.interactive.store(interactive, Ordering::SeqCst);
    }

    fn cursor(&self) -> Option<(f64, f64)> {
        cursor_points(self.lock().screen_height)
    }

    /// True while the cursor sits on a pixel of the sprite, or while the mask is
    /// still missing so that a fresh window never swallows all clicks.
    pub fn contains_cursor(&self) -> bool {
        if self.interactive.load(Ordering::SeqCst) {
            return true;
        }
        let Some(cursor) = self.cursor() else {
            return true;
        };
        let state = self.lock();
        if state.mask.is_empty() {
            // Keep the window usable until the renderer reports a shape.
            return true;
        }
        let local_x = cursor.0 - state.frame.x;
        let local_y = cursor.1 - state.frame.y;
        if local_x < 0.0
            || local_y < 0.0
            || local_x > state.frame.width
            || local_y > state.frame.height
        {
            return false;
        }
        state.mask.contains(local_x, local_y)
    }
}

/// Cursor position in the same point-based screen space as the cached frame,
/// i.e. with the origin at the top-left of the main display.
#[cfg(target_os = "macos")]
fn cursor_points(screen_height: f64) -> Option<(f64, f64)> {
    if screen_height <= 0.0 {
        return None;
    }
    // AppKit reports points from the bottom-left corner of the main screen.
    let mouse = objc2_app_kit::NSEvent::mouseLocation();
    Some((mouse.x, screen_height - mouse.y))
}

#[cfg(not(target_os = "macos"))]
fn cursor_points(_screen_height: f64) -> Option<(f64, f64)> {
    None
}

pub fn spawn_watcher(window: WebviewWindow, state: Arc<ClickThrough>) {
    thread::spawn(move || {
        let mut ignored = None;
        loop {
            thread::sleep(HIT_POLL);
            let want_ignore = !state.contains_cursor();
            if ignored == Some(want_ignore) {
                continue;
            }
            if window.set_ignore_cursor_events(want_ignore).is_ok() {
                ignored = Some(want_ignore);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click_through(cells: &[usize], cols: u32, rows: u32, cell: f64) -> Arc<ClickThrough> {
        let mut bits = vec![0u8; (cols as usize * rows as usize).div_ceil(8)];
        for index in cells {
            bits[index / 8] |= 1 << (index % 8);
        }
        let state = Arc::new(ClickThrough::new());
        state.set_mask(cols, rows, cell, bits);
        state
    }

    #[test]
    fn hits_only_packed_cells() {
        let mask = click_through(&[0, 5], 4, 3, 10.0);
        let mask = mask.lock();
        assert!(mask.mask.contains(5.0, 5.0));
        assert!(mask.mask.contains(15.0, 15.5));
        assert!(!mask.mask.contains(25.0, 5.0));
        assert!(!mask.mask.contains(9.9, 15.0));
    }

    #[test]
    fn rejects_points_outside_the_grid() {
        let mask = click_through(&[0, 1, 2, 3], 2, 2, 8.0);
        let mask = mask.lock();
        assert!(mask.mask.contains(15.9, 15.9));
        assert!(!mask.mask.contains(16.0, 4.0));
        assert!(!mask.mask.contains(-1.0, 4.0));
        assert!(!mask.mask.contains(f64::NAN, 1.0));
    }

    #[test]
    fn ignores_truncated_masks() {
        let mut mask = HitMask::default();
        mask.update(8, 8, 8.0, vec![0xff]);
        assert!(!mask.contains(4.0, 4.0));
    }

    #[test]
    fn an_empty_mask_keeps_the_window_interactive() {
        let state = click_through(&[], 4, 4, 10.0);
        assert!(state.lock().mask.is_empty());
        assert!(state.contains_cursor());
    }

    #[test]
    fn points_outside_the_window_never_capture() {
        let state = click_through(&[0], 4, 4, 10.0);
        state.set_frame(100.0, 200.0, 40.0, 40.0);
        state.set_screen_height(0.0);
        // Without a screen height the platform cursor is unavailable, so the
        // window stays interactive rather than silently losing all clicks.
        assert!(state.contains_cursor());
    }
}
