//! BLTE decoding: a sequence of chunks, each raw (`N`) or zlib (`Z`).
//! Encrypted (`E`) chunks are replaced with zeros, as the client does when
//! it lacks the key.

use std::io::Read;

use crate::{Result, format_err};

pub fn decode(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < 8 || &data[0..4] != b"BLTE" {
        return Err(format_err("missing BLTE magic"));
    }
    let header_size = u32::from_be_bytes(data[4..8].try_into().unwrap()) as usize;
    let mut out = Vec::new();
    if header_size == 0 {
        decode_chunk(&data[8..], None, &mut out)?;
        return Ok(out);
    }
    if data.len() < 12 || header_size > data.len() {
        return Err(format_err("BLTE header truncated"));
    }
    let count = u32::from_be_bytes([0, data[9], data[10], data[11]]) as usize;
    let mut chunks = Vec::with_capacity(count);
    for i in 0..count {
        let o = 12 + i * 24;
        let e = data
            .get(o..o + 8)
            .ok_or_else(|| format_err("BLTE chunk table truncated"))?;
        let compressed = u32::from_be_bytes(e[0..4].try_into().unwrap()) as usize;
        let decompressed = u32::from_be_bytes(e[4..8].try_into().unwrap()) as usize;
        chunks.push((compressed, decompressed));
    }
    let mut o = header_size;
    for (compressed, decompressed) in chunks {
        let chunk = data
            .get(o..o + compressed)
            .ok_or_else(|| format_err("BLTE chunk truncated"))?;
        decode_chunk(chunk, Some(decompressed), &mut out)?;
        o += compressed;
    }
    Ok(out)
}

fn decode_chunk(chunk: &[u8], size: Option<usize>, out: &mut Vec<u8>) -> Result<()> {
    let Some((&mode, body)) = chunk.split_first() else {
        return Ok(());
    };
    match mode {
        b'N' => out.extend_from_slice(body),
        b'Z' => {
            let mut z = flate2::read::ZlibDecoder::new(body);
            z.read_to_end(out)
                .map_err(|e| format_err(format!("zlib: {e}")))?;
        }
        b'F' => out.extend(decode(body)?),
        b'E' => out.resize(out.len() + size.unwrap_or(0), 0),
        other => {
            return Err(format_err(format!(
                "unsupported BLTE mode {}",
                other as char
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn decodes_single_raw_chunk() {
        let mut data = b"BLTE\0\0\0\0N".to_vec();
        data.extend_from_slice(b"hello");
        assert_eq!(decode(&data).unwrap(), b"hello");
    }

    #[test]
    fn decodes_chunk_table_with_zlib() {
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(b"world").unwrap();
        let zchunk = [vec![b'Z'], z.finish().unwrap()].concat();
        let rchunk = b"Nhi ".to_vec();
        let header_size = 12 + 2 * 24;
        let mut data = b"BLTE".to_vec();
        data.extend_from_slice(&(header_size as u32).to_be_bytes());
        data.extend_from_slice(&[0x0f, 0, 0, 2]);
        for (c, d) in [(rchunk.len(), 3), (zchunk.len(), 5)] {
            data.extend_from_slice(&(c as u32).to_be_bytes());
            data.extend_from_slice(&(d as u32).to_be_bytes());
            data.extend_from_slice(&[0; 16]);
        }
        data.extend(rchunk);
        data.extend(zchunk);
        assert_eq!(decode(&data).unwrap(), b"hi world");
    }
}
