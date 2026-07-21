use std::fs;
use std::io;
use std::path::Path;

use x11rb::connection::Connection as _;
use x11rb::protocol::composite::ConnectionExt as _;
use x11rb::protocol::xproto::{self, ConnectionExt as _, Format, ImageOrder, Visualtype};
use x11rb::rust_connection::RustConnection;

use crate::Result;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screenshot {
    pub width: u16,
    pub height: u16,
    pub rgb_pixels: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenshotDiff {
    pub width: u16,
    pub height: u16,
    pub total_pixels: u64,
    pub changed_pixels: u64,
    pub first_diff: Option<(u16, u16)>,
    pub changed_bounds: Option<(u16, u16, u16, u16)>,
    pub grid_changed_pixels: [u64; 9],
}

impl Screenshot {
    pub fn read_ppm(path: impl AsRef<Path>) -> Result<Self> {
        let bytes = fs::read(path)?;
        parse_ppm(&bytes)
    }

    pub fn write_ppm(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut body = format!("P6\n{} {}\n255\n", self.width, self.height).into_bytes();
        body.extend_from_slice(&self.rgb_pixels);
        fs::write(path, body)?;
        Ok(())
    }

    /// Normalize only the four compositor-owned corner squares. Unlike an
    /// inset, this preserves every non-corner pixel along the application
    /// edges so visual tests continue to cover shell and separator borders.
    pub fn mask_corner_squares(&self, pixels: u16) -> Result<Self> {
        let doubled = pixels
            .checked_mul(2)
            .ok_or_else(|| io::Error::other("screenshot corner mask overflow"))?;
        if doubled > self.width || doubled > self.height {
            return Err(io::Error::other("screenshot corner mask overlaps itself").into());
        }

        let mut masked = self.clone();
        for y in 0..self.height {
            let corner_row = y < pixels || y >= self.height - pixels;
            if !corner_row {
                continue;
            }
            for x in 0..self.width {
                if x >= pixels && x < self.width - pixels {
                    continue;
                }
                let start = (usize::from(y) * usize::from(self.width) + usize::from(x)) * 3;
                masked.rgb_pixels[start..start + 3].fill(0);
            }
        }
        Ok(masked)
    }

    pub fn diff(&self, expected: &Self) -> Result<ScreenshotDiff> {
        if self.width != expected.width || self.height != expected.height {
            return Err(io::Error::other(format!(
                "screenshot dimensions differ: actual {}x{}, expected {}x{}",
                self.width, self.height, expected.width, expected.height
            ))
            .into());
        }
        if self.rgb_pixels.len() != expected.rgb_pixels.len() {
            return Err(io::Error::other(format!(
                "screenshot byte lengths differ: actual {}, expected {}",
                self.rgb_pixels.len(),
                expected.rgb_pixels.len()
            ))
            .into());
        }

        let mut changed_pixels = 0u64;
        let mut first_diff = None;
        let mut min_x = self.width;
        let mut min_y = self.height;
        let mut max_x = 0u16;
        let mut max_y = 0u16;
        let mut grid_changed_pixels = [0u64; 9];

        for (index, (actual, expected)) in self
            .rgb_pixels
            .chunks_exact(3)
            .zip(expected.rgb_pixels.chunks_exact(3))
            .enumerate()
        {
            if actual == expected {
                continue;
            }
            let x = (index % self.width as usize) as u16;
            let y = (index / self.width as usize) as u16;
            changed_pixels += 1;
            first_diff.get_or_insert((x, y));
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            let col = ((x as usize) * 3 / self.width as usize).min(2);
            let row = ((y as usize) * 3 / self.height as usize).min(2);
            grid_changed_pixels[row * 3 + col] += 1;
        }

        Ok(ScreenshotDiff {
            width: self.width,
            height: self.height,
            total_pixels: self.width as u64 * self.height as u64,
            changed_pixels,
            first_diff,
            changed_bounds: first_diff.map(|_| (min_x, min_y, max_x, max_y)),
            grid_changed_pixels,
        })
    }

    pub fn write_diff_ppm(&self, expected: &Self, path: impl AsRef<Path>) -> Result<()> {
        let diff = self.diff(expected)?;
        let mut pixels = vec![0; self.rgb_pixels.len()];
        if diff.changed_pixels > 0 {
            for (out, (actual, expected)) in pixels
                .chunks_exact_mut(3)
                .zip(self.rgb_pixels.chunks_exact(3).zip(expected.rgb_pixels.chunks_exact(3)))
            {
                if actual == expected {
                    out.copy_from_slice(&[0, 0, 0]);
                } else {
                    out.copy_from_slice(&[255, 0, 0]);
                }
            }
        }
        Screenshot {
            width: self.width,
            height: self.height,
            rgb_pixels: pixels,
        }
        .write_ppm(path)
    }
}

impl ScreenshotDiff {
    pub fn is_exact(&self) -> bool {
        self.changed_pixels == 0
    }

    pub fn changed_percent(&self) -> f64 {
        if self.total_pixels == 0 {
            0.0
        } else {
            self.changed_pixels as f64 * 100.0 / self.total_pixels as f64
        }
    }

    pub fn grid_percentages(&self) -> [f64; 9] {
        let cell_pixels = |index: usize| -> u64 {
            let row = index / 3;
            let col = index % 3;
            let x0 = div_ceil(col as u64 * self.width as u64, 3);
            let x1 = div_ceil((col as u64 + 1) * self.width as u64, 3);
            let y0 = div_ceil(row as u64 * self.height as u64, 3);
            let y1 = div_ceil((row as u64 + 1) * self.height as u64, 3);
            (x1 - x0) * (y1 - y0)
        };
        std::array::from_fn(|index| {
            let total = cell_pixels(index);
            if total == 0 {
                0.0
            } else {
                self.grid_changed_pixels[index] as f64 * 100.0 / total as f64
            }
        })
    }
}

fn div_ceil(value: u64, divisor: u64) -> u64 {
    value.div_ceil(divisor)
}

pub(crate) fn capture_window(
    conn: &RustConnection,
    root: xproto::Window,
    window: xproto::Window,
) -> Result<Screenshot> {
    let geometry = conn.get_geometry(window)?.reply()?;
    let overlay = conn.composite_get_overlay_window(root)?.reply()?.overlay_win;
    let overlay_geometry = conn.get_geometry(overlay)?.reply()?;
    let origin = conn.translate_coordinates(window, overlay, 0, 0)?.reply()?;
    let capture_right = i32::from(origin.dst_x) + i32::from(geometry.width);
    let capture_bottom = i32::from(origin.dst_y) + i32::from(geometry.height);
    if origin.dst_x < 0
        || origin.dst_y < 0
        || capture_right > i32::from(overlay_geometry.width)
        || capture_bottom > i32::from(overlay_geometry.height)
    {
        conn.composite_release_overlay_window(root)?.check()?;
        return Err(io::Error::other(format!(
            "editor capture rectangle ({}, {}) {}x{} falls outside compositor overlay {}x{}",
            origin.dst_x,
            origin.dst_y,
            geometry.width,
            geometry.height,
            overlay_geometry.width,
            overlay_geometry.height,
        ))
        .into());
    }
    let reply = conn
        .get_image(
            xproto::ImageFormat::Z_PIXMAP,
            overlay,
            origin.dst_x,
            origin.dst_y,
            geometry.width,
            geometry.height,
            u32::MAX,
        )?
        .reply();
    let released = conn.composite_release_overlay_window(root)?.check();
    let reply = reply.map_err(|error| {
        io::Error::other(format!(
            "could not capture editor at ({}, {}) {}x{} from compositor overlay {}x{}: {error}",
            origin.dst_x,
            origin.dst_y,
            geometry.width,
            geometry.height,
            overlay_geometry.width,
            overlay_geometry.height,
        ))
    })?;
    released?;

    let setup = conn.setup();
    let format = setup
        .pixmap_formats
        .iter()
        .find(|format| format.depth == reply.depth)
        .ok_or_else(|| io::Error::other(format!("no X11 pixmap format for depth {}", reply.depth)))?;
    let visual = setup
        .roots
        .iter()
        .flat_map(|screen| &screen.allowed_depths)
        .flat_map(|depth| &depth.visuals)
        .find(|visual| visual.visual_id == reply.visual)
        .ok_or_else(|| io::Error::other(format!("no X11 visual for id {}", reply.visual)))?;

    decode_zpixmap(
        geometry.width,
        geometry.height,
        &reply.data,
        *format,
        visual,
        setup.image_byte_order,
    )
}

fn decode_zpixmap(
    width: u16,
    height: u16,
    data: &[u8],
    format: Format,
    visual: &Visualtype,
    byte_order: ImageOrder,
) -> Result<Screenshot> {
    let bits_per_pixel = usize::from(format.bits_per_pixel);
    let bytes_per_pixel = bits_per_pixel.div_ceil(8);
    if !(2..=4).contains(&bytes_per_pixel) {
        return Err(io::Error::other(format!(
            "unsupported X11 screenshot format: {} bits per pixel",
            format.bits_per_pixel
        ))
        .into());
    }
    let stride = align_to(usize::from(width) * bits_per_pixel, usize::from(format.scanline_pad)) / 8;
    let required = stride * usize::from(height);
    if data.len() < required {
        return Err(io::Error::other(format!(
            "X11 screenshot data is too short: got {}, need {required}",
            data.len()
        ))
        .into());
    }

    let mut rgb_pixels = Vec::with_capacity(usize::from(width) * usize::from(height) * 3);
    for y in 0..usize::from(height) {
        let row = y * stride;
        for x in 0..usize::from(width) {
            let offset = row + x * bytes_per_pixel;
            let pixel = read_pixel(&data[offset..offset + bytes_per_pixel], byte_order);
            rgb_pixels.push(channel(pixel, visual.red_mask));
            rgb_pixels.push(channel(pixel, visual.green_mask));
            rgb_pixels.push(channel(pixel, visual.blue_mask));
        }
    }
    Ok(Screenshot {
        width,
        height,
        rgb_pixels,
    })
}

fn align_to(value: usize, alignment: usize) -> usize {
    if alignment == 0 {
        value
    } else {
        value.div_ceil(alignment) * alignment
    }
}

fn read_pixel(bytes: &[u8], byte_order: ImageOrder) -> u32 {
    match byte_order {
        ImageOrder::LSB_FIRST => bytes
            .iter()
            .enumerate()
            .fold(0u32, |pixel, (index, byte)| pixel | ((*byte as u32) << (index * 8))),
        _ => bytes.iter().fold(0u32, |pixel, byte| (pixel << 8) | *byte as u32),
    }
}

fn channel(pixel: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let bits = mask.count_ones();
    let raw = (pixel & mask) >> shift;
    let max = (1u32 << bits) - 1;
    ((raw * 255 + max / 2) / max) as u8
}

fn parse_ppm(bytes: &[u8]) -> Result<Screenshot> {
    let mut cursor = 0usize;
    if next_ppm_token(bytes, &mut cursor) != Some(&b"P6"[..]) {
        return Err(io::Error::other("unsupported PPM header; expected P6").into());
    }
    let width = parse_ppm_u16(bytes, &mut cursor, "width")?;
    let height = parse_ppm_u16(bytes, &mut cursor, "height")?;
    let max = parse_ppm_u16(bytes, &mut cursor, "max channel")?;
    if max != 255 {
        return Err(io::Error::other(format!("unsupported PPM max channel {max}; expected 255")).into());
    }
    if cursor >= bytes.len() || !bytes[cursor].is_ascii_whitespace() {
        return Err(io::Error::other("PPM header must be followed by pixel data").into());
    }
    cursor += 1;
    let expected = usize::from(width) * usize::from(height) * 3;
    let data = bytes
        .get(cursor..cursor + expected)
        .ok_or_else(|| io::Error::other(format!("PPM data is too short: need {expected} bytes")))?;
    Ok(Screenshot {
        width,
        height,
        rgb_pixels: data.to_vec(),
    })
}

fn parse_ppm_u16(bytes: &[u8], cursor: &mut usize, label: &str) -> Result<u16> {
    let token = next_ppm_token(bytes, cursor).ok_or_else(|| io::Error::other(format!("missing PPM {label}")))?;
    let text = std::str::from_utf8(token)?;
    Ok(text.parse()?)
}

fn next_ppm_token<'a>(bytes: &'a [u8], cursor: &mut usize) -> Option<&'a [u8]> {
    loop {
        while *cursor < bytes.len() && bytes[*cursor].is_ascii_whitespace() {
            *cursor += 1;
        }
        if *cursor >= bytes.len() || bytes[*cursor] != b'#' {
            break;
        }
        while *cursor < bytes.len() && bytes[*cursor] != b'\n' {
            *cursor += 1;
        }
    }
    let start = *cursor;
    while *cursor < bytes.len() && !bytes[*cursor].is_ascii_whitespace() {
        *cursor += 1;
    }
    (start < *cursor).then_some(&bytes[start..*cursor])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ppm_round_trips_and_diff_summarizes_changed_pixels() {
        let image = Screenshot {
            width: 3,
            height: 3,
            rgb_pixels: vec![
                0, 0, 0, 10, 0, 0, 20, 0, 0, 0, 10, 0, 10, 10, 0, 20, 10, 0, 0, 20, 0, 10, 20, 0, 20, 20, 0,
            ],
        };
        let path = std::env::temp_dir().join(format!("lst-screenshot-{}.ppm", std::process::id()));
        image.write_ppm(&path).unwrap();
        let round_trip = Screenshot::read_ppm(&path).unwrap();
        let _ = fs::remove_file(path);
        assert_eq!(round_trip, image);

        let mut changed = image.clone();
        changed.rgb_pixels[12] = 255;
        let diff = changed.diff(&image).unwrap();
        assert_eq!(diff.changed_pixels, 1);
        assert_eq!(diff.first_diff, Some((1, 1)));
        assert_eq!(diff.changed_bounds, Some((1, 1, 1, 1)));
        assert_eq!(diff.grid_changed_pixels[4], 1);
    }

    #[test]
    fn corner_mask_preserves_non_corner_edges() {
        let image = Screenshot {
            width: 4,
            height: 3,
            rgb_pixels: (0..12).flat_map(|value| [value, value, value]).collect(),
        };

        let masked = image.mask_corner_squares(1).unwrap();

        assert_eq!(masked.width, 4);
        assert_eq!(masked.height, 3);
        assert_eq!(
            masked.rgb_pixels,
            vec![
                0, 0, 0, 1, 1, 1, 2, 2, 2, 0, 0, 0, 4, 4, 4, 5, 5, 5, 6, 6, 6, 7, 7, 7, 0, 0, 0, 9, 9, 9, 10, 10, 10,
                0, 0, 0,
            ]
        );
    }
}
