//! Bob Jenkins' `hashlittle2`, used by CASC for file path hashes.

fn rot(x: u32, k: u32) -> u32 {
    x.rotate_left(k)
}

/// Returns `(pc, pb)` as `hashlittle2` does.
pub fn hashlittle2(key: &[u8], mut pc: u32, mut pb: u32) -> (u32, u32) {
    let init = 0xdeadbeefu32
        .wrapping_add(key.len() as u32)
        .wrapping_add(pc);
    let (mut a, mut b, mut c) = (init, init, init.wrapping_add(pb));

    let mut k = key;
    while k.len() > 12 {
        let w = |i: usize| u32::from_le_bytes([k[i], k[i + 1], k[i + 2], k[i + 3]]);
        a = a.wrapping_add(w(0));
        b = b.wrapping_add(w(4));
        c = c.wrapping_add(w(8));
        // mix
        a = a.wrapping_sub(c);
        a ^= rot(c, 4);
        c = c.wrapping_add(b);
        b = b.wrapping_sub(a);
        b ^= rot(a, 6);
        a = a.wrapping_add(c);
        c = c.wrapping_sub(b);
        c ^= rot(b, 8);
        b = b.wrapping_add(a);
        a = a.wrapping_sub(c);
        a ^= rot(c, 16);
        c = c.wrapping_add(b);
        b = b.wrapping_sub(a);
        b ^= rot(a, 19);
        a = a.wrapping_add(c);
        c = c.wrapping_sub(b);
        c ^= rot(b, 4);
        b = b.wrapping_add(a);
        k = &k[12..];
    }
    if k.is_empty() {
        return (c, b);
    }
    let mut tail = [0u8; 12];
    tail[..k.len()].copy_from_slice(k);
    let w = |i: usize| u32::from_le_bytes([tail[i], tail[i + 1], tail[i + 2], tail[i + 3]]);
    a = a.wrapping_add(w(0));
    b = b.wrapping_add(w(4));
    c = c.wrapping_add(w(8));
    // final
    c ^= b;
    c = c.wrapping_sub(rot(b, 14));
    a ^= c;
    a = a.wrapping_sub(rot(c, 11));
    b ^= a;
    b = b.wrapping_sub(rot(a, 25));
    c ^= b;
    c = c.wrapping_sub(rot(b, 16));
    a ^= c;
    a = a.wrapping_sub(rot(c, 4));
    b ^= a;
    b = b.wrapping_sub(rot(a, 14));
    c ^= b;
    c = c.wrapping_sub(rot(b, 24));
    pc = c;
    pb = b;
    (pc, pb)
}

/// Hash of a file path as stored in the root file: upper case, with
/// backslashes.
pub fn path_hash(path: &str) -> u64 {
    let normalized: Vec<u8> = path
        .bytes()
        .map(|b| {
            if b == b'/' {
                b'\\'
            } else {
                b.to_ascii_uppercase()
            }
        })
        .collect();
    let (pc, pb) = hashlittle2(&normalized, 0, 0);
    ((pc as u64) << 32) | pb as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_reference_values() {
        // Test vectors from lookup3.c's driver.
        assert_eq!(hashlittle2(b"", 0, 0), (0xdeadbeef, 0xdeadbeef));
        assert_eq!(hashlittle2(b"", 0, 0xdeadbeef), (0xbd5b7dde, 0xdeadbeef));
        assert_eq!(
            hashlittle2(b"Four score and seven years ago", 0, 0).0,
            0x17770551
        );
        assert_eq!(
            hashlittle2(b"Four score and seven years ago", 1, 0).0,
            0xcd628161
        );
    }
}
