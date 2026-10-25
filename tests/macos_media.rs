//! Opt-in check against the player's own installation, with no game data in git.
#![cfg(target_os = "macos")]

use retzar::{
    ddv,
    media::{Clip, Kind, Media, Source},
};
use std::{path::PathBuf, sync::Arc};

#[test]
#[ignore = "Requires RETZAR_MPEG_TEST_FILE pointing at an MPEG-1 movie"]
fn mpeg_movie_decodes_on_macos() {
    let path = std::env::var_os("RETZAR_MPEG_TEST_FILE").expect("Set RETZAR_MPEG_TEST_FILE");
    let bytes = std::fs::read(path).unwrap();
    let mut media = Media::open(&bytes, Kind::Mpeg).unwrap();
    assert!(media.width > 0 && media.height > 0 && media.seconds > 0.);
    let audio = media.audio.take().expect("MPEG soundtrack");
    assert!(!audio.bytes.is_empty());
    let (_, picture) = media.next_frame().unwrap().expect("First MPEG frame");
    assert_eq!((picture.width, picture.height), (media.width, media.height));
}

#[test]
#[ignore = "Requires TZAR_DATA_ROOT pointing at an original installation with DDV movies"]
fn original_ddv_converts_to_mp4_on_macos() {
    let source = PathBuf::from(std::env::var_os("TZAR_DATA_ROOT").expect("Set TZAR_DATA_ROOT"));
    let archive = ddv::american_movies(&source).expect("DDV movies");
    let tables = Arc::new(ddv::Tables::of_installation(&source).expect("DDV decoding tables"));
    let entry = archive
        .entries
        .iter()
        .find(|e| retzar::media::movie_name(&e.name) == "hmm")
        .expect("Haemimont logo movie");
    let bytes = archive.read(entry).unwrap();
    let original = ddv::Movie::open(bytes.clone()).unwrap();
    let path = std::env::temp_dir().join(format!("retzar-ddv-mac-{}.mp4", std::process::id()));
    retzar::media::to_mp4(Source::Ddv(bytes, tables), &path, 90).unwrap();
    let mut converted = Media::open(&std::fs::read(&path).unwrap(), Kind::Mp4).unwrap();
    assert_eq!(
        (converted.width, converted.height),
        (original.width, original.height)
    );
    assert!(
        converted
            .audio
            .as_ref()
            .is_some_and(|p| !p.bytes.is_empty())
    );
    let mut frames = 0;
    while converted.next_frame().unwrap().is_some() {
        frames += 1;
    }
    assert_eq!(frames, original.frames());
    assert!((converted.seconds - original.frames() as f64 / original.fps as f64).abs() < 0.1);
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "Requires TZAR_DATA_ROOT pointing at an original installation with DDV movies"]
fn original_ddv_movies_decode_on_macos_with_sound_and_frames() {
    let source = PathBuf::from(std::env::var_os("TZAR_DATA_ROOT").expect("Set TZAR_DATA_ROOT"));
    let archive = ddv::american_movies(&source).expect("DDV movies");
    let tables = Arc::new(ddv::Tables::of_installation(&source).expect("DDV decoding tables"));
    let mut decoded = 0;
    for entry in &archive.entries {
        if !entry.name.to_ascii_lowercase().ends_with(".ddv") {
            continue;
        }
        let mut clip = Clip::open(Source::Ddv(archive.read(entry).unwrap(), tables.clone()))
            .unwrap_or_else(|e| panic!("{}: {e}", entry.name));
        assert!(clip.width > 0 && clip.height > 0 && clip.seconds > 0.);
        let audio = clip.audio.take().expect("Movie soundtrack");
        assert!(audio.rate > 0 && [1, 2].contains(&audio.channels));
        assert_eq!(audio.bits, 16);
        assert!(!audio.bytes.is_empty());
        assert!(
            (audio.seconds() - clip.seconds).abs() < 1.,
            "{}",
            entry.name
        );
        let frame = clip.frame_at(0.).unwrap().expect("First frame");
        assert_eq!((frame.width, frame.height), (clip.width, clip.height));
        assert_eq!(
            frame.rgba.len(),
            clip.width as usize * clip.height as usize * 4
        );
        // Dropping early also exercises the bounded decoder worker's cleanup.
        decoded += 1;
    }
    assert!(decoded > 0);
}
