//! Builds the Core Audio, AppKit and AVFoundation bridges on macOS, or the
//! XAudio2, Media Foundation and GDI bridges on Windows, using the target's
//! C++ compiler: cl for windows-msvc, the configured CXX for windows-gnu.
//! With MSVC the game's executable also gets Tzar's icon.

fn main() {
    println!("cargo:rerun-if-changed=tools/audio_mac.cpp");
    println!("cargo:rerun-if-changed=tools/dialog_mac.mm");
    println!("cargo:rerun-if-changed=tools/media_mac.mm");
    println!("cargo:rerun-if-changed=tools/audio_win.cpp");
    println!("cargo:rerun-if-changed=tools/media_win.cpp");
    println!("cargo:rerun-if-changed=tools/text_win.cpp");
    println!("cargo:rerun-if-changed=tools/tzar.ico");
    println!("cargo:rerun-if-env-changed=CXX");
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("apple-darwin") {
        cc::Build::new()
            .cpp(true)
            .file("tools/audio_mac.cpp")
            .flag("-std=c++17")
            .flag("-fno-exceptions")
            .flag("-fno-rtti")
            .opt_level(2)
            .compile("tz_audio");
        println!("cargo:rustc-link-lib=framework=AudioToolbox");
        println!("cargo:rustc-link-lib=framework=AudioUnit");
        cc::Build::new()
            .cpp(true)
            .file("tools/dialog_mac.mm")
            .file("tools/media_mac.mm")
            .flag("-std=c++17")
            .flag("-fobjc-arc")
            .opt_level(2)
            .compile("tz_dialog");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=AVFoundation");
        println!("cargo:rustc-link-lib=framework=CoreMedia");
        println!("cargo:rustc-link-lib=framework=CoreVideo");
        return;
    }
    if !target.contains("windows") {
        return;
    }
    if target.ends_with("windows-msvc") {
        // The Italian edition's golden Z (its launcher's icon, the richer
        // copy of Tzar.exe's own), as a .res the linker takes as it is.
        let ico = std::fs::read("tools/tzar.ico").expect("tools/tzar.ico");
        let res = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("tzar.res");
        std::fs::write(&res, icon_resource(&ico)).unwrap();
        println!("cargo:rustc-link-arg-bin=retzar={}", res.display());
    }
    let mut build = cc::Build::new();
    build
        .cpp(true)
        // The bridge uses no C++ library features: plain C ABI, no exceptions.
        .cpp_link_stdlib(None)
        .file("tools/audio_win.cpp")
        .file("tools/media_win.cpp")
        .file("tools/text_win.cpp")
        .opt_level(2);
    if target.ends_with("windows-gnu") {
        build
            .flag("-std=c++17")
            .flag("-fno-exceptions")
            .flag("-fno-rtti");
    } else {
        build.flag_if_supported("/std:c++17");
    }
    build.compile("tz_audio");
    println!("cargo:rustc-link-lib=ole32");
    println!("cargo:rustc-link-lib=winmm");
    println!("cargo:rustc-link-lib=uuid");
    println!("cargo:rustc-link-lib=gdi32");
    println!("cargo:rustc-link-lib=mfplat");
    println!("cargo:rustc-link-lib=mfreadwrite");
    println!("cargo:rustc-link-lib=mfuuid");
    println!("cargo:rustc-link-lib=shlwapi");
    if target.ends_with("windows-gnu") {
        // Keep third-party cache reusable while removing oversized DWARF data
        // from final Windows binaries and test executables.
        println!("cargo:rustc-link-arg=-Wl,--strip-debug");
    }
}

/// A compiled resource file holding the images of an .ico as `RT_ICON`
/// 1, 2, … and their directory as `RT_GROUP_ICON` 1, the program's icon
/// (`frontend::window_icon` loads it for the window).
fn icon_resource(ico: &[u8]) -> Vec<u8> {
    let u16_at = |at: usize| u16::from_le_bytes([ico[at], ico[at + 1]]);
    let u32_at = |at: usize| u32::from_le_bytes([ico[at], ico[at + 1], ico[at + 2], ico[at + 3]]);
    let mut out = Vec::new();
    // Every .res file opens with an empty resource.
    resource(&mut out, 0, 0, 0, &[]);
    let count = u16_at(4);
    let mut group = [0u16, 1, count].map(u16::to_le_bytes).concat();
    for i in 0..count {
        // ICONDIRENTRY: the size and colours, then where the image is.
        let at = 6 + 16 * i as usize;
        let (size, offset) = (u32_at(at + 8) as usize, u32_at(at + 12) as usize);
        resource(&mut out, 3, i + 1, 0x1010, &ico[offset..offset + size]);
        // GRPICONDIRENTRY: the same twelve bytes, then the image's id.
        group.extend_from_slice(&ico[at..at + 12]);
        group.extend_from_slice(&(i + 1).to_le_bytes());
    }
    resource(&mut out, 14, 1, 0x1030, &group);
    out
}
/// One resource with a numeric type and id, in the neutral language.
fn resource(out: &mut Vec<u8>, kind: u16, id: u16, flags: u16, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    for word in [0xffff, kind, 0xffff, id] {
        out.extend_from_slice(&u16::to_le_bytes(word));
    }
    // Data version, memory flags, language, version, characteristics.
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&[0; 10]);
    out.extend_from_slice(data);
    out.resize(out.len().next_multiple_of(4), 0);
}
