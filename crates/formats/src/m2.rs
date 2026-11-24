//! Reader for M2 models (creatures, characters, doodads) and their skin
//! files (the triangle lists), as stored by the Retail client.
//!
//! A Retail M2 file is a list of chunks. `MD21` holds the classic `MD20`
//! data; its offsets are relative to the start of that data. `SFID` lists
//! the FileDataIDs of the skin files, `TXID` those of the textures. Only
//! what a static draw needs is read: vertices, textures, materials, the
//! texture lookup table and the first skin's sections and batches.
//! Animation is not read; models are drawn in their bind pose.

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Texture {
    /// 0 for a file named by the model (`file`); other values are
    /// replaceable textures (1 skin, 6 hair, 11..13 creature skins, ...).
    pub kind: u32,
    /// FileDataID for kind 0, from the TXID chunk.
    pub file: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Material {
    pub flags: u16,
    /// 0 opaque, 1 alpha key, 2 alpha blend, 3+ additive and others.
    pub blend: u16,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Model {
    pub name: String,
    pub vertices: Vec<Vertex>,
    pub textures: Vec<Texture>,
    pub materials: Vec<Material>,
    /// Texture lookup: batch texture combo index -> texture index.
    pub texture_combos: Vec<u16>,
    /// FileDataIDs of the skin files (level of detail 0 first).
    pub skin_files: Vec<u32>,
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Section {
    /// Geoset ID: for characters, group * 100 + variant.
    pub id: u16,
    pub index_start: u32,
    pub index_count: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Batch {
    pub section: u16,
    pub material: u16,
    pub texture_combo: u16,
    pub texture_count: u16,
    pub priority: i8,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Skin {
    /// Triangle corners as indices into the model's vertices.
    pub indices: Vec<u32>,
    pub sections: Vec<Section>,
    pub batches: Vec<Batch>,
}

struct Reader<'a> {
    data: &'a [u8],
}

impl Reader<'_> {
    fn u8(&self, at: usize) -> Result<u8, String> {
        self.data
            .get(at)
            .copied()
            .ok_or_else(|| "truncated".to_string())
    }
    fn u16(&self, at: usize) -> Result<u16, String> {
        let b = self.data.get(at..at + 2).ok_or("truncated")?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&self, at: usize) -> Result<u32, String> {
        let b = self.data.get(at..at + 4).ok_or("truncated")?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn f32(&self, at: usize) -> Result<f32, String> {
        Ok(f32::from_bits(self.u32(at)?))
    }
    /// An M2Array: (count, offset).
    fn array(&self, at: usize) -> Result<(usize, usize), String> {
        Ok((self.u32(at)? as usize, self.u32(at + 4)? as usize))
    }
}

/// Split a chunked file into (tag, body) pairs.
fn chunks(data: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 8 <= data.len() {
        let tag = [data[at], data[at + 1], data[at + 2], data[at + 3]];
        let size =
            u32::from_le_bytes([data[at + 4], data[at + 5], data[at + 6], data[at + 7]]) as usize;
        let end = (at + 8 + size).min(data.len());
        out.push((tag, &data[at + 8..end]));
        at = end;
    }
    out
}

fn u32_list(body: &[u8]) -> Vec<u32> {
    body.as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect()
}

pub fn parse(data: &[u8]) -> Result<Model, String> {
    let mut md20: Option<&[u8]> = None;
    let mut skin_files = Vec::new();
    let mut texture_files = Vec::new();
    if data.starts_with(b"MD20") {
        md20 = Some(data);
    } else {
        for (tag, body) in chunks(data) {
            match &tag {
                b"MD21" => md20 = Some(body),
                b"SFID" => skin_files = u32_list(body),
                b"TXID" => texture_files = u32_list(body),
                _ => {}
            }
        }
    }
    let md20 = md20.ok_or("no MD21 chunk")?;
    if !md20.starts_with(b"MD20") {
        return Err("not an M2 model".into());
    }
    let r = Reader { data: md20 };
    let (name_len, name_ofs) = r.array(8)?;
    let name = md20
        .get(name_ofs..name_ofs + name_len)
        .map(|b| {
            String::from_utf8_lossy(b)
                .trim_end_matches('\0')
                .to_string()
        })
        .unwrap_or_default();

    let (nv, ov) = r.array(60)?;
    let mut vertices = Vec::with_capacity(nv);
    for i in 0..nv {
        let at = ov + i * 48;
        vertices.push(Vertex {
            pos: [r.f32(at)?, r.f32(at + 4)?, r.f32(at + 8)?],
            normal: [r.f32(at + 20)?, r.f32(at + 24)?, r.f32(at + 28)?],
            uv: [r.f32(at + 32)?, r.f32(at + 36)?],
        });
    }
    let num_skins = r.u32(68)? as usize;
    skin_files.truncate(num_skins.max(1));

    let (nt, ot) = r.array(80)?;
    let mut textures = Vec::with_capacity(nt);
    for i in 0..nt {
        let kind = r.u32(ot + i * 16)?;
        textures.push(Texture {
            kind,
            file: if kind == 0 {
                texture_files.get(i).copied().unwrap_or(0)
            } else {
                0
            },
        });
    }
    let (nm, om) = r.array(112)?;
    let mut materials = Vec::with_capacity(nm);
    for i in 0..nm {
        materials.push(Material {
            flags: r.u16(om + i * 4)?,
            blend: r.u16(om + i * 4 + 2)?,
        });
    }
    let (nc, oc) = r.array(128)?;
    let mut texture_combos = Vec::with_capacity(nc);
    for i in 0..nc {
        texture_combos.push(r.u16(oc + i * 2)?);
    }
    let b = |i: usize| r.f32(160 + i * 4);
    Ok(Model {
        name,
        vertices,
        textures,
        materials,
        texture_combos,
        skin_files,
        bounds_min: [b(0)?, b(1)?, b(2)?],
        bounds_max: [b(3)?, b(4)?, b(5)?],
    })
}

pub fn parse_skin(data: &[u8]) -> Result<Skin, String> {
    if !data.starts_with(b"SKIN") {
        return Err("not a skin file".into());
    }
    let r = Reader { data };
    let (nlv, olv) = r.array(4)?;
    let mut local = Vec::with_capacity(nlv);
    for i in 0..nlv {
        local.push(r.u16(olv + i * 2)? as u32);
    }
    let (ni, oi) = r.array(12)?;
    let mut indices = Vec::with_capacity(ni);
    for i in 0..ni {
        let li = r.u16(oi + i * 2)? as usize;
        indices.push(*local.get(li).ok_or("bad skin index")?);
    }
    let (ns, os) = r.array(28)?;
    let mut sections = Vec::with_capacity(ns);
    for i in 0..ns {
        let at = os + i * 48;
        let level = r.u16(at + 2)? as u32;
        sections.push(Section {
            id: r.u16(at)?,
            index_start: r.u16(at + 8)? as u32 + (level << 16),
            index_count: r.u16(at + 10)? as u32,
        });
    }
    let (nb, ob) = r.array(36)?;
    let mut batches = Vec::with_capacity(nb);
    for i in 0..nb {
        let at = ob + i * 24;
        batches.push(Batch {
            priority: r.u8(at + 1)? as i8,
            section: r.u16(at + 4)?,
            material: r.u16(at + 10)?,
            texture_count: r.u16(at + 14)?,
            texture_combo: r.u16(at + 16)?,
        });
    }
    Ok(Skin {
        indices,
        sections,
        batches,
    })
}
