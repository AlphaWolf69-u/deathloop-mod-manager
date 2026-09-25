//! Explicit x64 relocations for Lua-authored code patches. No implicit instruction relocation.
use crate::Result;

#[derive(Debug, Clone)]
pub struct Relocation {
    pub offset: usize,
    pub target: usize,
    pub cave: bool,
}

pub fn decode_hex(s: &str) -> Result<Vec<u8>> {
    let compact: String = s.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if compact.is_empty()
        || compact.len() > 131072
        || !compact.len().is_multiple_of(2)
        || !compact.is_ascii()
    {
        return Err("Expected 1..65536 hex bytes".into());
    }
    (0..compact.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&compact[i..i + 2], 16).map_err(|_| "Invalid hex byte".into()))
        .collect()
}

pub fn relocate(
    bytes: &mut [u8],
    address: usize,
    base: usize,
    cave: usize,
    cave_size: usize,
    relocations: &[Relocation],
) -> Result<()> {
    let mut used = std::collections::BTreeSet::new();
    for r in relocations {
        let end = r.offset.checked_add(4).ok_or("Relocation overflow")?;
        if end > bytes.len() || (r.offset..end).any(|i| !used.insert(i)) {
            return Err("Invalid or overlapping rel32 relocation".into());
        }
        if r.cave && r.target >= cave_size {
            return Err("Relocation target outside cave".into());
        }
        let target = (if r.cave { cave } else { base })
            .checked_add(r.target)
            .ok_or("Target overflow")?;
        let from = address.checked_add(end).ok_or("Source overflow")?;
        let displacement =
            i32::try_from(target as i128 - from as i128).map_err(|_| "rel32 out of range")?;
        bytes[r.offset..end].copy_from_slice(&displacement.to_le_bytes());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hex_validation() {
        assert_eq!(decode_hex("90 E9\n00").unwrap(), [0x90, 0xe9, 0]);
        for s in ["", "0", "zz", "éé"] {
            assert!(decode_hex(s).is_err());
        }
    }
    #[test]
    fn forward_backward_and_invalid_relocations() {
        let mut b = [0u8; 8];
        let r = Relocation {
            offset: 0,
            target: 0x20,
            cave: true,
        };
        relocate(&mut b, 0x1000, 0, 0x2000, 4096, std::slice::from_ref(&r)).unwrap();
        assert_eq!(i32::from_le_bytes(b[..4].try_into().unwrap()), 0x101c);
        relocate(&mut b, 0x3000, 0, 0x2000, 4096, std::slice::from_ref(&r)).unwrap();
        assert_eq!(i32::from_le_bytes(b[..4].try_into().unwrap()), -0xfe4);
        assert!(relocate(&mut b, 0, 0, 0x90000000, 4096, std::slice::from_ref(&r)).is_err());
        assert!(relocate(&mut b, 0, 0, 0, 16, std::slice::from_ref(&r)).is_err());
        assert!(relocate(&mut b, 0, 0, 0, 4096, &[r.clone(), r]).is_err());
    }
}
