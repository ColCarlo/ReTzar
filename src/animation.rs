//! Presentation timelines, independent of simulation and render frame rate.
//!
//! The original engine (Tzar.exe 1.11) times pictures in two ways:
//! - sheets with directions (8 or 24 columns) carry an animation record:
//!   `anim_time` (default 1000 ms, rounded down to a multiple of the rows),
//!   optional per-row `delay`s, padded one millisecond at a time, row after
//!   row, until they reach `anim_time`;
//! - single-column sheets are stepped by a timer every `1000 / fps` ms.
use crate::assets::Ini;

#[derive(Clone, Debug)]
pub struct Timeline {
    ends: Vec<u64>,
    pub duration_ms: u64,
}
impl Timeline {
    pub fn new(delays: impl IntoIterator<Item = u64>) -> Self {
        let mut duration_ms = 0;
        let ends = delays
            .into_iter()
            .map(|d| {
                duration_ms += d.max(1);
                duration_ms
            })
            .collect();
        Self { ends, duration_ms }
    }
    /// Every frame lasts `period_ms`.
    pub fn uniform(rows: usize, period_ms: u64) -> Self {
        Self::new(std::iter::repeat_n(period_ms.max(1), rows.max(1)))
    }
    /// The animation record of a directional sheet, as the original builds it
    /// when it loads `IMAGES.INI`.
    pub fn directional(ini: &Ini, section: &str, rows: usize) -> Self {
        let rows = rows.max(1);
        let anim_time = ini
            .get(section, "anim_time")
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(1000);
        let per_row = anim_time / rows as u64;
        let total = per_row * rows as u64;
        let mut delays: Vec<u64> = ini
            .get(section, "delay")
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .filter_map(|v| v.parse().ok())
            .collect();
        // A wrong count is a data error in the original; equal rows keep the
        // picture moving instead.
        if delays.len() != rows {
            delays = vec![per_row; rows];
        }
        let mut sum: u64 = delays.iter().sum();
        let mut row = 0;
        while sum < total {
            delays[row] += 1;
            row = (row + 1) % rows;
            sum += 1;
        }
        Self::new(delays)
    }
    /// A single-column sheet stepped by its `fps` key, or by `default_fps`
    /// when the key is absent or zero. `None` when the picture stays still.
    pub fn stepped(ini: &Ini, section: &str, rows: usize, default_fps: u64) -> Option<Self> {
        let fps = ini
            .get(section, "fps")
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|&v| v > 0)
            .unwrap_or(default_fps);
        (rows > 1 && fps > 0).then(|| Self::uniform(rows, 1000 / fps))
    }
    /// Where each frame ends, in milliseconds from the start of the cycle.
    pub fn ends(&self) -> &[u64] {
        &self.ends
    }
    /// How long frame `k` shows.
    pub fn delay(&self, k: usize) -> u64 {
        self.ends.get(k).map_or(0, |end| end - self.start_of(k))
    }
    /// When frame `k` begins, from the start of the cycle.
    pub fn start_of(&self, k: usize) -> u64 {
        if k == 0 {
            0
        } else {
            self.ends.get(k - 1).copied().unwrap_or(0)
        }
    }
    pub fn frame(&self, elapsed_ms: u64, phase_ms: u64) -> usize {
        if self.ends.is_empty() {
            return 0;
        }
        let t = (elapsed_ms % self.duration_ms + phase_ms % self.duration_ms) % self.duration_ms;
        self.ends.partition_point(|&end| end <= t)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_idle_holds_then_animates_and_loops_without_frame_rate_drift() {
        let ini = Ini::parse(b"[idle]\nrows=3\nanim_time=1\ndelay=8000,228,227\n");
        let t = Timeline::directional(&ini, "idle", 3);
        assert_eq!(t.duration_ms, 8455);
        assert_eq!(
            (
                t.frame(7999, 0),
                t.frame(8000, 0),
                t.frame(8228, 0),
                t.frame(8455, 0)
            ),
            (0, 1, 2, 0)
        );
        assert_eq!(t.frame(8455 * 1000 + 8228, 0), 2);
        assert_eq!(t.frame(0, 8000), 1);
    }
    #[test]
    fn directional_records_default_to_one_second_and_pad_short_delays() {
        // No keys: 1000 ms split over 7 rows, rounded down to 994.
        let t = Timeline::directional(&Ini::parse(b"[ship]\nrows=7"), "ship", 7);
        assert_eq!(t.duration_ms, 994);
        // Delays below anim_time grow by one millisecond per row, in order.
        let ini = Ini::parse(b"[d]\nanim_time=1000\ndelay=100,100,100,9000\n");
        let short = Timeline::directional(&Ini::parse(b"[d]\ndelay=10,10\n"), "d", 2);
        assert_eq!(short.duration_ms, 1000);
        assert_eq!(short.frame(499, 0), 0);
        assert_eq!(short.frame(500, 0), 1);
        // Longer delays are kept as they are.
        assert_eq!(Timeline::directional(&ini, "d", 4).duration_ms, 9300);
        // A trailing comma does not add a row.
        let cow = Ini::parse(b"[cow]\ndelay=800,200,\n");
        assert_eq!(Timeline::directional(&cow, "cow", 2).duration_ms, 1000);
    }
    #[test]
    fn single_column_sheets_step_at_their_fps_or_stay_still() {
        let flag = Timeline::stepped(&Ini::parse(b"[flag]\nfps=4"), "flag", 7, 0).unwrap();
        assert_eq!(flag.duration_ms, 1750);
        assert_eq!(flag.frame(250, 0), 1);
        // Building overlays without fps run at 10 frames a second.
        let overlay = Timeline::stepped(&Ini::parse(b"[c]\nrows=6"), "c", 6, 10).unwrap();
        assert_eq!(overlay.duration_ms, 600);
        assert!(Timeline::stepped(&Ini::parse(b"[still]"), "still", 6, 0).is_none());
        assert!(Timeline::stepped(&Ini::parse(b"[one]\nfps=5"), "one", 1, 0).is_none());
    }
}
