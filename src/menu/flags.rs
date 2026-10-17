//! The flags beside the names of «Lingua», an addition of this version, so a
//! player finds their language at a glance in a list written in other ones.
//! Each is drawn from its geometry, sampled many times within each pixel,
//! then lit as cloth in a dark frame, as the pictures of the game are.
use crate::{bitmap::Bitmap, lang::Language};

/// Size of a flag in the menus' pixels.
pub const SIZE: [u32; 2] = [24, 16];

const WHITE: [u8; 3] = [246, 246, 240];

/// Every flag, made once, by [`Language::index`].
pub fn flags() -> &'static [Bitmap] {
    static FLAGS: std::sync::OnceLock<Vec<Bitmap>> = std::sync::OnceLock::new();
    FLAGS.get_or_init(|| Language::ALL.map(flag).to_vec())
}
/// The flag of a language: Italy, the United Kingdom, France, Germany,
/// Spain, Poland, Portugal.
pub fn flag(language: Language) -> Bitmap {
    let [w, h] = SIZE;
    let mut out = Bitmap {
        width: w,
        height: h,
        rgba: vec![0; (w * h * 4) as usize],
    };
    // Samples across each pixel's side.
    const N: u32 = 6;
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0f32; 3];
            for sy in 0..N {
                for sx in 0..N {
                    let u = (x as f32 + (sx as f32 + 0.5) / N as f32) / w as f32;
                    let v = (y as f32 + (sy as f32 + 0.5) / N as f32) / h as f32;
                    // Mixed as light, not as numbers.
                    for (s, c) in sum.iter_mut().zip(colour(language, u, v)) {
                        *s += (c as f32 / 255.).powf(2.2);
                    }
                }
            }
            let (u, v) = ((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
            // The cloth: lighter above, a gentle fold across.
            let light = 1.06 - 0.14 * v + 0.07 * (u * 7.5 + 0.8).sin() * (1. - 0.4 * v);
            let k = ((y * w + x) * 4) as usize;
            for (ch, s) in sum.iter().enumerate() {
                let c = (s / (N * N) as f32 * light).clamp(0., 1.);
                out.rgba[k + ch] = (c.powf(1. / 2.2) * 255.).round() as u8;
            }
            out.rgba[k + 3] = 255;
        }
    }
    // A dark frame, a little light along the top inside it.
    for y in 0..h {
        for x in 0..w {
            let k = ((y * w + x) * 4) as usize;
            if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
                out.rgba[k..k + 3].copy_from_slice(&[38, 28, 18]);
            } else if y == 1 {
                for c in &mut out.rgba[k..k + 3] {
                    *c = (*c as u16 + (255 - *c as u16) / 5) as u8;
                }
            }
        }
    }
    out
}

/// The colour of the flag at `u`, `v` (0 to 1 across and down).
fn colour(language: Language, u: f32, v: f32) -> [u8; 3] {
    match language {
        Language::Italian => thirds(u, [[0, 140, 69], WHITE, [205, 33, 42]]),
        Language::French => thirds(u, [[0, 38, 152], WHITE, [237, 41, 57]]),
        Language::German => thirds(v, [[20, 20, 20], [221, 0, 0], [255, 206, 0]]),
        Language::Polish => {
            if v < 0.5 {
                WHITE
            } else {
                [220, 20, 60]
            }
        }
        Language::Spanish => spain(u, v),
        Language::Portuguese => portugal(u, v),
        Language::English => union(u, v),
    }
}
fn thirds(t: f32, colours: [[u8; 3]; 3]) -> [u8; 3] {
    colours[((t * 3.) as usize).min(2)]
}

/// Red, yellow twice as tall, red; the arms in the yellow towards the
/// hoist, made small: the crown over the quartered shield between the two
/// pillars.
fn spain(u: f32, v: f32) -> [u8; 3] {
    const RED: [u8; 3] = [173, 21, 25];
    const YELLOW: [u8; 3] = [250, 189, 0];
    const GOLD: [u8; 3] = [196, 140, 20];
    const DARK: [u8; 3] = [120, 20, 20];
    if !(0.25..0.75).contains(&v) {
        return RED;
    }
    // The arms in units of the flag's height, their middle at a third of
    // the width.
    let (x, y) = ((u - 1. / 3.) * 1.5, v - 0.5);
    // The pillars.
    if (0.07..0.1).contains(&x.abs()) && (-0.1..0.13).contains(&y) {
        return if y < -0.07 { GOLD } else { [215, 210, 200] };
    }
    // The crown.
    if x.abs() < 0.065 && (-0.16..-0.1).contains(&y) {
        return if y < -0.13 && (x.abs() * 40.) as i32 % 2 == 1 {
            YELLOW
        } else {
            GOLD
        };
    }
    // The shield, rounded below, quartered: Castile, León, Aragon,
    // Navarre.
    let inside = x.abs() < 0.06 && y > -0.1 && (y < 0.06 || x * x + (y - 0.06).powi(2) < 0.0036);
    if inside {
        if x.abs() > 0.05 || y < -0.09 || y > 0.11 {
            return DARK;
        }
        return match (x < 0., y < 0.) {
            (true, true) => RED,
            (false, true) => [235, 225, 210],
            (true, false) => {
                if (x * 60.) as i32 % 2 == 0 {
                    YELLOW
                } else {
                    RED
                }
            }
            (false, false) => [190, 30, 40],
        };
    }
    YELLOW
}

/// Green at the hoist, two fifths, then red; on the line between them the
/// arms, made small: the gold armillary sphere round the shield, red
/// edged, white within.
fn portugal(u: f32, v: f32) -> [u8; 3] {
    const GREEN: [u8; 3] = [4, 106, 56];
    const RED: [u8; 3] = [218, 41, 28];
    const GOLD: [u8; 3] = [250, 210, 0];
    const BLUE: [u8; 3] = [0, 56, 168];
    // The arms in units of the flag's height, their middle on the line.
    let (x, y) = ((u - 0.4) * 1.5, v - 0.5);
    let inside = x.abs() < 0.08 && y > -0.1 && (y < 0.04 || x * x + (y - 0.04).powi(2) < 0.0064);
    if inside {
        let inner =
            x.abs() < 0.045 && y > -0.065 && (y < 0.04 || x * x + (y - 0.04).powi(2) < 0.002);
        return if !inner {
            RED
        } else if x.abs() < 0.015 || y.abs() < 0.015 {
            BLUE
        } else {
            WHITE
        };
    }
    let r = (x * x + y * y).sqrt();
    if (0.15..0.2).contains(&r) || (r < 0.2 && (y - 0.35 * x).abs() < 0.022) {
        return GOLD;
    }
    if u < 0.4 { GREEN } else { RED }
}

/// The Union Flag, on its own measures (60 by 30) laid over the flag's
/// shape: St Andrew's white saltire with St Patrick's red counterchanged
/// (the broad white uppermost at the hoist, each red arm turned the same
/// way round), St George's red cross edged in white.
fn union(u: f32, v: f32) -> [u8; 3] {
    const BLUE: [u8; 3] = [1, 33, 105];
    const RED: [u8; 3] = [200, 16, 46];
    let (x, y) = (u * 60. - 30., v * 30. - 15.);
    if x.abs() < 3. || y.abs() < 3. {
        return RED;
    }
    if x.abs() < 5. || y.abs() < 5. {
        return WHITE;
    }
    // The arm towards the corner of this quarter, and the distance from
    // its middle line, counted towards the side a quarter turn back.
    let out = [30. * x.signum(), 15. * y.signum()];
    let length = (out[0] * out[0] + out[1] * out[1]).sqrt();
    let side = (x * out[1] - y * out[0]) / length;
    if (0.0..2.).contains(&side) {
        RED
    } else if side.abs() < 3. {
        WHITE
    } else {
        BLUE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(b: &Bitmap, x: u32, y: u32) -> [u8; 3] {
        let k = ((y * b.width + x) * 4) as usize;
        [b.rgba[k], b.rgba[k + 1], b.rgba[k + 2]]
    }

    #[test]
    fn each_flag_shows_its_colours_where_they_belong() {
        let red = |c: [u8; 3]| c[0] > 150 && c[1] < 80 && c[2] < 90;
        let white = |c: [u8; 3]| c.iter().all(|&v| v > 200);
        let blue = |c: [u8; 3]| c[2] > 60 && c[0] < 40;
        let it = flag(Language::Italian);
        assert!(at(&it, 3, 8)[1] > 100 && white(at(&it, 12, 8)) && red(at(&it, 20, 8)));
        let fr = flag(Language::French);
        assert!(blue(at(&fr, 3, 8)) && white(at(&fr, 12, 8)) && red(at(&fr, 20, 8)));
        let de = flag(Language::German);
        assert!(at(&de, 12, 3)[0] < 60 && red(at(&de, 12, 8)) && at(&de, 12, 13)[1] > 150);
        let pl = flag(Language::Polish);
        assert!(white(at(&pl, 12, 4)) && red(at(&pl, 12, 12)));
        let es = flag(Language::Spanish);
        assert!(red(at(&es, 18, 2)) && at(&es, 18, 8)[1] > 150 && red(at(&es, 18, 14)));
        // Green at the hoist, red at the fly, the gold sphere on the line.
        let pt = flag(Language::Portuguese);
        let gold = |c: [u8; 3]| c[0] > 180 && c[1] > 150 && c[2] < 90;
        assert!(at(&pt, 2, 8)[1] > 70 && at(&pt, 2, 8)[0] < 40 && red(at(&pt, 20, 8)));
        assert!((3..7).any(|y| gold(at(&pt, 9, y)) || gold(at(&pt, 10, y))));
        // The Union Flag: the red cross in the middle, blue in the
        // quarters, white along the cross.
        let uk = flag(Language::English);
        assert!(red(at(&uk, 12, 8)) && red(at(&uk, 12, 2)) && red(at(&uk, 3, 8)));
        assert!(blue(at(&uk, 7, 2)) && blue(at(&uk, 16, 14)));
        // A dark frame round each.
        for l in Language::ALL {
            let f = flag(l);
            assert!(at(&f, 0, 0)[0] < 60 && at(&f, 23, 15)[0] < 60, "{l:?}");
        }
    }

    /// The broad white is uppermost at the hoist: in the upper hoist
    /// quarter the red arm of the saltire lies below its middle line.
    #[test]
    fn the_union_flag_is_the_right_way_up() {
        // Points on the upper hoist arm (from the middle towards the
        // top-left corner), just above and just below its middle line.
        let (x, y) = (-18., -9.);
        let above = union((x + 30.) / 60., (y - 1.5 + 15.) / 30.);
        let below = union((x + 30.) / 60., (y + 1.5 + 15.) / 30.);
        assert_eq!(above, WHITE);
        assert_eq!(below, [200, 16, 46]);
    }
}
