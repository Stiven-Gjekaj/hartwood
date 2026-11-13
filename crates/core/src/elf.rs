//! A reader for 64-bit RISC-V ELF files.
//!
//! It reads only what an emulator needs: the entry point, the segments to
//! load, and the symbol table. The symbol table gives the address of `tohost`
//! for the test programs, and names for the debugger.

use std::fmt;

const EM_RISCV: u16 = 243;
const PT_LOAD: u32 = 1;
const SHT_SYMTAB: u32 = 2;

/// An error that stops the read of an ELF file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElfError {
    /// The file is shorter than a header or a table says.
    Truncated,
    /// The file does not start with the ELF magic number.
    NotElf,
    /// The file is not a 64-bit little endian RISC-V file.
    WrongKind(&'static str),
}

impl fmt::Display for ElfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ElfError::Truncated => write!(f, "the ELF file is truncated"),
            ElfError::NotElf => write!(f, "the file is not an ELF file"),
            ElfError::WrongKind(what) => write!(f, "the ELF file is not {what}"),
        }
    }
}

impl std::error::Error for ElfError {}

/// One loadable segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// The physical address of the first byte.
    pub addr: u64,
    /// The bytes from the file.
    pub data: Vec<u8>,
    /// The size in memory. The bytes after `data` up to this size are zero.
    pub mem_size: u64,
}

/// A symbol from the symbol table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub addr: u64,
    pub size: u64,
}

/// The parts of an ELF file that the emulator uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Elf {
    pub entry: u64,
    pub segments: Vec<Segment>,
    pub symbols: Vec<Symbol>,
}

impl Elf {
    /// Reads an ELF file from its bytes.
    pub fn parse(bytes: &[u8]) -> Result<Elf, ElfError> {
        let r = Reader(bytes);
        if r.bytes(0, 4)? != b"\x7fELF" {
            return Err(ElfError::NotElf);
        }
        if r.u8(4)? != 2 {
            return Err(ElfError::WrongKind("a 64-bit file"));
        }
        if r.u8(5)? != 1 {
            return Err(ElfError::WrongKind("little endian"));
        }
        if r.u16(18)? != EM_RISCV {
            return Err(ElfError::WrongKind("a RISC-V file"));
        }

        let entry = r.u64(24)?;
        let ph_off = r.u64(32)?;
        let sh_off = r.u64(40)?;
        let ph_size = u64::from(r.u16(54)?);
        let ph_count = u64::from(r.u16(56)?);
        let sh_size = u64::from(r.u16(58)?);
        let sh_count = u64::from(r.u16(60)?);

        let mut segments = Vec::new();
        for i in 0..ph_count {
            let ph = ph_off + i * ph_size;
            if r.u32(ph)? != PT_LOAD {
                continue;
            }
            let offset = r.u64(ph + 8)?;
            let paddr = r.u64(ph + 24)?;
            let file_size = r.u64(ph + 32)?;
            let mem_size = r.u64(ph + 40)?;
            let data = r.bytes(offset, file_size)?.to_vec();
            segments.push(Segment {
                addr: paddr,
                data,
                mem_size,
            });
        }

        let mut symbols = Vec::new();
        for i in 0..sh_count {
            let sh = sh_off + i * sh_size;
            if r.u32(sh + 4)? != SHT_SYMTAB {
                continue;
            }
            let table_off = r.u64(sh + 24)?;
            let table_size = r.u64(sh + 32)?;
            let link = u64::from(r.u32(sh + 40)?);
            let entry_size = r.u64(sh + 56)?.max(24);
            let strtab = sh_off + link * sh_size;
            let str_off = r.u64(strtab + 24)?;
            let str_size = r.u64(strtab + 32)?;
            let strings = r.bytes(str_off, str_size)?;
            for j in 0..table_size / entry_size {
                let sym = table_off + j * entry_size;
                let name_off = r.u32(sym)? as usize;
                let name = strings
                    .get(name_off..)
                    .and_then(|s| s.split(|&b| b == 0).next())
                    .ok_or(ElfError::Truncated)?;
                if name.is_empty() {
                    continue;
                }
                symbols.push(Symbol {
                    name: String::from_utf8_lossy(name).into_owned(),
                    addr: r.u64(sym + 8)?,
                    size: r.u64(sym + 16)?,
                });
            }
        }

        Ok(Elf {
            entry,
            segments,
            symbols,
        })
    }

    /// Gives the address of the symbol with this name.
    pub fn symbol(&self, name: &str) -> Option<u64> {
        self.symbols.iter().find(|s| s.name == name).map(|s| s.addr)
    }
}

/// Reads little endian values at file offsets, and fails on a short file.
struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn bytes(&self, offset: u64, len: u64) -> Result<&[u8], ElfError> {
        let start = usize::try_from(offset).map_err(|_| ElfError::Truncated)?;
        let len = usize::try_from(len).map_err(|_| ElfError::Truncated)?;
        let end = start.checked_add(len).ok_or(ElfError::Truncated)?;
        self.0.get(start..end).ok_or(ElfError::Truncated)
    }

    fn u8(&self, offset: u64) -> Result<u8, ElfError> {
        Ok(self.bytes(offset, 1)?[0])
    }

    fn u16(&self, offset: u64) -> Result<u16, ElfError> {
        let b = self.bytes(offset, 2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&self, offset: u64) -> Result<u32, ElfError> {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(self.bytes(offset, 4)?);
        Ok(u32::from_le_bytes(buf))
    }

    fn u64(&self, offset: u64) -> Result<u64, ElfError> {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(self.bytes(offset, 8)?);
        Ok(u64::from_le_bytes(buf))
    }
}

/// Writes a small ELF file for tests: one segment and an optional symbol
/// table. Each test builds its own program with it, so no test reads a file
/// from the disk.
#[cfg(any(test, feature = "test-util"))]
pub mod builder {
    /// Builds an ELF file that loads `code` at `addr`, with the entry at
    /// `addr`, and with a symbol for each `(name, address)` pair.
    pub fn build(addr: u64, code: &[u8], symbols: &[(&str, u64)]) -> Vec<u8> {
        let mut out = vec![0u8; 64 + 56];
        let code_off = out.len() as u64;
        out.extend_from_slice(code);

        // The string table: a zero byte, then each name with a zero byte.
        let mut strtab = vec![0u8];
        let mut name_offs = Vec::new();
        for (name, _) in symbols {
            name_offs.push(strtab.len() as u32);
            strtab.extend_from_slice(name.as_bytes());
            strtab.push(0);
        }
        let strtab_off = out.len() as u64;
        out.extend_from_slice(&strtab);
        while !out.len().is_multiple_of(8) {
            out.push(0);
        }

        // The symbol table: a null entry, then one entry for each symbol.
        let symtab_off = out.len() as u64;
        out.extend_from_slice(&[0u8; 24]);
        for (i, (_, value)) in symbols.iter().enumerate() {
            let mut sym = [0u8; 24];
            sym[0..4].copy_from_slice(&name_offs[i].to_le_bytes());
            sym[8..16].copy_from_slice(&value.to_le_bytes());
            sym[16..24].copy_from_slice(&8u64.to_le_bytes());
            out.extend_from_slice(&sym);
        }
        let symtab_size = (symbols.len() as u64 + 1) * 24;

        // Three section headers: null, the symbol table, the strings.
        let sh_off = out.len() as u64;
        out.extend_from_slice(&[0u8; 64]);
        let mut symtab = [0u8; 64];
        symtab[4..8].copy_from_slice(&2u32.to_le_bytes());
        symtab[24..32].copy_from_slice(&symtab_off.to_le_bytes());
        symtab[32..40].copy_from_slice(&symtab_size.to_le_bytes());
        symtab[40..44].copy_from_slice(&2u32.to_le_bytes());
        symtab[56..64].copy_from_slice(&24u64.to_le_bytes());
        out.extend_from_slice(&symtab);
        let mut strsec = [0u8; 64];
        strsec[4..8].copy_from_slice(&3u32.to_le_bytes());
        strsec[24..32].copy_from_slice(&strtab_off.to_le_bytes());
        strsec[32..40].copy_from_slice(&(strtab.len() as u64).to_le_bytes());
        out.extend_from_slice(&strsec);

        // The file header.
        out[0..4].copy_from_slice(b"\x7fELF");
        out[4] = 2;
        out[5] = 1;
        out[6] = 1;
        out[16..18].copy_from_slice(&2u16.to_le_bytes());
        out[18..20].copy_from_slice(&243u16.to_le_bytes());
        out[20..24].copy_from_slice(&1u32.to_le_bytes());
        out[24..32].copy_from_slice(&addr.to_le_bytes());
        out[32..40].copy_from_slice(&64u64.to_le_bytes());
        out[40..48].copy_from_slice(&sh_off.to_le_bytes());
        out[52..54].copy_from_slice(&64u16.to_le_bytes());
        out[54..56].copy_from_slice(&56u16.to_le_bytes());
        out[56..58].copy_from_slice(&1u16.to_le_bytes());
        out[58..60].copy_from_slice(&64u16.to_le_bytes());
        out[60..62].copy_from_slice(&3u16.to_le_bytes());

        // The one program header.
        let ph = 64;
        out[ph..ph + 4].copy_from_slice(&1u32.to_le_bytes());
        out[ph + 4..ph + 8].copy_from_slice(&7u32.to_le_bytes());
        out[ph + 8..ph + 16].copy_from_slice(&code_off.to_le_bytes());
        out[ph + 16..ph + 24].copy_from_slice(&addr.to_le_bytes());
        out[ph + 24..ph + 32].copy_from_slice(&addr.to_le_bytes());
        out[ph + 32..ph + 40].copy_from_slice(&(code.len() as u64).to_le_bytes());
        out[ph + 40..ph + 48].copy_from_slice(&(code.len() as u64 + 16).to_le_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_built_file_reads_back() {
        let code = [1, 2, 3, 4];
        let bytes = builder::build(0x8000_0000, &code, &[("tohost", 0x8000_1000)]);
        let elf = Elf::parse(&bytes).unwrap();
        assert_eq!(elf.entry, 0x8000_0000);
        assert_eq!(elf.segments.len(), 1);
        assert_eq!(elf.segments[0].addr, 0x8000_0000);
        assert_eq!(elf.segments[0].data, code);
        assert_eq!(elf.segments[0].mem_size, 20);
        assert_eq!(elf.symbol("tohost"), Some(0x8000_1000));
        assert_eq!(elf.symbol("fromhost"), None);
    }

    #[test]
    fn a_file_with_no_magic_number_is_refused() {
        assert_eq!(Elf::parse(&[0u8; 64]), Err(ElfError::NotElf));
    }

    #[test]
    fn a_short_file_is_refused() {
        let bytes = builder::build(0x8000_0000, &[0; 4], &[]);
        assert_eq!(Elf::parse(&bytes[..70]), Err(ElfError::Truncated));
        assert_eq!(Elf::parse(b"\x7fELF"), Err(ElfError::Truncated));
    }

    #[test]
    fn a_file_for_another_machine_is_refused() {
        let mut bytes = builder::build(0x8000_0000, &[0; 4], &[]);
        bytes[18] = 62;
        assert_eq!(
            Elf::parse(&bytes),
            Err(ElfError::WrongKind("a RISC-V file"))
        );
    }
}
