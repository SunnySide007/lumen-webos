//! glibc compatibility for the TV build.
//!
//! webosbrew's SDK links against a glibc 2.12 sysroot so binaries run on every
//! webOS release, but Rust's std calls `getauxval`, which glibc only gained in
//! 2.16. The executable therefore carries its own, reading the auxiliary vector
//! the kernel publishes in `/proc/self/auxv`. plx-native solves the same link
//! error the same way (`src/compat/getauxval.c`).

/// Looks `kind` up in a raw auxiliary vector of native-endian `word`-byte
/// (type, value) pairs, stopping at `AT_NULL`. Unknown types read as 0, which
/// is what glibc returns for them.
// Only the TV build calls it outside tests.
#[cfg_attr(not(all(target_os = "linux", target_arch = "arm")), allow(dead_code))]
pub fn find_aux(auxv: &[u8], kind: u64, word: usize) -> u64 {
    let read = |bytes: &[u8]| -> u64 {
        let mut buf = [0u8; 8];
        if cfg!(target_endian = "little") {
            buf[..word].copy_from_slice(bytes);
            u64::from_le_bytes(buf)
        } else {
            buf[8 - word..].copy_from_slice(bytes);
            u64::from_be_bytes(buf)
        }
    };
    for pair in auxv.chunks_exact(word * 2) {
        let (key, value) = pair.split_at(word);
        match read(key) {
            0 => break,
            k if k == kind => return read(value),
            _ => {}
        }
    }
    0
}

/// The TV-only symbol. On 32-bit ARM `unsigned long` is 32 bits.
#[cfg(all(target_os = "linux", target_arch = "arm"))]
#[unsafe(no_mangle)]
pub extern "C" fn getauxval(kind: std::ffi::c_ulong) -> std::ffi::c_ulong {
    match std::fs::read("/proc/self/auxv") {
        Ok(auxv) => {
            find_aux(&auxv, u64::from(kind), size_of::<std::ffi::c_ulong>()) as std::ffi::c_ulong
        }
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auxv32(pairs: &[(u32, u32)]) -> Vec<u8> {
        pairs
            .iter()
            .flat_map(|(k, v)| [k.to_ne_bytes(), v.to_ne_bytes()].concat())
            .collect()
    }

    #[test]
    fn finds_a_32_bit_entry() {
        const AT_PAGESZ: u32 = 6;
        const AT_HWCAP: u32 = 16;
        let auxv = auxv32(&[(AT_PAGESZ, 4096), (AT_HWCAP, 0x1FB8D7), (0, 0)]);
        assert_eq!(find_aux(&auxv, 16, 4), 0x1FB8D7);
        assert_eq!(find_aux(&auxv, 6, 4), 4096);
    }

    #[test]
    fn missing_entries_and_entries_after_at_null_read_as_zero() {
        const AT_MINSIGSTKSZ: u32 = 51;
        let auxv = auxv32(&[(6, 4096), (0, 0), (AT_MINSIGSTKSZ, 8192)]);
        assert_eq!(find_aux(&auxv, 51, 4), 0);
        assert_eq!(find_aux(&[], 6, 4), 0);
    }

    #[test]
    fn reads_64_bit_words() {
        let auxv: Vec<u8> = [(6u64, 16384u64), (0, 0)]
            .iter()
            .flat_map(|(k, v)| [k.to_ne_bytes(), v.to_ne_bytes()].concat())
            .collect();
        assert_eq!(find_aux(&auxv, 6, 8), 16384);
    }
}
