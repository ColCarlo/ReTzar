//! «Testi in alta definizione» of the «GRAFICA» page, an addition of this
//! version. The menu and the interface are drawn as always, but with their
//! texts recorded instead of painted (`font::record`); a text nothing drew
//! over afterwards comes back in Windows' Arial Bold at the screen's
//! resolution, as a picture of its own over the interface's, stretched to
//! the original font's width so nothing moves; a text partly covered keeps
//! the original font where it still shows.
use super::view::{Surface, UiLayout};
use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::RenderLayers,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use retzar::{
    bitmap::Bitmap,
    font::{self, Recording, TextOp},
};
use std::collections::HashMap;

/// The texts a recorded picture keeps for the screen. A text something was
/// drawn over afterwards (a hint box, a greyed control) takes back its
/// pixels from `original`, the picture drawn as always.
pub fn resolve(
    b: &mut Bitmap,
    mut rec: Recording,
    original: impl FnOnce() -> Bitmap,
) -> Vec<TextOp> {
    let ops = std::mem::take(&mut rec.ops);
    let (sharp, back): (Vec<_>, Vec<_>) = ops.into_iter().enumerate().partition(|(k, op)| {
        let k = (k + 1).min(u16::MAX as usize);
        op.pixels.iter().all(|&(i, _)| rec.shows(k, i))
    });
    if !back.is_empty() {
        let original = original();
        for (_, op) in &back {
            for &(i, _) in &op.pixels {
                b.rgba[i * 4..i * 4 + 4].copy_from_slice(&original.rgba[i * 4..i * 4 + 4]);
            }
        }
    }
    sharp.into_iter().map(|(_, op)| op).collect()
}

/// The sharp texts of one picture (menu or interface) and where it lies.
#[derive(Default)]
pub struct Texts {
    pub ops: Vec<TextOp>,
    pub layout: Option<UiLayout>,
    pub z: f32,
    pub shown: bool,
}
#[derive(Resource, Default)]
pub struct HdText {
    pub menu: Texts,
    pub hud: Texts,
    sprites: [Vec<Entity>; 2],
    /// Pictures by text, height, width and colour.
    cache: HashMap<(String, u32, u32, [u8; 3]), Handle<Image>>,
}

/// One text in Arial Bold, `width × height` screen pixels for the cell the
/// original font gives it.
fn picture(op: &TextOp, width: u32, height: u32) -> Option<Image> {
    // Arial's capitals to descenders are about four fifths of the cell.
    let size = (height as f32 * 0.8).round().max(4.) as u32;
    let (w, h, cover) = font::arial(&op.text, size)?;
    let (pw, ph) = (width.max(1), height.max(1));
    let mut data = vec![0u8; (pw * ph * 4) as usize];
    let top = (ph as i32 - h as i32) / 2;
    for y in 0..ph {
        let sy = y as i32 - top;
        if !(0..h as i32).contains(&sy) {
            continue;
        }
        for x in 0..pw {
            // Stretched across to the original font's width.
            let fx = (x as f32 + 0.5) * w as f32 / pw as f32 - 0.5;
            let x0 = fx.floor().clamp(0., (w - 1) as f32) as u32;
            let x1 = (x0 + 1).min(w - 1);
            let t = (fx - x0 as f32).clamp(0., 1.);
            let row = sy as u32 * w;
            let c = cover[(row + x0) as usize] as f32 * (1. - t)
                + cover[(row + x1) as usize] as f32 * t;
            let at = ((y * pw + x) * 4) as usize;
            data[at..at + 3].copy_from_slice(&op.color);
            data[at + 3] = c.round() as u8;
        }
    }
    Some(Image::new(
        Extent3d {
            width: pw,
            height: ph,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    ))
}

/// Lays the sharp texts out as pictures over their interface pictures.
pub fn update(
    mut commands: Commands,
    mut hd: ResMut<HdText>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut sprites: Query<(&mut Sprite, &mut Transform, &mut Visibility)>,
) {
    let surface = Surface::of(&window);
    let hd = &mut *hd;
    let mut used = std::collections::HashSet::new();
    for which in 0..2 {
        let texts = if which == 0 {
            &mut hd.menu
        } else {
            &mut hd.hud
        };
        let pool = &mut hd.sprites[which];
        let mut placed = 0;
        if let (true, Some(layout)) = (texts.shown, texts.layout) {
            for (k, op) in texts.ops.iter().enumerate() {
                // Whole screen pixels, the picture 1:1 on them.
                let corner = (layout.origin + Vec2::new(op.x, op.y) * layout.factor).round();
                let w = (op.width * layout.factor).round().max(1.) as u32;
                let h = (op.height * layout.factor).round().max(1.) as u32;
                let key = (op.text.clone(), h, w, op.color);
                let image = match hd.cache.get(&key) {
                    Some(image) => image.clone(),
                    None => {
                        let Some(picture) = picture(op, w, h) else {
                            continue;
                        };
                        let image = images.add(picture);
                        hd.cache.insert(key.clone(), image.clone());
                        image
                    }
                };
                used.insert(key);
                let size = Vec2::new(w as f32, h as f32) / layout.factor;
                let at = (corner - layout.origin) / layout.factor;
                let centre = layout.place(surface, at, size);
                // Over each other in the order they were drawn.
                let z = texts.z + 0.5 + k as f32 * 1e-4;
                let place = Transform::from_translation(centre.extend(z));
                if let Some(&e) = pool.get(placed) {
                    if let Ok((mut s, mut t, mut v)) = sprites.get_mut(e) {
                        if s.image != image {
                            s.image = image;
                        }
                        s.custom_size = Some(size);
                        t.set_if_neq(place);
                        v.set_if_neq(Visibility::Inherited);
                    }
                } else {
                    let e = commands
                        .spawn((
                            Sprite {
                                custom_size: Some(size),
                                ..Sprite::from_image(image)
                            },
                            place,
                            RenderLayers::layer(2),
                        ))
                        .id();
                    pool.push(e);
                }
                placed += 1;
            }
        }
        for &e in &pool[placed.min(pool.len())..] {
            if let Ok((_, _, mut v)) = sprites.get_mut(e) {
                v.set_if_neq(Visibility::Hidden);
            }
        }
    }
    // Pictures no text uses any more.
    hd.cache.retain(|key, image| {
        let keep = used.contains(key);
        if !keep {
            images.remove(image.id());
        }
        keep
    });
}
