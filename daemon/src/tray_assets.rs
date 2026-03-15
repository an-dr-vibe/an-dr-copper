use resvg::{tiny_skia, usvg};

const COPPER_SERVER_SVG: &[u8] = include_bytes!("../assets/icons/copper-server.svg");
const PIN_DARK_SVG: &[u8] = include_bytes!("../assets/icons/pin-dark.svg");
const PIN_LIGHT_SVG: &[u8] = include_bytes!("../assets/icons/pin-light.svg");
const PIN_OFF_DARK_SVG: &[u8] = include_bytes!("../assets/icons/pin-off-dark.svg");
const PIN_OFF_LIGHT_SVG: &[u8] = include_bytes!("../assets/icons/pin-off-light.svg");

pub struct RenderedIcon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn copper_server_icon() -> Result<RenderedIcon, String> {
    render_svg_icon(COPPER_SERVER_SVG)
}

pub fn windows_display_pin_icon(pinned: bool, dark_variant: bool) -> Result<RenderedIcon, String> {
    let svg = match (pinned, dark_variant) {
        (true, true) => PIN_DARK_SVG,
        (true, false) => PIN_LIGHT_SVG,
        (false, true) => PIN_OFF_DARK_SVG,
        (false, false) => PIN_OFF_LIGHT_SVG,
    };
    render_svg_icon(svg)
}

fn render_svg_icon(svg: &[u8]) -> Result<RenderedIcon, String> {
    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data(svg, &options).map_err(|err| err.to_string())?;
    let size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height()).ok_or_else(|| {
        format!(
            "failed to allocate {}x{} tray pixmap",
            size.width(),
            size.height()
        )
    })?;
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    let mut pixels = pixmap.take();
    unpremultiply_rgba(&mut pixels);
    Ok(RenderedIcon {
        width: size.width(),
        height: size.height(),
        rgba: pixels,
    })
}

fn unpremultiply_rgba(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = pixel[3];
        if alpha == 0 {
            pixel[0] = 0;
            pixel[1] = 0;
            pixel[2] = 0;
            continue;
        }
        if alpha == u8::MAX {
            continue;
        }
        let alpha_u32 = u32::from(alpha);
        pixel[0] = unpremultiply_channel(pixel[0], alpha_u32);
        pixel[1] = unpremultiply_channel(pixel[1], alpha_u32);
        pixel[2] = unpremultiply_channel(pixel[2], alpha_u32);
    }
}

fn unpremultiply_channel(value: u8, alpha: u32) -> u8 {
    (((u32::from(value) * 255) + (alpha / 2)) / alpha).min(255) as u8
}

#[cfg(windows)]
pub fn create_hicon(
    icon: &RenderedIcon,
) -> Option<windows_sys::Win32::UI::WindowsAndMessaging::HICON> {
    use std::ptr;
    use windows_sys::Win32::UI::WindowsAndMessaging::{CreateIcon, HICON};

    let width = i32::try_from(icon.width).ok()?;
    let height = i32::try_from(icon.height).ok()?;
    let pixel_count = usize::try_from(width.checked_mul(height)?).ok()?;
    if icon.rgba.len() != pixel_count * 4 {
        return None;
    }

    let mut xor = vec![0u8; pixel_count * 4];
    let stride = ((width + 31) / 32 * 4) as usize;
    let mut and_mask = vec![0u8; stride * height as usize];

    for y in 0..height {
        for x in 0..width {
            let src_idx = ((y * width + x) * 4) as usize;
            let dst_y = height - 1 - y;
            let dst_idx = ((dst_y * width + x) * 4) as usize;
            let r = icon.rgba[src_idx];
            let g = icon.rgba[src_idx + 1];
            let b = icon.rgba[src_idx + 2];
            let a = icon.rgba[src_idx + 3];
            xor[dst_idx] = b;
            xor[dst_idx + 1] = g;
            xor[dst_idx + 2] = r;
            xor[dst_idx + 3] = a;

            if a == 0 {
                let row = dst_y as usize;
                let byte_index = row * stride + (x as usize / 8);
                let bit = 0x80u8 >> (x as usize % 8);
                and_mask[byte_index] |= bit;
            }
        }
    }

    let hicon: HICON = unsafe {
        CreateIcon(
            ptr::null_mut(),
            width,
            height,
            1,
            32,
            and_mask.as_ptr(),
            xor.as_ptr(),
        )
    };
    if hicon.is_null() {
        None
    } else {
        Some(hicon)
    }
}

#[cfg(test)]
mod tests {
    use super::{copper_server_icon, windows_display_pin_icon};

    #[test]
    fn copper_server_icon_renders_visible_pixels() {
        let icon = copper_server_icon().expect("server icon");
        assert_eq!(icon.width, 24);
        assert_eq!(icon.height, 24);
        assert!(
            icon.rgba.chunks_exact(4).any(|pixel| pixel[3] > 0),
            "icon should contain visible pixels"
        );
    }

    #[test]
    fn pin_variants_render_as_distinct_assets() {
        let pinned_dark = windows_display_pin_icon(true, true).expect("pinned dark");
        let unpinned_dark = windows_display_pin_icon(false, true).expect("unpinned dark");
        let pinned_light = windows_display_pin_icon(true, false).expect("pinned light");

        assert_eq!(pinned_dark.width, 24);
        assert_eq!(pinned_dark.height, 24);
        assert_ne!(pinned_dark.rgba, unpinned_dark.rgba);
        assert_ne!(pinned_dark.rgba, pinned_light.rgba);
    }
}
