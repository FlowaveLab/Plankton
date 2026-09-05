use std::{collections::HashSet, fs, path::PathBuf};

use tauri::image::Image;

fn tauri_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn load_png(relative_path: &str) -> Image<'static> {
    let path = tauri_root().join(relative_path);
    Image::from_bytes(&fs::read(&path).unwrap_or_else(|error| {
        panic!("failed to read {}: {error}", path.display());
    }))
    .unwrap_or_else(|error| panic!("failed to decode {}: {error}", path.display()))
    .to_owned()
}

fn opaque_pixels<'a>(image: &'a Image<'_>) -> impl Iterator<Item = (u32, u32, [u8; 4])> + 'a {
    let width = image.width();
    image
        .rgba()
        .chunks_exact(4)
        .enumerate()
        .filter(|(_, pixel)| pixel[3] >= 96)
        .map(move |(index, pixel)| {
            (
                index as u32 % width,
                index as u32 / width,
                [pixel[0], pixel[1], pixel[2], pixel[3]],
            )
        })
}

#[test]
fn tray_mark_is_left_right_symmetric_with_a_central_spine_and_open_body() {
    for (relative_path, size) in [
        ("assets/tray/generated/macos/plankton-trayTemplate.png", 32),
        (
            "assets/tray/generated/macos/plankton-trayTemplate@2x.png",
            64,
        ),
    ] {
        let image = load_png(relative_path);
        assert_eq!((image.width(), image.height()), (size, size));
        let alpha = |x: u32, y: u32| image.rgba()[((y * size + x) * 4 + 3) as usize];
        let mut mismatches = 0;
        for y in 0..size {
            for x in 0..size / 2 {
                if alpha(x, y).abs_diff(alpha(size - 1 - x, y)) > 32 {
                    mismatches += 1;
                }
            }
        }
        assert!(
            mismatches <= 4,
            "{relative_path} is not mirror symmetric: {mismatches}"
        );
        assert!(
            alpha(size / 2, size / 4) >= 96,
            "{relative_path} must retain its central spine at small sizes"
        );
        let spine_tip = opaque_pixels(&image)
            .filter(|(x, _, _)| x.abs_diff(size / 2) <= 1)
            .map(|(_, y, _)| y)
            .min()
            .expect("central spine must be visible");
        let antenna_tip = opaque_pixels(&image)
            .filter(|(x, _, _)| x.abs_diff(size / 2) > size / 8)
            .map(|(_, y, _)| y)
            .min()
            .expect("paired antennae must be visible");
        assert!(
            spine_tip < antenna_tip,
            "central spine must reach above the antennae"
        );
        assert_eq!(
            alpha(size / 2, size * 45 / 64),
            0,
            "body cutout must be transparent"
        );
        assert!(
            alpha(size / 2, size * 55 / 64) >= 200,
            "rounded body must remain solid"
        );
    }
}

#[test]
fn macos_template_is_monochrome_black_with_transparency() {
    for relative_path in [
        "assets/tray/generated/macos/plankton-trayTemplate.png",
        "assets/tray/generated/macos/plankton-trayTemplate@2x.png",
    ] {
        let image = load_png(relative_path);
        assert!(
            image.rgba().chunks_exact(4).any(|pixel| pixel[3] == 0),
            "{relative_path} must retain a transparent background"
        );
        assert!(
            opaque_pixels(&image).all(|(_, _, pixel)| pixel[..3] == [0, 0, 0]),
            "{relative_path} must contain black template pixels only"
        );
    }
}

#[test]
fn windows_tray_assets_cover_light_and_dark_shells() {
    for (relative_path, expected_rgb) in [
        (
            "assets/tray/generated/windows/plankton-tray-light-32.png",
            [17, 24, 39],
        ),
        (
            "assets/tray/generated/windows/plankton-tray-dark-32.png",
            [255, 255, 255],
        ),
    ] {
        let image = load_png(relative_path);
        assert_eq!((image.width(), image.height()), (32, 32));
        assert!(image.rgba().chunks_exact(4).any(|pixel| pixel[3] == 0));
        assert!(
            opaque_pixels(&image)
                .filter(|(_, _, pixel)| pixel[3] == 255)
                .all(|(_, _, pixel)| pixel[..3] == expected_rgb),
            "{relative_path} contains an unexpected foreground color"
        );
    }
}

#[test]
fn reasoning_spinner_frames_are_dedicated_and_platform_themed() {
    let idle = load_png("assets/tray/generated/macos/plankton-trayTemplate.png");
    let mut macos_frames = Vec::new();
    let mut all_spinner_frames = HashSet::new();

    for frame in 0..8 {
        let relative_path =
            format!("assets/tray/generated/macos/plankton-tray-spinnerTemplate-{frame}.png");
        let image = load_png(&relative_path);
        assert_eq!((image.width(), image.height()), (32, 32));
        assert!(
            image.rgba().chunks_exact(4).any(|pixel| pixel[3] == 0),
            "{relative_path} must retain a transparent background"
        );
        assert!(
            image
                .rgba()
                .chunks_exact(4)
                .filter(|pixel| pixel[3] > 0)
                .all(|pixel| pixel[..3] == [0, 0, 0]),
            "{relative_path} must contain black template pixels only"
        );
        let alphas = image
            .rgba()
            .chunks_exact(4)
            .filter(|pixel| pixel[3] > 0)
            .map(|pixel| pixel[3])
            .collect::<HashSet<_>>();
        assert!(
            alphas.iter().any(|alpha| *alpha < 96) && alphas.contains(&255),
            "{relative_path} must preserve both the opaque lead and translucent trail"
        );
        assert_ne!(
            image.rgba(),
            idle.rgba(),
            "{relative_path} must be a spinner frame, not the brand mark"
        );
        let pixels = image.rgba().to_vec();
        assert!(
            all_spinner_frames.insert(pixels.clone()),
            "{relative_path} duplicates another spinner frame"
        );
        macos_frames.push(pixels);
    }

    for frame in 0..8 {
        assert_ne!(
            macos_frames[frame],
            macos_frames[(frame + 1) % 8],
            "adjacent spinner frames must be visually distinct"
        );
    }

    for (variant, expected_rgb) in [("light", [17, 24, 39]), ("dark", [255, 255, 255])] {
        let brand = load_png(&format!(
            "assets/tray/generated/windows/plankton-tray-{variant}-32.png"
        ));
        for frame in 0..8 {
            let relative_path = format!(
                "assets/tray/generated/windows/plankton-tray-spinner-{variant}-32-{frame}.png"
            );
            let image = load_png(&relative_path);
            assert_eq!((image.width(), image.height()), (32, 32));
            assert!(image.rgba().chunks_exact(4).any(|pixel| pixel[3] == 0));
            let visible_pixels = image
                .rgba()
                .chunks_exact(4)
                .filter(|pixel| pixel[3] > 0)
                .collect::<Vec<_>>();
            let max_weighted_color_delta = visible_pixels
                .iter()
                .map(|pixel| {
                    pixel[..3]
                        .iter()
                        .zip(expected_rgb)
                        .map(|(actual, expected)| {
                            u16::from(actual.abs_diff(expected)) * u16::from(pixel[3])
                        })
                        .max()
                        .unwrap_or_default()
                })
                .max()
                .unwrap_or_default();
            assert!(
                max_weighted_color_delta <= 512,
                "{relative_path} has premultiplied theme-color error {max_weighted_color_delta}"
            );
            let alphas = visible_pixels
                .iter()
                .map(|pixel| pixel[3])
                .collect::<HashSet<_>>();
            assert!(
                alphas.iter().any(|alpha| *alpha < 96) && alphas.contains(&255),
                "{relative_path} must preserve both the opaque lead and translucent trail"
            );
            assert_ne!(
                image.rgba(),
                brand.rgba(),
                "{relative_path} must be a spinner frame, not the brand mark"
            );
            assert!(
                all_spinner_frames.insert(image.rgba().to_vec()),
                "{relative_path} duplicates another spinner frame"
            );
        }
    }

    assert_eq!(
        all_spinner_frames.len(),
        24,
        "all macOS and Windows theme spinner frames must be globally unique"
    );
}

#[test]
fn app_icon_containers_and_canonical_png_are_valid() {
    let icon = load_png("icons/icon.png");
    assert_eq!((icon.width(), icon.height()), (512, 512));

    let ico_path = tauri_root().join("icons/icon.ico");
    let ico = fs::read(&ico_path).expect("Windows ICO must exist");
    assert_eq!(&ico[..4], &[0, 0, 1, 0]);
    let ico_entry_count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    let mut ico_sizes = (0..ico_entry_count)
        .map(|index| {
            let offset = 6 + index * 16;
            (
                if ico[offset] == 0 {
                    256
                } else {
                    ico[offset] as u16
                },
                if ico[offset + 1] == 0 {
                    256
                } else {
                    ico[offset + 1] as u16
                },
            )
        })
        .collect::<Vec<_>>();
    ico_sizes.sort_unstable();
    assert_eq!(
        ico_sizes,
        [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (256, 256)],
        "Windows ICO must contain the standard tray and taskbar sizes"
    );

    let icns_path = tauri_root().join("icons/icon.icns");
    let icns = fs::read(&icns_path).expect("macOS ICNS must exist");
    assert_eq!(&icns[..4], b"icns");
    assert_eq!(
        u32::from_be_bytes([icns[4], icns[5], icns[6], icns[7]]) as usize,
        icns.len(),
        "ICNS header length must match the file"
    );
    for chunk_type in [b"ic07", b"ic08", b"ic09", b"ic10", b"ic13", b"ic14"] {
        assert!(
            icns.windows(4).any(|window| window == chunk_type),
            "ICNS is missing {}",
            String::from_utf8_lossy(chunk_type)
        );
    }
}

#[test]
fn macos_app_icon_keeps_a_dock_safe_area() {
    let icon = load_png("icons/macos-icon.png");
    assert_eq!((icon.width(), icon.height()), (512, 512));

    let opaque = opaque_pixels(&icon).collect::<Vec<_>>();
    let min_x = opaque.iter().map(|(x, _, _)| *x).min().unwrap_or_default();
    let max_x = opaque.iter().map(|(x, _, _)| *x).max().unwrap_or_default();
    let min_y = opaque.iter().map(|(_, y, _)| *y).min().unwrap_or_default();
    let max_y = opaque.iter().map(|(_, y, _)| *y).max().unwrap_or_default();

    assert!(
        min_x >= 48 && min_y >= 48 && max_x <= 463 && max_y <= 463,
        "macOS app artwork must leave a balanced transparent Dock margin; bounds=({min_x},{min_y})-({max_x},{max_y})"
    );
}
