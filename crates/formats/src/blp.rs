//! BLP2 texture decoding to RGBA8.

#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Row-major RGBA, 4 bytes per pixel.
    pub rgba: Vec<u8>,
}

pub fn decode(data: &[u8]) -> Result<Image, String> {
    if data.len() < 148 || &data[0..4] != b"BLP2" {
        return Err("not a BLP2 file".into());
    }
    let compression = data[8];
    let alpha_depth = data[9];
    let alpha_type = data[10];
    let u32_at = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
    let width = u32_at(12);
    let height = u32_at(16);
    let offset = u32_at(20) as usize;
    let size = u32_at(84) as usize;
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err(format!("bad BLP size {width}x{height}"));
    }
    let mip = data
        .get(offset..offset + size)
        .ok_or("BLP mipmap out of range")?;
    let (w, h) = (width as usize, height as usize);
    let rgba = match compression {
        1 => {
            let palette = data.get(148..148 + 1024).ok_or("BLP palette missing")?;
            decode_palette(mip, palette, w, h, alpha_depth)?
        }
        2 => match alpha_type {
            0 => decode_dxt(mip, w, h, Dxt::Dxt1)?,
            1 => decode_dxt(mip, w, h, Dxt::Dxt3)?,
            7 => decode_dxt(mip, w, h, Dxt::Dxt5)?,
            t => return Err(format!("unsupported DXT alpha type {t}")),
        },
        3 => {
            let px = mip.get(..w * h * 4).ok_or("BLP data truncated")?;
            px.as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[2], p[1], p[0], p[3]])
                .collect()
        }
        c => return Err(format!("unsupported BLP compression {c}")),
    };
    Ok(Image {
        width,
        height,
        rgba,
    })
}

fn decode_palette(
    mip: &[u8],
    palette: &[u8],
    w: usize,
    h: usize,
    alpha_depth: u8,
) -> Result<Vec<u8>, String> {
    let n = w * h;
    let indices = mip.get(..n).ok_or("BLP data truncated")?;
    let alpha = &mip[n..];
    let mut out = Vec::with_capacity(n * 4);
    for (i, &idx) in indices.iter().enumerate() {
        let p = &palette[idx as usize * 4..idx as usize * 4 + 4];
        let a = match alpha_depth {
            0 => 255,
            1 => {
                let bit = alpha.get(i / 8).map(|b| (b >> (i % 8)) & 1).unwrap_or(1);
                if bit == 1 { 255 } else { 0 }
            }
            4 => {
                let b = alpha.get(i / 2).copied().unwrap_or(0xff);
                let nib = if i % 2 == 0 { b & 0x0f } else { b >> 4 };
                nib * 17
            }
            _ => alpha.get(i).copied().unwrap_or(255),
        };
        out.extend_from_slice(&[p[2], p[1], p[0], a]);
    }
    Ok(out)
}

#[derive(Clone, Copy, PartialEq)]
enum Dxt {
    Dxt1,
    Dxt3,
    Dxt5,
}

fn rgb565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 0x1f) as u32;
    let g = ((c >> 5) & 0x3f) as u32;
    let b = (c & 0x1f) as u32;
    [
        ((r * 255 + 15) / 31) as u8,
        ((g * 255 + 31) / 63) as u8,
        ((b * 255 + 15) / 31) as u8,
    ]
}

fn decode_dxt(mip: &[u8], w: usize, h: usize, kind: Dxt) -> Result<Vec<u8>, String> {
    let block_size = if kind == Dxt::Dxt1 { 8 } else { 16 };
    let bw = w.div_ceil(4);
    let bh = h.div_ceil(4);
    if mip.len() < bw * bh * block_size {
        return Err("DXT data truncated".into());
    }
    let mut out = vec![0u8; w * h * 4];
    for by in 0..bh {
        for bx in 0..bw {
            let block = &mip[(by * bw + bx) * block_size..][..block_size];
            let (alpha_block, color_block) = if kind == Dxt::Dxt1 {
                (None, block)
            } else {
                (Some(&block[..8]), &block[8..])
            };
            let c0 = u16::from_le_bytes([color_block[0], color_block[1]]);
            let c1 = u16::from_le_bytes([color_block[2], color_block[3]]);
            let (p0, p1) = (rgb565(c0), rgb565(c1));
            let mut colors = [[0u8; 4]; 4];
            colors[0] = [p0[0], p0[1], p0[2], 255];
            colors[1] = [p1[0], p1[1], p1[2], 255];
            let mix = |a: u8, b: u8, wa: u32, wb: u32, d: u32| {
                ((a as u32 * wa + b as u32 * wb) / d) as u8
            };
            if c0 > c1 || kind != Dxt::Dxt1 {
                for i in 0..3 {
                    colors[2][i] = mix(p0[i], p1[i], 2, 1, 3);
                    colors[3][i] = mix(p0[i], p1[i], 1, 2, 3);
                }
                colors[2][3] = 255;
                colors[3][3] = 255;
            } else {
                for i in 0..3 {
                    colors[2][i] = mix(p0[i], p1[i], 1, 1, 2);
                }
                colors[2][3] = 255;
                colors[3] = [0, 0, 0, 0];
            }
            let indices = u32::from_le_bytes(color_block[4..8].try_into().unwrap());

            let mut alphas = [255u8; 16];
            if let Some(ab) = alpha_block {
                if kind == Dxt::Dxt3 {
                    for (i, a) in alphas.iter_mut().enumerate() {
                        let nib = (ab[i / 2] >> ((i % 2) * 4)) & 0x0f;
                        *a = nib * 17;
                    }
                } else {
                    let (a0, a1) = (ab[0] as u32, ab[1] as u32);
                    let mut table = [0u32; 8];
                    table[0] = a0;
                    table[1] = a1;
                    if a0 > a1 {
                        for i in 1..7 {
                            table[i + 1] = ((7 - i as u32) * a0 + i as u32 * a1) / 7;
                        }
                    } else {
                        for i in 1..5 {
                            table[i + 1] = ((5 - i as u32) * a0 + i as u32 * a1) / 5;
                        }
                        table[6] = 0;
                        table[7] = 255;
                    }
                    let mut bits = 0u64;
                    for (i, b) in ab[2..8].iter().enumerate() {
                        bits |= (*b as u64) << (8 * i);
                    }
                    for (i, a) in alphas.iter_mut().enumerate() {
                        *a = table[((bits >> (3 * i)) & 7) as usize] as u8;
                    }
                }
            }

            for py in 0..4 {
                for px in 0..4 {
                    let (x, y) = (bx * 4 + px, by * 4 + py);
                    if x >= w || y >= h {
                        continue;
                    }
                    let i = py * 4 + px;
                    let mut c = colors[((indices >> (2 * i)) & 3) as usize];
                    if kind != Dxt::Dxt1 {
                        c[3] = alphas[i];
                    }
                    out[(y * w + x) * 4..][..4].copy_from_slice(&c);
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(
        compression: u8,
        alpha_depth: u8,
        alpha_type: u8,
        w: u32,
        h: u32,
        mip: &[u8],
    ) -> Vec<u8> {
        let mut d = vec![0u8; 148 + 1024];
        d[0..4].copy_from_slice(b"BLP2");
        d[4..8].copy_from_slice(&1u32.to_le_bytes());
        d[8] = compression;
        d[9] = alpha_depth;
        d[10] = alpha_type;
        d[12..16].copy_from_slice(&w.to_le_bytes());
        d[16..20].copy_from_slice(&h.to_le_bytes());
        d[20..24].copy_from_slice(&(1172u32).to_le_bytes());
        d[84..88].copy_from_slice(&(mip.len() as u32).to_le_bytes());
        d.extend_from_slice(mip);
        d
    }

    #[test]
    fn decodes_bgra() {
        let img = decode(&header(3, 8, 0, 1, 1, &[1, 2, 3, 4])).unwrap();
        assert_eq!(img.rgba, vec![3, 2, 1, 4]);
    }

    #[test]
    fn decodes_dxt1_solid_block() {
        // c0 = c1 = pure red (0xf800); all indices 0.
        let block = [0x00, 0xf8, 0x00, 0xf8, 0, 0, 0, 0];
        let img = decode(&header(2, 0, 0, 4, 4, &block)).unwrap();
        assert_eq!(&img.rgba[0..4], &[255, 0, 0, 255]);
        assert_eq!(img.rgba.len(), 64);
    }

    #[test]
    fn decodes_palette_with_alpha() {
        let mut d = header(1, 8, 0, 1, 1, &[5, 128]);
        d[148 + 20..148 + 24].copy_from_slice(&[10, 20, 30, 0]);
        let img = decode(&d).unwrap();
        assert_eq!(img.rgba, vec![30, 20, 10, 128]);
    }
}
