//! TGA images, which addons ship next to BLP files. The client reads
//! true-color (types 2 and 10) and grayscale (types 3 and 11) images,
//! uncompressed or run-length encoded, at 8, 24 or 32 bits per pixel.

use crate::blp::Image;

pub fn decode(data: &[u8]) -> Result<Image, String> {
    if data.len() < 18 {
        return Err("TGA header truncated".into());
    }
    let id_len = data[0] as usize;
    let color_map_type = data[1];
    let kind = data[2];
    let width = u16::from_le_bytes([data[12], data[13]]) as u32;
    let height = u16::from_le_bytes([data[14], data[15]]) as u32;
    let bpp = data[16] as usize;
    let top_down = data[17] & 0x20 != 0;
    if color_map_type != 0 {
        return Err("color-mapped TGA images are not supported".into());
    }
    let gray = matches!(kind, 3 | 11);
    let rle = matches!(kind, 10 | 11);
    if !matches!(kind, 2 | 3 | 10 | 11) {
        return Err(format!("TGA image type {kind} is not supported"));
    }
    let bytes = bpp / 8;
    if !(gray && bytes == 1) && !(!gray && (bytes == 3 || bytes == 4)) {
        return Err(format!("TGA with {bpp} bits per pixel is not supported"));
    }
    let pixels = (width * height) as usize;
    let mut src = data.get(18 + id_len..).ok_or("TGA data truncated")?;
    // Pixels in file order, as RGBA.
    let mut out = Vec::with_capacity(pixels * 4);
    let pixel = |p: &[u8]| -> [u8; 4] {
        match p.len() {
            1 => [p[0], p[0], p[0], 255],
            3 => [p[2], p[1], p[0], 255],
            _ => [p[2], p[1], p[0], p[3]],
        }
    };
    while out.len() < pixels * 4 {
        if rle {
            let (&header, rest) = src.split_first().ok_or("TGA data truncated")?;
            let count = (header & 0x7f) as usize + 1;
            if header & 0x80 != 0 {
                let p = rest.get(..bytes).ok_or("TGA data truncated")?;
                let px = pixel(p);
                for _ in 0..count {
                    out.extend_from_slice(&px);
                }
                src = &rest[bytes..];
            } else {
                let run = rest.get(..count * bytes).ok_or("TGA data truncated")?;
                for p in run.chunks(bytes) {
                    out.extend_from_slice(&pixel(p));
                }
                src = &rest[count * bytes..];
            }
        } else {
            let run = src.get(..pixels * bytes).ok_or("TGA data truncated")?;
            for p in run.chunks(bytes) {
                out.extend_from_slice(&pixel(p));
            }
            break;
        }
    }
    out.truncate(pixels * 4);
    if !top_down {
        // Bottom-up files store the last row first.
        let row = width as usize * 4;
        let mut flipped = Vec::with_capacity(out.len());
        for r in out.chunks(row).rev() {
            flipped.extend_from_slice(r);
        }
        out = flipped;
    }
    Ok(Image {
        width,
        height,
        rgba: out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(kind: u8, w: u16, h: u16, bpp: u8, desc: u8) -> Vec<u8> {
        let mut d = vec![0u8; 18];
        d[2] = kind;
        d[12..14].copy_from_slice(&w.to_le_bytes());
        d[14..16].copy_from_slice(&h.to_le_bytes());
        d[16] = bpp;
        d[17] = desc;
        d
    }

    #[test]
    fn reads_bottom_up_uncompressed_pixels() {
        let mut d = header(2, 1, 2, 32, 0x08);
        // Bottom row first: blue, then the top row red (BGRA in the file).
        d.extend_from_slice(&[255, 0, 0, 255, 0, 0, 255, 128]);
        let img = decode(&d).unwrap();
        assert_eq!(img.rgba, vec![255, 0, 0, 128, 0, 0, 255, 255]);
    }

    #[test]
    fn reads_run_length_encoded_pixels() {
        let mut d = header(10, 3, 1, 24, 0x20);
        // A run of two green pixels, then one raw white pixel.
        d.extend_from_slice(&[0x81, 0, 255, 0, 0x00, 255, 255, 255]);
        let img = decode(&d).unwrap();
        assert_eq!(
            img.rgba,
            vec![0, 255, 0, 255, 0, 255, 0, 255, 255, 255, 255, 255]
        );
    }
}
