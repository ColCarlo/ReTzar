//! A separate full-window camera keeps the movie independent of menu scaling.
use super::*;
use bevy::window::CursorOptions;
use retzar::{
    ddv,
    media::{self, Clip, Kind, Source},
};
use std::{
    collections::VecDeque,
    path::Path,
    time::{Duration, Instant},
};

/// What to play: movies one after the other, each skipped by itself.
pub struct Request(pub Vec<Source>);
impl Request {
    /// What the game plays at start in the language of the moment. In the
    /// installation's own language its launcher's intro, when it has one
    /// (the Italian edition's), and in Spanish the Spanish edition's of the
    /// pack; otherwise the American release's sequence (0x476318–0x4764ac),
    /// when its movies are found; failing that, the other one.
    pub fn start(source: &Path) -> Option<Self> {
        if retzar::lang::current() == retzar::lang::Language::Spanish
            && !retzar::lang::native()
            && let Some(bytes) = media::converted(source, media::SPANISH_INTRO)
        {
            return Some(Self(vec![Source::Media(bytes, Kind::Mp4)]));
        }
        if retzar::lang::native() {
            Self::intro(source).or_else(|| Self::american(source))
        } else {
            Self::american(source).or_else(|| Self::intro(source))
        }
    }
    /// The Italian launcher's intro: the portable copy's MP4, or
    /// `Avance\intro.MPG` of the installation.
    pub(crate) fn intro(source: &Path) -> Option<Self> {
        if let Some(bytes) = media::converted(source, "intro") {
            return Some(Self(vec![Source::Media(bytes, Kind::Mp4)]));
        }
        let bytes = retzar::vfs::read(&media::italian_intro(source)?).ok()?;
        Some(Self(vec![Source::Media(bytes, Kind::Mpeg)]))
    }
    pub(crate) fn american(source: &Path) -> Option<Self> {
        Self::movies(source, &ddv::START)
    }
    /// A movie a script plays (`@playmovie`, 0x4b8430 → 0x47fc70), such as
    /// `movies\outro.ddv` at the end of the American campaign: a DDV of the
    /// American release's archive. None when it is not there: the original
    /// then shows nothing (0x47fcfb).
    pub(crate) fn named(source: &Path, name: &str) -> Option<Self> {
        Self::movies(source, &[name])
    }
    /// The American movies by their names in `movies.wdt`: the portable
    /// copy's MP4, or the DDV of the archive decoded with the tables of the
    /// installation's `Tzar.exe`.
    fn movies(source: &Path, names: &[&str]) -> Option<Self> {
        let mut archive = None;
        let mut tables = None;
        let mut clips = Vec::new();
        for name in names {
            if let Some(bytes) = media::converted(source, &media::movie_name(name)) {
                clips.push(Source::Media(bytes, Kind::Mp4));
                continue;
            }
            if archive.is_none() {
                archive = ddv::american_movies(source);
                tables = ddv::Tables::of_installation(source).ok().map(Arc::new);
            }
            if let (Some(pack), Some(tables)) = (&archive, &tables)
                && let Ok(bytes) = pack.get(name)
            {
                clips.push(Source::Ddv(bytes, tables.clone()));
            }
        }
        (!clips.is_empty()).then_some(Self(clips))
    }
}

/// The size of the movie on screen.
#[derive(Clone, Copy, Debug, Default)]
pub struct Status {
    pub width: u32,
    pub height: u32,
}

/// The movies playing: the one on screen and those to come.
struct Run {
    queue: VecDeque<Source>,
    playing: Option<Playing>,
}
struct Playing {
    clip: Clip,
    /// When the clip's time started, its sound with it; none until its
    /// first picture is on screen and the sound has been handed over.
    started: Option<Instant>,
    /// Since when the window has been inactive, the clip halted.
    halted: Option<Instant>,
    shown: usize,
}
impl Playing {
    fn elapsed(&self) -> Duration {
        match (self.started, self.halted) {
            (Some(s), Some(h)) => h - s,
            (Some(s), None) => s.elapsed(),
            _ => Duration::ZERO,
        }
    }
    /// The sound from `at` on.
    fn sound_from(&self, at: Duration) -> Option<retzar::audio::Pcm> {
        let pcm = self.clip.audio.as_ref()?;
        let frame = (pcm.channels as usize * pcm.bits as usize / 8).max(1);
        let skip = ((at.as_secs_f64() * pcm.rate as f64) as usize * frame).min(pcm.bytes.len());
        Some(retzar::audio::Pcm {
            bytes: pcm.bytes[skip..].to_vec(),
            ..*pcm
        })
    }
}

#[derive(Resource, Default)]
pub struct Movie {
    pub request: Option<Request>,
    run: Option<Run>,
    pub status: Status,
    block_input: bool,
    /// Restore the caller's cursor visibility, including skip/error paths.
    cursor_visible: Option<bool>,
}
impl Movie {
    pub fn active(&self) -> bool {
        self.run.is_some() || self.request.is_some()
    }
}
pub fn inactive(movie: Res<Movie>) -> bool {
    !movie.block_input && !movie.active()
}
#[derive(Component)]
pub(super) struct MovieCamera;
#[derive(Component)]
pub(super) struct MovieScreen;

fn black() -> Image {
    rgba_image(Bitmap {
        width: 1,
        height: 1,
        rgba: vec![0, 0, 0, 255],
    })
}

pub fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.spawn((
        Camera2d,
        bevy::core_pipeline::tonemapping::Tonemapping::None,
        Camera {
            order: 2,
            is_active: false,
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        MovieCamera,
        // All cameras sharing the window target must agree on sample count,
        // including the intro overlay over the menu/game cameras.
        Msaa::Off,
        CompositingSpace::Srgb,
        RenderLayers::layer(3),
    ));
    commands.spawn((
        Sprite::from_image(images.add(black())),
        MovieScreen,
        RenderLayers::layer(3),
    ));
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut movie: ResMut<Movie>,
    mut menu: ResMut<MenuState>,
    mut images: ResMut<Assets<Image>>,
    window: Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    audio: Option<Res<SoundPlayer>>,
    mut cameras: Query<(&mut Camera, &mut Projection), With<MovieCamera>>,
    mut sprites: Query<&mut Sprite, With<MovieScreen>>,
) {
    let (window, mut cursor) = window.into_inner();
    movie.block_input = movie.active();
    // Only inputs received while focused skip; do not leak the same Escape
    // or click into the underlying menu on this frame.
    let skip = window.focused
        && (keys.just_pressed(KeyCode::Escape)
            || keys.just_pressed(KeyCode::Space)
            || keys.just_pressed(KeyCode::Enter)
            || mouse.just_pressed(MouseButton::Left)
            || mouse.just_pressed(MouseButton::Right));
    let sound = |pcm: Option<retzar::audio::Pcm>| {
        if let Some(audio) = &audio {
            audio.player.movie(pcm);
        }
    };
    if let Some(Request(clips)) = movie.request.take() {
        // Replaying always starts at zero, with fresh clips and frame.
        if movie.run.take().is_some() {
            sound(None);
        }
        movie.status = Status::default();
        if let Ok(mut sprite) = sprites.single_mut() {
            super::renew_image(&mut images, &mut sprite, black());
        }
        movie.run = Some(Run {
            queue: clips.into(),
            playing: None,
        });
    } else if movie.run.is_some() {
        let run = movie.run.as_mut().unwrap();
        // Each movie of the sequence is skipped by itself (0x47fc70).
        if skip && run.playing.take().is_some() {
            sound(None);
        }
        let mut error = None;
        let mut frame = None;
        loop {
            if run.playing.is_none() {
                let Some(source) = run.queue.pop_front() else {
                    break;
                };
                match Clip::open(source) {
                    Ok(clip) => {
                        run.playing = Some(Playing {
                            clip,
                            started: None,
                            halted: None,
                            shown: 0,
                        })
                    }
                    Err(e) => {
                        error = Some(e.to_string());
                        continue;
                    }
                }
            }
            let playing = run.playing.as_mut().unwrap();
            if playing.started.is_none() {
                // The sound starts once the first picture is on screen, so
                // the player already knows the window has the focus.
                if window.focused && playing.shown > 0 {
                    sound(playing.sound_from(Duration::ZERO));
                    playing.started = Some(Instant::now());
                }
            } else if !window.focused && playing.halted.is_none() {
                playing.halted = Some(Instant::now());
                sound(None);
            } else if window.focused
                && let Some(halted) = playing.halted.take()
            {
                // Resume where it stopped.
                if let Some(s) = playing.started.as_mut() {
                    *s += halted.elapsed();
                }
                sound(playing.sound_from(playing.elapsed()));
            }
            let time = playing.elapsed().as_secs_f64();
            match playing.clip.frame_at(time) {
                Ok(Some(f)) => {
                    playing.shown += 1;
                    frame = Some(f);
                }
                Ok(None) => {}
                Err(e) => error = Some(e.to_string()),
            }
            let over = error.is_some() || (playing.started.is_some() && playing.clip.ended(time));
            if over {
                run.playing = None;
                sound(None);
                if error.is_some() {
                    run.queue.clear();
                }
                continue;
            }
            break;
        }
        if let Some(f) = frame {
            movie.status = Status {
                width: f.width,
                height: f.height,
            };
            let mut image = rgba_image(f);
            image.sampler = ImageSampler::linear();
            if let Ok(mut sprite) = sprites.single_mut() {
                super::renew_image(&mut images, &mut sprite, image);
            }
        }
        if let Some(error) = error {
            menu.notice = retzar::lang::fill("Impossibile riprodurre intro: {}", &[&error]);
            menu.dirty = true;
        }
        let run = movie.run.as_ref().unwrap();
        if run.playing.is_none() && run.queue.is_empty() {
            movie.run = None;
            movie.status = Status::default();
            menu.dirty = true;
        }
    }
    let active = movie.active();
    if active {
        movie.cursor_visible.get_or_insert(cursor.visible);
        cursor.visible = false;
    } else if let Some(saved) = movie.cursor_visible.take() {
        cursor.visible = saved;
    }
    for (mut camera, mut projection) in &mut cameras {
        camera.is_active = active;
        if let Projection::Orthographic(p) = &mut *projection {
            p.scale = window.scale_factor();
        }
    }
    if movie.status.width > 0 && movie.status.height > 0 {
        let size = fit(
            Surface::of(window).physical,
            Vec2::new(movie.status.width as f32, movie.status.height as f32),
        );
        for mut sprite in &mut sprites {
            sprite.custom_size = Some(size);
        }
    }
}

fn fit(window: Vec2, image: Vec2) -> Vec2 {
    image * (window.x / image.x).min(window.y / image.y)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn movie_preserves_aspect_ratio_at_all_window_sizes() {
        for size in [
            Vec2::new(800., 600.),
            Vec2::new(1920., 1080.),
            Vec2::new(600., 1000.),
        ] {
            let fitted = fit(size, Vec2::new(352., 288.));
            assert!(fitted.x <= size.x + 0.001 && fitted.y <= size.y + 0.001);
            assert!((fitted.x / fitted.y - 352. / 288.).abs() < 0.001);
            assert!((fitted.x - size.x).abs() < 0.001 || (fitted.y - size.y).abs() < 0.001);
        }
    }
}
