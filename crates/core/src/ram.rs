//! The main memory of the machine.

/// A flat block of RAM at a fixed physical base address.
///
/// The bus checks that an access falls inside the block before it calls a
/// method here, so each method takes an offset and not an address.
pub struct Ram {
    data: Vec<u8>,
}

impl Ram {
    /// Makes a block of `size` bytes, filled with zeros.
    pub fn new(size: usize) -> Self {
        Self {
            data: vec![0; size],
        }
    }

    /// The size of the block in bytes.
    pub fn size(&self) -> u64 {
        self.data.len() as u64
    }

    /// Reads `width` bytes (1, 2, 4 or 8) at `offset`, little endian.
    ///
    /// Returns `None` if the access does not fit in the block.
    pub fn read(&self, offset: u64, width: usize) -> Option<u64> {
        let b = self.slice(offset, width)?;
        // One arm for each width, so each copy has a fixed size and the
        // compiler makes it one load, and not a call to memcpy.
        Some(match width {
            1 => u64::from(b[0]),
            2 => u64::from(u16::from_le_bytes([b[0], b[1]])),
            4 => u64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            8 => u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]),
            _ => return None,
        })
    }

    /// Writes the low `width` bytes (1, 2, 4 or 8) of `value` at `offset`.
    ///
    /// Returns `None` if the access does not fit in the block.
    pub fn write(&mut self, offset: u64, width: usize, value: u64) -> Option<()> {
        let start = usize::try_from(offset).ok()?;
        let end = start.checked_add(width)?;
        let dest = self.data.get_mut(start..end)?;
        match width {
            1 => dest[0] = value as u8,
            2 => dest.copy_from_slice(&(value as u16).to_le_bytes()),
            4 => dest.copy_from_slice(&(value as u32).to_le_bytes()),
            8 => dest.copy_from_slice(&value.to_le_bytes()),
            _ => return None,
        }
        Some(())
    }

    /// Gives the bytes at `offset..offset + len`.
    pub fn slice(&self, offset: u64, len: usize) -> Option<&[u8]> {
        let start = usize::try_from(offset).ok()?;
        self.data.get(start..start.checked_add(len)?)
    }

    /// Gives the bytes at `offset..offset + len` for a write.
    pub fn slice_mut(&mut self, offset: u64, len: usize) -> Option<&mut [u8]> {
        let start = usize::try_from(offset).ok()?;
        self.data.get_mut(start..start.checked_add(len)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_reads_back_in_little_endian_order() {
        let mut ram = Ram::new(16);
        ram.write(4, 4, 0x1122_3344).unwrap();
        assert_eq!(ram.read(4, 1), Some(0x44));
        assert_eq!(ram.read(4, 2), Some(0x3344));
        assert_eq!(ram.read(4, 4), Some(0x1122_3344));
        assert_eq!(ram.slice(4, 4).unwrap(), &[0x44, 0x33, 0x22, 0x11]);
    }

    #[test]
    fn a_write_only_changes_its_own_bytes() {
        let mut ram = Ram::new(16);
        ram.write(0, 8, u64::MAX).unwrap();
        ram.write(2, 2, 0).unwrap();
        assert_eq!(ram.read(0, 8), Some(0xffff_ffff_0000_ffff));
    }

    #[test]
    fn a_width_that_is_not_a_power_of_two_fails() {
        let mut ram = Ram::new(16);
        assert_eq!(ram.read(0, 3), None);
        assert_eq!(ram.write(0, 3, 0), None);
    }

    #[test]
    fn an_access_past_the_end_fails() {
        let mut ram = Ram::new(16);
        assert_eq!(ram.read(12, 8), None);
        assert_eq!(ram.write(16, 1, 0), None);
        assert_eq!(ram.read(u64::MAX, 1), None);
        assert_eq!(ram.read(8, 8), Some(0));
    }
}
