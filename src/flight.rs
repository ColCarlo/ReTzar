//! Flying animals in motion. The original `eagle` class (also behind the
//! seagulls of the `bird` class) wanders over the whole map: it picks a
//! random point ten cells inside the edges, flies there one pixel per step
//! in the eight directions of its movement table, beats its wings every few
//! steps and turns through the in-between columns of its 24-column sheet.
use crate::world::{FLIGHT_STEPS, Flight};

/// The state of one flier; [`Flight`] holds what never changes.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Flyer {
    /// The object's point in screen pixels (y down).
    position: [i32; 2],
    target: Option<[i32; 2]>,
    /// The points of a loop flown before the target (0x4edc80), in order.
    #[serde(default)]
    route: Vec<[i32; 2]>,
    /// After the loop, the corners of the cells of the flyers' path to the
    /// target (0x4e7980), in order.
    #[serde(default)]
    path: Vec<[i32; 2]>,
    direction: usize,
    column: usize,
    row: usize,
    steps: u32,
    pending_ms: u64,
    random: u32,
    /// Frame ticks since the cell began (+0x12e): the wings move on the
    /// odd ones (0x4ee151) and a turn's columns come at a quarter and at
    /// half of the cell (0x4ee127).
    #[serde(default)]
    beats: u32,
    /// The columns of a turn at a quarter and at half of the cell (+0x128,
    /// +0x12a, 0x4edb10).
    #[serde(default)]
    turn: Option<[usize; 2]>,
    /// Pixel steps into the present cell; each cell counts its frame ticks
    /// from 0 again (0x4edc65).
    #[serde(default)]
    cell_steps: u32,
}
impl Flyer {
    pub fn valid(&self, spec: &Flight) -> bool {
        let bound = spec
            .start
            .iter()
            .chain(&spec.range)
            .fold(1024f32, |n, v| n.max(v.abs())) as i64
            * 2
            + 1024;
        self.direction < 8
            && self.column < spec.frames.len()
            && self.row < spec.frames[self.column].len()
            && self.pending_ms < spec.step_ms
            && self.steps < spec.steps_per_frame
            && self.random != 0
            && self
                .position
                .iter()
                .chain(self.target.iter().flatten())
                .chain(self.route.iter().flatten())
                .chain(self.path.iter().flatten())
                .all(|&v| (v as i64).abs() <= bound)
    }
    pub fn new(spec: &Flight) -> Self {
        let columns = spec.frames.len().max(1);
        let column = spec.column % columns;
        Self {
            position: [spec.start[0] as i32, -spec.start[1] as i32],
            target: None,
            route: Vec::new(),
            path: Vec::new(),
            direction: (column * 8 + columns / 2) / columns % 8,
            column,
            row: 0,
            steps: 0,
            pending_ms: 0,
            random: spec.seed | 1,
            beats: 0,
            turn: None,
            cell_steps: 0,
        }
    }
    fn next_random(&mut self) -> u32 {
        // xorshift32: repeatable per bird, like a saved game would be.
        let mut x = self.random;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.random = x;
        x
    }
    /// Run every whole step that fits in `elapsed_ms` more milliseconds;
    /// `find` gives the flyers' path between two cells, the first left out.
    pub fn advance(
        &mut self,
        spec: &Flight,
        elapsed_ms: u64,
        find: &mut dyn FnMut([i32; 2], [i32; 2]) -> Vec<[i32; 2]>,
    ) {
        // A long stall (loading, a debugger) must not replay minutes of flight.
        self.pending_ms = (self.pending_ms + elapsed_ms).min(2000);
        while self.pending_ms >= spec.step_ms {
            self.pending_ms -= spec.step_ms;
            self.step(spec, find);
        }
    }
    fn step(&mut self, spec: &Flight, find: &mut dyn FnMut([i32; 2], [i32; 2]) -> Vec<[i32; 2]>) {
        let low = spec.range[0] as i32;
        let high = spec.range[1].max(spec.range[0]) as i32;
        // A point of the loop or of the path reached: a new cell begins.
        let reached = if self.route.first() == Some(&self.position) {
            self.route.remove(0);
            true
        } else if self.route.is_empty() && self.path.first() == Some(&self.position) {
            self.path.remove(0);
            true
        } else {
            false
        };
        let target = match self.target {
            Some(t) if t != self.position => t,
            _ => {
                // 0x4e0a23: a whole cell ten cells inside the edges.
                let (first, last) = ((low + 31) / 32, high / 32);
                let span = (last - first + 1).max(1) as u32;
                let t = [
                    (first + (self.next_random() % span) as i32) * 32,
                    (first + (self.next_random() % span) as i32) * 32,
                ];
                self.target = Some(t);
                // 0x4edc80: turning by more than 45° the flier first loops
                // back over 22 cells; points outside the margin are skipped
                // (0x4ee2d0).
                let new = heading([t[0] - self.position[0], t[1] - self.position[1]]);
                let turn = self.direction.abs_diff(new);
                if turn > 1 && turn != 7 {
                    let cell = [
                        self.position[0].div_euclid(32),
                        self.position[1].div_euclid(32),
                    ];
                    self.route = manoeuvre(self.direction, new)
                        .into_iter()
                        .map(|[x, y]| [(cell[0] + x) * 32, (cell[1] + y) * 32])
                        .filter(|p| p.iter().all(|v| (low..=high).contains(v)))
                        .collect();
                }
                // 0x4e7980: from the loop's end, the flyers' path to the
                // target's cell, cell by cell.
                let from = self.route.last().copied().unwrap_or(self.position);
                self.path = find(
                    [from[0].div_euclid(32), from[1].div_euclid(32)],
                    [t[0] / 32, t[1] / 32],
                )
                .into_iter()
                .map(|[x, y]| [x * 32, y * 32])
                .collect();
                t
            }
        };
        let next = self
            .route
            .first()
            .or(self.path.first())
            .copied()
            .unwrap_or(target);
        let direction = heading([next[0] - self.position[0], next[1] - self.position[1]]);
        let columns = spec.frames.len().max(1);
        // A new heading starts a cell: the turn shows its first column at
        // once (0x4edbf0).
        // Without a path (none found) a cell is 32 steps.
        let new_cell = reached || (self.path.is_empty() && self.cell_steps >= 32);
        if direction != self.direction {
            self.direction = direction;
            self.turn(columns);
        } else if new_cell {
            self.cell_steps = 0;
            self.beats = 0;
            self.turn = None;
        }
        let [dx, dy] = FLIGHT_STEPS[self.direction];
        self.position = [self.position[0] + dx, self.position[1] + dy];
        self.cell_steps += 1;
        self.steps += 1;
        if self.steps >= spec.steps_per_frame {
            self.steps = 0;
            // 0x4ee108: the turn's second column at a quarter of the cell's
            // frame ticks (+0x7c: 32 / steps per frame, 0x49897d), the
            // heading's own at half.
            let ticks = (32 / spec.steps_per_frame).max(1);
            if let Some([quarter, half]) = self.turn {
                if self.beats == ticks / 4 {
                    self.column = quarter;
                } else if self.beats == ticks / 2 {
                    self.column = half;
                }
            }
            let rows = spec.frames.get(self.column).map_or(1, Vec::len).max(1);
            // Gliding (row 0) turns into a beat with 101 chances in 701 on
            // the odd ticks; a beat runs through its rows back to 0
            // (0x4ee151–0x4ee19d).
            if self.beats % 2 == 1 && (self.row != 0 || self.next_random() % 701 <= 100) {
                self.row = (self.row + 1) % rows;
            }
            self.beats = self.beats.wrapping_add(1);
        }
    }
    /// 0x4edb10: toward the heading's column (three to a heading of the 24),
    /// one column at once, two at a quarter of the cell and the heading's at
    /// half; up when the heading's column is higher, down when lower, but
    /// across column 0 between 0 and 21. Turning down from column 0 or 1
    /// starts from column 2.
    fn turn(&mut self, columns: usize) {
        let goal = self.direction * columns / 8;
        self.cell_steps = 0;
        self.beats = 0;
        let c = self.column;
        let [first, quarter] = if goal == c {
            [c, c]
        } else if c == 0 && goal + 3 == columns {
            [columns - 1, columns - 2]
        } else if goal == 0 && c + 3 == columns {
            [columns - 2, columns - 1]
        } else if goal > c {
            [c + 1, (c + 2).min(columns - 1)]
        } else {
            let c = c.max(2);
            [c - 1, c - 2]
        };
        self.column = first;
        self.turn = Some([quarter, goal]);
    }
    /// World position (y up) of the picture's top-left corner.
    pub fn top_left(&self, spec: &Flight) -> [f32; 2] {
        [
            self.position[0] as f32 + spec.offset[0],
            -(self.position[1] as f32) + spec.offset[1],
        ]
    }
    pub fn frame(&self) -> (usize, usize) {
        (self.column, self.row)
    }
}

/// The loop of a flier turning from heading `from` to `to` (0x4edc80): cell
/// offsets from where it is, one of two templates (0x590acc, 0x590b24)
/// mirrored (0x4eda00) or turned by quarters (0x4eda50) as the present
/// heading and the new one require (switch 0x4ee014).
fn manoeuvre(from: usize, to: usize) -> [[i32; 2]; 22] {
    const A: [[i32; 2]; 22] = [
        [0, -1],
        [0, -2],
        [0, -3],
        [0, -4],
        [0, -5],
        [0, -6],
        [1, -7],
        [2, -8],
        [3, -8],
        [4, -8],
        [5, -8],
        [6, -7],
        [7, -6],
        [7, -5],
        [7, -4],
        [6, -3],
        [5, -2],
        [4, -2],
        [3, -2],
        [2, -2],
        [1, -1],
        [0, 0],
    ];
    const B: [[i32; 2]; 22] = [
        [-1, -1],
        [-2, -2],
        [-3, -2],
        [-4, -2],
        [-5, -2],
        [-6, -3],
        [-7, -4],
        [-7, -5],
        [-7, -6],
        [-6, -7],
        [-5, -8],
        [-4, -8],
        [-3, -8],
        [-2, -8],
        [-1, -7],
        [0, -6],
        [0, -5],
        [0, -4],
        [0, -3],
        [0, -2],
        [0, -1],
        [0, 0],
    ];
    let flip_x = |[x, y]: [i32; 2]| [-x, y];
    let flip_y = |[x, y]: [i32; 2]| [x, -y];
    let both = |[x, y]: [i32; 2]| [-x, -y];
    let quarter = |[x, y]: [i32; 2]| [-y, x];
    let three = |[x, y]: [i32; 2]| [y, -x];
    let near = |range: std::ops::RangeInclusive<usize>| range.contains(&to);
    let (template, change): (_, &dyn Fn([i32; 2]) -> [i32; 2]) = match from {
        0 if near(2..=4) => (A, &both),
        0 => (A, &flip_y),
        1 if near(3..=5) => (B, &both),
        1 => (B, &|p| three(flip_y(p))),
        2 if near(4..=6) => (A, &quarter),
        2 => (A, &|p| flip_y(quarter(p))),
        3 if near(5..=7) => (B, &quarter),
        3 => (B, &flip_x),
        4 if near(1..=2) => (A, &flip_x),
        4 => (A, &|p| p),
        5 if near(2..=3) => (B, &|p| quarter(flip_y(p))),
        5 => (B, &|p| p),
        6 if near(0..=2) => (A, &three),
        6 => (A, &|p| flip_y(three(p))),
        _ if near(1..=3) => (B, &three),
        _ => (B, &flip_y),
    };
    template.map(change)
}

/// The movement direction (index into [`FLIGHT_STEPS`]) toward `delta` on an
/// eight-direction route: diagonally until one axis lines up, then straight,
/// so that each flight turns once and never zig-zags.
fn heading(delta: [i32; 2]) -> usize {
    let step = [delta[0].signum(), delta[1].signum()];
    FLIGHT_STEPS.iter().position(|&s| s == step).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn spec() -> Flight {
        Flight {
            body: 0,
            shadow: None,
            frames: vec![vec![0; 4]; 24],
            shadow_frames: Vec::new(),
            step_ms: 7,
            steps_per_frame: 9,
            start: [640., -640.],
            offset: [-1., 2.],
            range: [320., 64. * 32. - 352.],
            column: 0,
            seed: 12345,
            shadow_lift: [40., 70.],
        }
    }
    #[test]
    fn headings_follow_the_original_table() {
        assert_eq!(heading([0, 10]), 0);
        assert_eq!(heading([10, 10]), 1);
        assert_eq!(heading([10, 0]), 2);
        assert_eq!(heading([10, -10]), 3);
        assert_eq!(heading([0, -10]), 4);
        assert_eq!(heading([-10, -10]), 5);
        assert_eq!(heading([-10, 0]), 6);
        assert_eq!(heading([-10, 9]), 7);
        assert_eq!(heading([10, 1]), 1, "diagonal first");
    }
    #[test]
    fn birds_keep_flying_inside_the_map_one_pixel_per_step() {
        let s = spec();
        let mut bird = Flyer::new(&s);
        let start = bird.position;
        bird.advance(&s, 7 * 9, &mut |_, _| Vec::new());
        let moved = (bird.position[0] - start[0])
            .abs()
            .max((bird.position[1] - start[1]).abs());
        assert_eq!(moved, 9);
        // 0x4ee151: the first frame tick may start a beat or keep gliding.
        assert!(bird.frame().1 <= 1);
        let mut seen = std::collections::BTreeSet::new();
        let mut rows = std::collections::BTreeSet::new();
        for _ in 0..4000 {
            bird.advance(&s, 50, &mut |_, _| Vec::new());
            seen.insert(bird.frame().0);
            rows.insert(bird.frame().1);
            for v in bird.position {
                assert!((320..=64 * 32 - 352).contains(&v), "{v} outside the map");
            }
        }
        // The bird glides and beats its wings through every row.
        assert_eq!(rows.len(), 4);
        // Many headings over a long flight, all shown through valid columns.
        assert!(seen.len() > 8 && seen.iter().all(|&c| c < 24));
        assert_eq!(bird.top_left(&s)[0], bird.position[0] as f32 - 1.);
    }
    #[test]
    fn sharp_turns_loop_ahead_and_come_back_like_the_original() {
        // 0x4edc80: every loop starts one cell ahead on the present
        // heading and ends where it began.
        for from in 0..8usize {
            for to in 0..8 {
                let turn = from.abs_diff(to);
                if turn <= 1 || turn == 7 {
                    continue;
                }
                let path = manoeuvre(from, to);
                assert_eq!(path[0], FLIGHT_STEPS[from], "{from} → {to}");
                assert_eq!(path[21], [0, 0]);
                // Neighbouring points, one cell apart.
                for w in path.windows(2) {
                    let d = [w[1][0] - w[0][0], w[1][1] - w[0][1]];
                    assert!(d[0].abs() <= 1 && d[1].abs() <= 1, "{from} → {to}");
                }
            }
        }
        // Flying north and turning south: template 0x590acc as it is,
        // round to the east.
        assert_eq!(manoeuvre(4, 0)[8], [3, -8]);
        assert_eq!(manoeuvre(4, 1)[8], [-3, -8]);
        // In flight the loop comes before the new target.
        let s = spec();
        for seed in 1..40 {
            let mut bird = Flyer::new(&Flight { seed, ..s.clone() });
            bird.position = [960, 960];
            bird.direction = 0;
            bird.step(&s, &mut |_, _| Vec::new());
            let t = bird.target.unwrap();
            let new = heading([t[0] - 960, t[1] - 960]);
            let turn = new.abs_diff(0);
            assert_eq!(!bird.route.is_empty(), turn > 1 && turn != 7);
        }
    }
    #[test]
    fn after_the_loop_birds_follow_the_flyers_path_cell_by_cell() {
        // 0x4e7980: the path's cells, here straight east then south east,
        // flown corner to corner; each point reached starts a cell.
        let s = spec();
        let mut bird = Flyer::new(&s);
        bird.position = [960, 960];
        bird.direction = 2;
        bird.target = None;
        let asked = std::cell::RefCell::new(Vec::new());
        let mut find = |from: [i32; 2], to: [i32; 2]| {
            asked.borrow_mut().push((from, to));
            vec![[31, 30], [32, 31]]
        };
        bird.step(&s, &mut find);
        let t = bird.target.unwrap();
        // A loop ends where it began: the path starts from the same cell.
        assert_eq!(*asked.borrow(), [([30, 30], [t[0] / 32, t[1] / 32])]);
        assert_eq!(bird.path, [[31 * 32, 960], [32 * 32, 31 * 32]]);
        // Without the loop: east to the first corner, one pixel a step.
        (bird.position, bird.direction, bird.cell_steps) = ([960, 960], 2, 0);
        bird.route.clear();
        for _ in 0..32 {
            bird.step(&s, &mut find);
        }
        assert_eq!(bird.position, [31 * 32, 960]);
        bird.step(&s, &mut find);
        // The point reached: the next is south east, a cell begun.
        assert_eq!((bird.direction, bird.cell_steps), (1, 1));
        assert_eq!(bird.path, [[32 * 32, 31 * 32]]);
    }
    #[test]
    fn turns_show_two_columns_then_the_heading_like_the_original() {
        // A frame every 4 steps: 8 frame ticks a cell (+0x7c), the second
        // column at tick 2 and the heading's at tick 4 (0x4ee108).
        let s = Flight {
            steps_per_frame: 4,
            ..spec()
        };
        let mut bird = Flyer::new(&s);
        bird.position = [960, 960];
        bird.target = Some([960 + 320, 960]);
        (bird.direction, bird.column) = (0, 0);
        bird.step(&s, &mut |_, _| Vec::new());
        assert_eq!((bird.direction, bird.column), (2, 1), "at once");
        let mut seen = Vec::new();
        for _ in 0..4 * 6 - 1 {
            bird.step(&s, &mut |_, _| Vec::new());
            if bird.steps == 0 {
                seen.push(bird.column);
            }
        }
        assert_eq!(seen, [1, 1, 2, 2, 6, 6]);
        // 0x4edb10: down when the heading's column is lower, from column 2
        // at least; across 0 only between 0 and 21.
        (bird.column, bird.direction) = (12, 1);
        bird.turn(24);
        assert_eq!((bird.column, bird.turn), (11, Some([10, 3])));
        (bird.column, bird.direction) = (1, 0);
        bird.turn(24);
        assert_eq!((bird.column, bird.turn), (1, Some([0, 0])));
        (bird.column, bird.direction) = (0, 7);
        bird.turn(24);
        assert_eq!((bird.column, bird.turn), (23, Some([22, 21])));
        (bird.column, bird.direction) = (21, 0);
        bird.turn(24);
        assert_eq!((bird.column, bird.turn), (22, Some([23, 0])));
        // Not the short way otherwise: from 3 to 18 upwards.
        (bird.column, bird.direction) = (3, 6);
        bird.turn(24);
        assert_eq!((bird.column, bird.turn), (4, Some([5, 18])));
    }
}
