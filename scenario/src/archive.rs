//! The archive an unsigned index is served in: a tar archive, gzipped; and
//! the member of a tar stream an index is read from.
//!
//! apk reads `APKINDEX.tar.gz` through zlib, which reads a deflate stream of
//! stored blocks as readily as a compressed one, so nothing here compresses.

/// A ustar archive holding `files`, each a name and its contents.
pub fn tar(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut archive = Vec::new();
    for (name, contents) in files {
        let mut header = [0u8; 512];
        let field = |header: &mut [u8; 512], at: usize, value: &[u8]| {
            header[at..at + value.len()].copy_from_slice(value);
        };
        field(&mut header, 0, name.as_bytes());
        field(&mut header, 100, b"0000644\0");
        field(&mut header, 108, b"0000000\0");
        field(&mut header, 116, b"0000000\0");
        field(
            &mut header,
            124,
            format!("{:011o}\0", contents.len()).as_bytes(),
        );
        field(&mut header, 136, b"14544517400\0");
        field(&mut header, 148, b"        ");
        header[156] = b'0';
        field(&mut header, 257, b"ustar\0");
        field(&mut header, 263, b"00");
        let sum: u32 = header.iter().map(|&byte| u32::from(byte)).sum();
        field(&mut header, 148, format!("{sum:06o}\0 ").as_bytes());
        archive.extend_from_slice(&header);
        archive.extend_from_slice(contents);
        archive.resize(archive.len().div_ceil(512) * 512, 0);
    }
    archive.resize(archive.len() + 1024, 0);
    archive
}

/// The contents of the first member named `name` in the tar stream
/// `archive`, before the stream's end: its first block of zeros, or the
/// end of the bytes. A member is matched by the name its header states.
///
/// A signed index is two gzip members, decompressed as one tar stream: the
/// signature, and then the signed index, whose end blocks end the archive.
/// Reading stops at the first member of the name, and at the end, so
/// neither a second member of the name nor anything after the signed
/// stream is read.
pub fn member<'a>(archive: &'a [u8], name: &str) -> Option<&'a [u8]> {
    let mut at: usize = 0;
    while let Some(header) = archive.get(at..at.checked_add(512)?) {
        if header.iter().all(|&byte| byte == 0) {
            return None;
        }
        let named = header.get(..100)?;
        let named = &named[..named.iter().position(|&byte| byte == 0).unwrap_or(100)];
        let size = octal(header.get(124..136)?)?;
        let start = at + 512;
        if named == name.as_bytes() {
            return archive.get(start..start.checked_add(size)?);
        }
        at = start.checked_add(size.div_ceil(512).checked_mul(512)?)?;
    }
    None
}

/// The number a tar header's octal field holds: digits, padded with
/// spaces or zero bytes.
fn octal(field: &[u8]) -> Option<usize> {
    let digits = field
        .iter()
        .copied()
        .skip_while(|&byte| byte == b' ')
        .take_while(|&byte| byte != 0 && byte != b' ');
    let mut value: usize = 0;
    for digit in digits {
        if !(b'0'..=b'7').contains(&digit) {
            return None;
        }
        value = value
            .checked_mul(8)?
            .checked_add(usize::from(digit - b'0'))?;
    }
    Some(value)
}

/// `data` in a gzip stream of stored deflate blocks.
pub fn gzip(data: &[u8]) -> Vec<u8> {
    let mut stream = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff];
    let mut blocks = data.chunks(0xffff).peekable();
    if blocks.peek().is_none() {
        stream.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while let Some(block) = blocks.next() {
        let length = u16::try_from(block.len()).unwrap_or(u16::MAX);
        stream.push(u8::from(blocks.peek().is_none()));
        stream.extend_from_slice(&length.to_le_bytes());
        stream.extend_from_slice(&(!length).to_le_bytes());
        stream.extend_from_slice(block);
    }
    stream.extend_from_slice(&crc32(data).to_le_bytes());
    stream.extend_from_slice(&(data.len() as u32).to_le_bytes());
    stream
}

/// The CRC-32 gzip carries (ISO-HDLC: reflected, polynomial 0xedb88320).
fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (crc & 1).wrapping_neg());
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checksum_is_gzip_s() {
        // The standard check value of CRC-32/ISO-HDLC.
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn a_long_input_spans_blocks() {
        let data = vec![7u8; 0x2_0000];
        let stream = gzip(&data);
        // Header, two full blocks and a short one, each with five bytes of
        // framing, and the trailer.
        assert_eq!(stream.len(), 10 + data.len() + 3 * 5 + 8);
        assert_eq!(stream[10], 0);
        assert_eq!(
            &stream[stream.len() - 4..],
            &(data.len() as u32).to_le_bytes()
        );
    }

    #[test]
    fn the_first_member_of_a_name_is_read_and_nothing_past_the_end() {
        let archive = tar(&[
            ("DESCRIPTION", b"description"),
            ("APKINDEX", b"first"),
            ("APKINDEX", b"second"),
        ]);
        assert_eq!(member(&archive, "APKINDEX"), Some(&b"first"[..]));
        assert_eq!(member(&archive, "DESCRIPTION"), Some(&b"description"[..]));
        assert_eq!(member(&archive, "APK"), None);
        // A member after the end blocks is not read.
        let appended = [tar(&[("DESCRIPTION", b"d")]), tar(&[("APKINDEX", b"x")])].concat();
        assert_eq!(member(&appended, "APKINDEX"), None);
        // Nor is a member cut short.
        let cut = tar(&[("APKINDEX", &[b'a'; 600][..])]);
        assert_eq!(member(&cut[..1000], "APKINDEX"), None);
    }

    #[test]
    fn a_tar_member_is_padded_to_a_block() {
        let archive = tar(&[("APKINDEX", b"P:a\n")]);
        assert_eq!(archive.len(), 512 + 512 + 1024);
        assert_eq!(&archive[257..262], b"ustar");
        assert_eq!(&archive[512..516], b"P:a\n");
    }
}
