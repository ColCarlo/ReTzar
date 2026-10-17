//! The lightning of the original (class 101, vtable 0x566240): a bolt from
//! one point to another in map pixels, a centre line that wanders across the
//! straight path, two lines one pixel either side and `branches` pairs of
//! fainter lines further out.

use serde::{Deserialize, Serialize};

/// What the spell asks of a bolt (the record read by 0x46ca10).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Bolt {
    /// Start and end in map pixels (+0x38, +0x3c).
    pub start: [i32; 2],
    pub end: [i32; 2],
    /// Length of a segment (+0x40).
    pub segment: u16,
    /// The wander's step, and how it leaves the path in the first third and
    /// comes back after (+0x42, +0x44, +0x46).
    pub step: u16,
    pub away: u16,
    pub back: u16,
    /// Lifetime in ms (+0x48) and pairs of outer lines (+0x54).
    pub ms: u32,
    pub branches: u16,
    /// Drops its outer lines when more than six bolts show (+0x56).
    pub lean: bool,
    /// Colours of the ends and of the middle (RGB555, +0x4c and +0x50).
    pub ends: u16,
    pub middle: u16,
}

/// A drawn bolt: the polylines of the centre, of its two neighbours and of
/// each pair of outer lines, and the colour of each segment (from 1).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape {
    pub centre: Vec<[i32; 2]>,
    pub minus: Vec<[i32; 2]>,
    pub plus: Vec<[i32; 2]>,
    pub outer: Vec<(Vec<[i32; 2]>, Vec<[i32; 2]>)>,
    pub colours: Vec<u16>,
}

/// A bolt as it is made: its first wander (0x46d2d8) and the unit normal
/// of its path, which later wanders keep.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Strike {
    pub bolt: Bolt,
    pub shape: Shape,
    pub normal: (f64, f64),
}

/// `__ftol` (0x549eb0): towards zero.
fn trunc(v: f64) -> i32 {
    v as i32
}

/// The straight path (0x46ca10): the unit normal `(p, q)` = (dy, −dx) /
/// length, the number of segments (at least two, the length then stretched)
/// and both ends of every line; the colours fade from the ends to the middle
/// over each half (RGB555, 0x46d180).
pub fn shape(bolt: &Bolt) -> (Shape, f64, f64) {
    let [sx, sy] = bolt.start;
    let dx = bolt.end[0] - sx;
    let dy = bolt.end[1] - sy;
    let length = ((dx * dx + dy * dy) as f64).sqrt();
    let (p, q) = if length != 0. {
        (dy as f64 / length, -dx as f64 / length)
    } else {
        (0., 0.)
    };
    let segment = bolt.segment.max(1) as i32;
    let count = ((trunc(length) / segment).max(2)) as usize;
    let full = (count as i32 * segment) as f64;
    let at = |along: f64, side: f64| {
        [
            sx + trunc(side * p - along * q),
            sy + trunc(side * q + along * p),
        ]
    };
    let line = |side: f64| {
        let mut points = vec![[0, 0]; count + 1];
        points[0] = at(0., side);
        points[count] = at(full, side);
        points
    };
    let outer = (0..bolt.branches)
        .map(|k| (line(-(k as f64) - 2.), line(k as f64 + 2.)))
        .collect();
    let mut colours = vec![0u16; count + 1];
    let half = count / 2;
    let channel = |c: u16, shift: u16| ((c >> shift) & 0x1f) as i32;
    for k in 1..=half {
        let mix = |shift| {
            let a = channel(bolt.ends, shift);
            let b = channel(bolt.middle, shift);
            ((a + (b - a) * k as i32 / half as i32) & 0x1f) as u16
        };
        let c = mix(10) << 10 | mix(5) << 5 | mix(0);
        colours[k] = c;
        colours[count - k] = c;
    }
    colours[count] = colours[count - 1];
    (
        Shape {
            centre: line(0.),
            minus: line(-1.),
            plus: line(1.),
            outer,
            colours,
        },
        p,
        q,
    )
}

/// A new wander of the inner points (vt+0x2b0, 0x46d5a0), drawn from `roll`
/// (`R(lo, hi)`, the stream of pictures): at each point a coin, then a step
/// of `R(0, step)` away from or back towards the path. In the first third
/// the bolt keeps leaving the path unless `R(0, away)` says otherwise; then
/// it turns back unless `R(0, back)` does.
pub fn wander(
    bolt: &Bolt,
    shape: &mut Shape,
    p: f64,
    q: f64,
    mut roll: impl FnMut(i32, i32) -> i32,
) {
    let count = shape.centre.len() - 1;
    let third = count / 3;
    let [sx, sy] = bolt.start;
    let segment = bolt.segment.max(1) as i32;
    let at = |along: f64, side: f64| {
        [
            sx + trunc(side * p - along * q),
            sy + trunc(side * q + along * p),
        ]
    };
    let mut d: i32 = 0;
    for i in 1..count {
        let along = (segment * i as i32) as f64;
        let coin = roll(0, 1);
        let up = if i < third {
            if d < 0 {
                coin != 0 && roll(0, bolt.away as i32) != 0
            } else if d == 0 {
                coin != 0
            } else {
                coin != 0 || roll(0, bolt.away as i32) == 0
            }
        } else if d < 0 {
            coin != 0 || roll(0, bolt.back as i32) == 0
        } else if d == 0 {
            coin != 0
        } else {
            coin != 0 && roll(0, bolt.back as i32) != 0
        };
        let step = roll(0, bolt.step as i32);
        d = if up { d + step } else { d - step };
        let side = d as f64;
        shape.centre[i] = at(along, side);
        shape.minus[i] = at(along, side - 1.);
        shape.plus[i] = at(along, side + 1.);
        for (k, (minus, plus)) in shape.outer.iter_mut().enumerate() {
            minus[i] = at(along, side - k as f64 - 2.);
            plus[i] = at(along, side + k as f64 + 2.);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bolt() -> Bolt {
        Bolt {
            start: [100, 100],
            end: [300, 100],
            segment: 4,
            step: 3,
            away: 9,
            back: 1,
            ms: 700,
            branches: 10,
            lean: true,
            ends: 0x001f,
            middle: 0x7fff,
        }
    }
    #[test]
    fn the_path_runs_from_end_to_end_with_lines_aside() {
        // 0x46ca10: 200 pixels in segments of 4; for an eastward bolt the
        // normal (dy, −dx) / length points north (y grows southwards).
        let (s, p, q) = shape(&bolt());
        assert_eq!(s.centre.len(), 51);
        assert_eq!((p, q), (0., -1.));
        assert_eq!(s.centre[0], [100, 100]);
        assert_eq!(s.centre[50], [300, 100]);
        assert_eq!(s.minus[0], [100, 101]);
        assert_eq!(s.plus[50], [300, 99]);
        assert_eq!(s.outer[0].0[0], [100, 102]);
        assert_eq!(s.outer[9].1[0], [100, 89]);
        // Blue at the ends towards white in the middle (0x46d189).
        assert_eq!(s.colours[25], 0x7fff);
        assert_eq!(s.colours[1], s.colours[49]);
        assert_eq!(s.colours[50], s.colours[49]);
        let blue = s.colours[1] & 0x1f;
        assert_eq!(blue, 31);
        assert_eq!(s.colours[1] >> 10, 31 / 25);
    }
    #[test]
    fn a_short_bolt_keeps_two_segments_and_a_still_roll_keeps_it_straight() {
        let mut b = bolt();
        b.end = [103, 100];
        let (mut s, p, q) = shape(&b);
        assert_eq!(s.centre.len(), 3);
        assert_eq!(s.centre[2], [108, 100]);
        // A roll that never steps leaves the inner points on the path.
        wander(&b, &mut s, p, q, |lo, _| lo);
        assert_eq!(s.centre[1], [104, 100]);
        assert_eq!(s.minus[1], [104, 101]);
    }
    #[test]
    fn the_wander_leaves_the_path_early_and_comes_back_late() {
        // Always heads (coin 1) with the highest checks: away from the path
        // all the way with `away`, back towards it later only when d < 0.
        let b = bolt();
        let (mut s, p, q) = shape(&b);
        wander(&b, &mut s, p, q, |_, hi| hi);
        // d grows by 3 a point in the first third, along the normal.
        assert_eq!(s.centre[1], [104, 97]);
        assert_eq!(s.centre[2], [108, 94]);
    }
}
