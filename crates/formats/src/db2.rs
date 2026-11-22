//! DB2 table reading (the `WDC5` format).
//!
//! The reader decodes every field to `u32` values (arrays give several
//! values). Callers know the meaning of each field from the table's
//! definition. String fields hold an offset; `Table::string` resolves it.
//! Sparse tables (variable-size records) are not supported.

pub struct Table {
    pub records: Vec<Record>,
    record_size: usize,
    total_records: usize,
    fields: Vec<FieldInfo>,
    strings: Vec<u8>,
}

pub struct Record {
    pub id: u32,
    /// Position of the record among all records, used to resolve strings.
    index: usize,
    /// Field values; arrays are flattened.
    pub values: Vec<Vec<u32>>,
}

#[derive(Clone, Debug)]
struct FieldInfo {
    offset_bits: usize,
    size_bits: usize,
    additional_size: usize,
    compression: u32,
    val1: u32,
    #[allow(dead_code)]
    val2: u32,
    val3: u32,
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn u16(&mut self) -> Result<u16, String> {
        let v = self.bytes(2)?;
        Ok(u16::from_le_bytes([v[0], v[1]]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        let v = self.bytes(4)?;
        Ok(u32::from_le_bytes(v.try_into().unwrap()))
    }
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        let v = self
            .data
            .get(self.pos..self.pos + n)
            .ok_or("DB2 data truncated")?;
        self.pos += n;
        Ok(v)
    }
}

/// Read `bits` bits starting at bit `offset` of `data`, little endian.
fn read_bits(data: &[u8], offset: usize, bits: usize) -> u32 {
    let mut value = 0u64;
    let first = offset / 8;
    let shift = offset % 8;
    for i in 0..8 {
        if let Some(b) = data.get(first + i) {
            value |= (*b as u64) << (8 * i);
        }
    }
    let v = value >> shift;
    if bits >= 32 {
        v as u32
    } else {
        (v & ((1u64 << bits) - 1)) as u32
    }
}

struct Section {
    file_offset: usize,
    record_count: usize,
    string_table_size: usize,
    id_list_size: usize,
    relationship_size: usize,
    offset_map_count: usize,
    copy_count: usize,
    tact_key: u64,
}

impl Table {
    pub fn parse(data: &[u8]) -> Result<Table, String> {
        let mut r = Reader { data, pos: 0 };
        let magic = r.bytes(4)?;
        if magic != b"WDC5" {
            return Err(format!(
                "unsupported DB2 format {:?}",
                String::from_utf8_lossy(magic)
            ));
        }
        let _version = r.u32()?;
        r.bytes(128)?; // schema string
        let record_count = r.u32()? as usize;
        let field_count = r.u32()? as usize;
        let record_size = r.u32()? as usize;
        let _string_table_size = r.u32()?;
        let _table_hash = r.u32()?;
        let _layout_hash = r.u32()?;
        let _min_id = r.u32()?;
        let _max_id = r.u32()?;
        let _locale = r.u32()?;
        let flags = r.u16()?;
        let id_index = r.u16()? as usize;
        let _total_field_count = r.u32()?;
        let _bitpacked_offset = r.u32()?;
        let _lookup_columns = r.u32()?;
        let field_info_size = r.u32()? as usize;
        let common_size = r.u32()? as usize;
        let pallet_size = r.u32()? as usize;
        let section_count = r.u32()? as usize;
        if flags & 1 != 0 {
            return Err("sparse DB2 tables are not supported".into());
        }

        let mut sections = Vec::new();
        for _ in 0..section_count {
            let tact_key = u64::from_le_bytes(r.bytes(8)?.try_into().unwrap());
            let file_offset = r.u32()? as usize;
            let record_count = r.u32()? as usize;
            let string_table_size = r.u32()? as usize;
            let _records_end = r.u32()?;
            let id_list_size = r.u32()? as usize;
            let relationship_size = r.u32()? as usize;
            let offset_map_count = r.u32()? as usize;
            let copy_count = r.u32()? as usize;
            sections.push(Section {
                file_offset,
                record_count,
                string_table_size,
                id_list_size,
                relationship_size,
                offset_map_count,
                copy_count,
                tact_key,
            });
        }
        // Field structure (size and offset); the storage info below is enough.
        r.bytes(field_count * 4)?;
        let mut fields = Vec::new();
        for _ in 0..field_info_size / 24 {
            fields.push(FieldInfo {
                offset_bits: r.u16()? as usize,
                size_bits: r.u16()? as usize,
                additional_size: r.u32()? as usize,
                compression: r.u32()?,
                val1: r.u32()?,
                val2: r.u32()?,
                val3: r.u32()?,
            });
        }
        let pallet = r.bytes(pallet_size)?;
        let common = r.bytes(common_size)?;

        // Pallet and common data are stored per field, in field order.
        let mut pallet_start = Vec::new();
        let mut common_maps = Vec::new();
        let (mut p, mut c) = (0usize, 0usize);
        for f in &fields {
            pallet_start.push(p);
            let mut map = std::collections::HashMap::new();
            match f.compression {
                3 | 4 => p += f.additional_size,
                2 => {
                    let block = common.get(c..c + f.additional_size).unwrap_or(&[]);
                    for pair in block.as_chunks::<8>().0 {
                        let id = u32::from_le_bytes(pair[0..4].try_into().unwrap());
                        let v = u32::from_le_bytes(pair[4..8].try_into().unwrap());
                        map.insert(id, v);
                    }
                    c += f.additional_size;
                }
                _ => {}
            }
            common_maps.push(map);
        }

        let mut records = Vec::with_capacity(record_count);
        let mut strings = Vec::new();
        let mut global_index = 0usize;
        for s in &sections {
            if s.tact_key != 0 && s.record_count > 0 && s.file_offset == 0 {
                continue;
            }
            let mut sr = Reader {
                data,
                pos: s.file_offset,
            };
            let record_data = sr.bytes(s.record_count * record_size)?;
            strings.extend_from_slice(sr.bytes(s.string_table_size)?);
            let mut ids = Vec::new();
            for _ in 0..s.id_list_size / 4 {
                ids.push(sr.u32()?);
            }
            let mut copies = Vec::new();
            for _ in 0..s.copy_count {
                copies.push((sr.u32()?, sr.u32()?));
            }
            sr.bytes(s.offset_map_count * 6)?;
            let mut relations = std::collections::HashMap::new();
            if s.relationship_size > 0 {
                let n = sr.u32()? as usize;
                let _min = sr.u32()?;
                let _max = sr.u32()?;
                for _ in 0..n {
                    let foreign = sr.u32()?;
                    let index = sr.u32()? as usize;
                    relations.insert(index, foreign);
                }
            }

            let section_first = records.len();
            for i in 0..s.record_count {
                let rec = &record_data[i * record_size..(i + 1) * record_size];
                let mut values = Vec::with_capacity(fields.len());
                let mut id_from_field = None;
                for (fi, f) in fields.iter().enumerate() {
                    let v = match f.compression {
                        0 => {
                            let count = (f.size_bits / 32).max(1);
                            let width = if f.size_bits >= 32 { 32 } else { f.size_bits };
                            (0..count)
                                .map(|k| read_bits(rec, f.offset_bits + k * 32, width))
                                .collect()
                        }
                        1 | 5 => {
                            let mut v = read_bits(rec, f.offset_bits, f.size_bits);
                            if f.compression == 5 || f.val3 & 1 != 0 {
                                let bits = f.size_bits as u32;
                                if bits < 32 && v & (1 << (bits - 1)) != 0 {
                                    v |= !0u32 << bits;
                                }
                            }
                            vec![v]
                        }
                        2 => vec![0], // filled in once the id is known
                        3 => {
                            let idx = read_bits(rec, f.offset_bits, f.size_bits) as usize;
                            let o = pallet_start[fi] + idx * 4;
                            vec![
                                pallet
                                    .get(o..o + 4)
                                    .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                                    .unwrap_or(0),
                            ]
                        }
                        4 => {
                            let idx = read_bits(rec, f.offset_bits, f.size_bits) as usize;
                            let n = f.val3 as usize;
                            (0..n)
                                .map(|k| {
                                    let o = pallet_start[fi] + (idx * n + k) * 4;
                                    pallet
                                        .get(o..o + 4)
                                        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                                        .unwrap_or(0)
                                })
                                .collect()
                        }
                        _ => vec![0],
                    };
                    if fi == id_index {
                        id_from_field = v.first().copied();
                    }
                    values.push(v);
                }
                let id = ids.get(i).copied().or(id_from_field).unwrap_or(0);
                for (fi, f) in fields.iter().enumerate() {
                    if f.compression == 2 {
                        values[fi] = vec![common_maps[fi].get(&id).copied().unwrap_or(f.val1)];
                    }
                }
                if let Some(foreign) = relations.get(&i) {
                    values.push(vec![*foreign]);
                }
                records.push(Record {
                    id,
                    index: global_index,
                    values,
                });
                global_index += 1;
            }
            for (new_id, old_id) in copies {
                if let Some(src) = records[section_first..].iter().find(|r| r.id == old_id) {
                    let copy = Record {
                        id: new_id,
                        index: src.index,
                        values: src.values.clone(),
                    };
                    records.push(copy);
                }
            }
        }
        Ok(Table {
            records,
            record_size,
            total_records: record_count,
            fields,
            strings,
        })
    }

    /// Resolve an inline string field of a record.
    pub fn string(&self, record: &Record, field: usize) -> String {
        let Some(f) = self.fields.get(field) else {
            return String::new();
        };
        let Some(&value) = record.values.get(field).and_then(|v| v.first()) else {
            return String::new();
        };
        let pos = record.index * self.record_size + f.offset_bits / 8 + value as usize;
        let Some(start) = pos.checked_sub(self.total_records * self.record_size) else {
            return String::new();
        };
        let Some(bytes) = self.strings.get(start..) else {
            return String::new();
        };
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..end]).into_owned()
    }

    pub fn field_count(&self) -> usize {
        self.fields.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_bits_across_bytes() {
        let data = [0b1010_0000, 0b0000_0011];
        assert_eq!(read_bits(&data, 5, 5), 0b11101);
        assert_eq!(read_bits(&[0x78, 0x56, 0x34, 0x12], 0, 32), 0x12345678);
    }
}
