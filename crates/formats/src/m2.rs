//! Reader for M2 models (creatures, characters, doodads) and their skin
//! files (the triangle lists), as stored by the Retail client.
//!
//! A Retail M2 file is a list of chunks. `MD21` holds the classic `MD20`
//! data; its offsets are relative to the start of that data. `SFID` lists
//! the FileDataIDs of the skin files, `TXID` those of the textures. Only
//! what a static draw needs is read: vertices, textures, materials, the
//! texture lookup table and the first skin's sections and batches.
//! Of the animations, only the first frame of Stand is read
//! ([`stand_pose`]), to pose models instead of drawing the bind pose.

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    /// Up to four bones that move the vertex, with weights out of 255.
    pub bone_weights: [u8; 4],
    pub bone_indices: [u8; 4],
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
    /// FileDataID of the skeleton file (SKID chunk), for character models
    /// whose bones and animations are kept separately. 0 if none.
    pub skeleton_file: u32,
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
    let mut skeleton_file = 0;
    if data.starts_with(b"MD20") {
        md20 = Some(data);
    } else {
        for (tag, body) in chunks(data) {
            match &tag {
                b"MD21" => md20 = Some(body),
                b"SFID" => skin_files = u32_list(body),
                b"TXID" => texture_files = u32_list(body),
                b"SKID" => skeleton_file = u32_list(body).first().copied().unwrap_or(0),
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
            bone_weights: [
                r.u8(at + 12)?,
                r.u8(at + 13)?,
                r.u8(at + 14)?,
                r.u8(at + 15)?,
            ],
            bone_indices: [
                r.u8(at + 16)?,
                r.u8(at + 17)?,
                r.u8(at + 18)?,
                r.u8(at + 19)?,
            ],
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
        skeleton_file,
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

/// A 3x4 affine matrix, row-major: rows are x, y, z of the output.
pub type Matrix = [[f32; 4]; 3];

pub const IDENTITY: Matrix = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

fn mul(a: &Matrix, b: &Matrix) -> Matrix {
    let mut out = [[0.0; 4]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
            if j == 3 {
                *cell += a[i][3];
            }
        }
    }
    out
}

/// Apply a matrix to a point (`w` 1) or a direction (`w` 0).
pub fn transform(m: &Matrix, v: [f32; 3], w: f32) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2] + m[0][3] * w,
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2] + m[1][3] * w,
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2] + m[2][3] * w,
    ]
}

fn compressed(v: i16) -> f32 {
    let v = v as f32;
    if v < 0.0 {
        (v + 32768.0) / 32767.0
    } else {
        (v - 32767.0) / 32767.0
    }
}

/// The first key of a bone track for one sequence: the offset of its value,
/// or `None` when the track has no keys for that sequence.
fn first_key(r: &Reader, track: usize, seq: usize) -> Result<Option<usize>, String> {
    let global = r.u16(track + 2)? as i16;
    let (count, offset) = r.array(track + 12)?;
    let index = if global >= 0 { 0 } else { seq };
    if index >= count {
        return Ok(None);
    }
    let (n, at) = r.array(offset + index * 8)?;
    Ok((n > 0).then_some(at))
}

/// Bone matrices for the first frame of the Stand animation, or `None`
/// when the model has no bones or Stand is not stored with them. `skeleton`
/// is the model's skeleton file, if it has one. Vertex bone indices point
/// into the returned list.
pub fn stand_pose(model: &[u8], skeleton: Option<&[u8]>) -> Option<Vec<Matrix>> {
    // Where the bones and sequences are: in the skeleton's SKB1 and SKS1
    // chunks (offsets relative to each chunk), or in the model's MD20.
    let (bones_data, bones_at, seqs_data, seqs_at) = match skeleton {
        Some(skel) => {
            let found = chunks(skel);
            let get = |t: &[u8; 4]| found.iter().find(|(tag, _)| tag == t).map(|(_, b)| *b);
            (get(b"SKB1")?, 0, get(b"SKS1")?, 8)
        }
        None => {
            let md20 = if model.starts_with(b"MD20") {
                model
            } else {
                chunks(model)
                    .into_iter()
                    .find(|(t, _)| t == b"MD21")
                    .map(|(_, b)| b)?
            };
            (md20, 44, md20, 28)
        }
    };
    let rs = Reader { data: seqs_data };
    let (nseq, oseq) = rs.array(seqs_at).ok()?;
    let seq = (0..nseq).find(|&i| {
        rs.u16(oseq + i * 64).ok() == Some(0) && rs.u16(oseq + i * 64 + 2).ok() == Some(0)
    })?;
    // Flag 0x20: the keys are stored with the bones, not in an .anim file.
    if rs.u32(oseq + seq * 64 + 12).ok()? & 0x20 == 0 {
        return None;
    }
    let r = Reader { data: bones_data };
    let (nb, ob) = r.array(bones_at).ok()?;
    if nb == 0 {
        return None;
    }
    let mut out: Vec<Matrix> = Vec::with_capacity(nb);
    for b in 0..nb {
        let at = ob + b * 88;
        let parent = r.u16(at + 8).ok()? as i16;
        let pivot = [
            r.f32(at + 76).ok()?,
            r.f32(at + 80).ok()?,
            r.f32(at + 84).ok()?,
        ];
        let vec3 = |off: Option<usize>, default: [f32; 3]| -> [f32; 3] {
            match off {
                Some(o) => [
                    r.f32(o).unwrap_or(default[0]),
                    r.f32(o + 4).unwrap_or(default[1]),
                    r.f32(o + 8).unwrap_or(default[2]),
                ],
                None => default,
            }
        };
        let t = vec3(first_key(&r, at + 16, seq).ok()?, [0.0; 3]);
        let sc = vec3(first_key(&r, at + 56, seq).ok()?, [1.0; 3]);
        let q = match first_key(&r, at + 36, seq).ok()? {
            Some(o) => [
                compressed(r.u16(o).ok()? as i16),
                compressed(r.u16(o + 2).ok()? as i16),
                compressed(r.u16(o + 4).ok()? as i16),
                compressed(r.u16(o + 6).ok()? as i16),
            ],
            None => [0.0, 0.0, 0.0, 1.0],
        };
        let [x, y, z, w] = q;
        let rot = [
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y - z * w),
                2.0 * (x * z + y * w),
            ],
            [
                2.0 * (x * y + z * w),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z - x * w),
            ],
            [
                2.0 * (x * z - y * w),
                2.0 * (y * z + x * w),
                1.0 - 2.0 * (x * x + y * y),
            ],
        ];
        // local = T(pivot + t) * R * S * T(-pivot)
        let mut local = [[0.0; 4]; 3];
        for i in 0..3 {
            for j in 0..3 {
                local[i][j] = rot[i][j] * sc[j];
            }
            let rp = local[i][0] * pivot[0] + local[i][1] * pivot[1] + local[i][2] * pivot[2];
            local[i][3] = pivot[i] + t[i] - rp;
        }
        let m = if parent >= 0 && (parent as usize) < out.len() {
            mul(&out[parent as usize], &local)
        } else {
            local
        };
        out.push(m);
    }
    Some(out)
}
