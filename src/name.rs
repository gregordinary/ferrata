//! Names, and the bytes apk holds them as.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::borrow::Borrow;
use core::fmt::{self, Write};

/// A name a package carries, provides, or depends on, as apk holds it: a
/// string of bytes.
///
/// apk reads names as bytes and compares them byte for byte. It does not
/// require them to be UTF-8: a package whose name holds a byte that is not
/// is loaded, and installed when something asks for those bytes. A `Name`
/// is those bytes. Every name in Alpine's indexes is ASCII, so for almost
/// every name a caller meets, [`to_str`](Name::to_str) gives its text.
///
/// A name equals a `str`, a `String` or a byte string (a slice, an array or a
/// `Vec<u8>`) holding the same bytes, from either side, and so does a
/// reference to a name, as the accessors give one; names order byte by
/// byte. [`Display`](fmt::Display) writes the name as text:
/// UTF-8 as it is, each byte that is not UTF-8 as `\xNN`, and a backslash
/// as `\\`, so no two names are written alike. A control character is UTF-8,
/// and written as it is: a caller writing to a terminal quotes the text.
/// [`Debug`](fmt::Debug) writes the name quoted, as a `str` is, each byte
/// that is not UTF-8 as `\xNN`.
///
/// ```
/// use ferrata::Dialect;
///
/// let entry = Dialect::V3_0.dependency("musl>=1.2")?;
/// assert_eq!(entry.name(), "musl");
/// assert_eq!(entry.name().to_str(), Some("musl"));
///
/// let entry = Dialect::V3_0.dependency(b"lib\xff>=1")?;
/// assert_eq!(entry.name(), b"lib\xff");
/// assert_eq!(entry.name().to_str(), None);
/// assert_eq!(entry.name().to_string(), r"lib\xff");
/// assert_eq!(format!("{:?}", entry.name()), r#""lib\xff""#);
///
/// // The four bytes `\xff` are another name, and written otherwise.
/// let entry = Dialect::V3_0.dependency(r"lib\xff>=1")?;
/// assert_eq!(entry.name().to_string(), r"lib\\xff");
/// # Ok::<(), ferrata::dependency::DependencyError>(())
/// ```
#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name(Box<[u8]>);

impl Name {
    /// The name's bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The name as text, where its bytes are UTF-8.
    pub fn to_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.0).ok()
    }
}

impl From<&[u8]> for Name {
    fn from(bytes: &[u8]) -> Name {
        Name(bytes.into())
    }
}

impl From<&str> for Name {
    fn from(text: &str) -> Name {
        Name::from(text.as_bytes())
    }
}

impl AsRef<[u8]> for Name {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Borrow<[u8]> for Name {
    fn borrow(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&Text(&self.0), f)
    }
}

impl fmt::Debug for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&Text(&self.0), f)
    }
}

/// Makes a name equal to each of the given types that holds the same bytes,
/// from either side.
macro_rules! equal_to_bytes {
    ($($other:ty),*) => {$(
        impl PartialEq<$other> for Name {
            fn eq(&self, other: &$other) -> bool {
                *self.0 == *AsRef::<[u8]>::as_ref(other)
            }
        }

        impl PartialEq<Name> for $other {
            fn eq(&self, other: &Name) -> bool {
                other == self
            }
        }
    )*};
}

equal_to_bytes!(str, &str, String, [u8], &[u8], Vec<u8>);

/// Makes a reference to a name, which is what every accessor gives, equal to
/// each of the given owned types, from either side. A reference compared
/// with a reference, or with a `str` or a byte string, goes through the
/// name's own comparisons; an owned value has no other way.
macro_rules! reference_equal_to_owned {
    ($($other:ty),*) => {$(
        impl PartialEq<$other> for &Name {
            fn eq(&self, other: &$other) -> bool {
                **self == *other
            }
        }

        impl PartialEq<&Name> for $other {
            fn eq(&self, other: &&Name) -> bool {
                **other == *self
            }
        }
    )*};
}

reference_equal_to_owned!(String, Vec<u8>);

impl<const N: usize> PartialEq<[u8; N]> for Name {
    fn eq(&self, other: &[u8; N]) -> bool {
        *self.0 == *other
    }
}

impl<const N: usize> PartialEq<&[u8; N]> for Name {
    fn eq(&self, other: &&[u8; N]) -> bool {
        *self.0 == **other
    }
}

impl<const N: usize> PartialEq<Name> for [u8; N] {
    fn eq(&self, other: &Name) -> bool {
        other == self
    }
}

impl<const N: usize> PartialEq<Name> for &[u8; N] {
    fn eq(&self, other: &Name) -> bool {
        other == self
    }
}

/// Bytes read from an index, a database or a request, written as text: the
/// one rendering every `Display` of the crate that writes such bytes goes
/// through.
///
/// [`Display`](fmt::Display) writes UTF-8 as it is, each byte that is not
/// part of UTF-8 as `\xNN`, and a backslash as `\\`. apk accepts a
/// backslash wherever it accepts a name, and in a package's version
/// (measured with apk.static 3.0.9): `a\xff`, the four bytes, and `a`
/// followed by the byte 0xFF are two packages it installs side by side.
/// With the backslash escaped too, a backslash in what is written always
/// begins an escape, so two different strings of bytes are never written
/// alike, and the bytes can be read back from the text.
///
/// [`Debug`](fmt::Debug) writes them quoted, as a `str` is, each byte that
/// is not part of UTF-8 escaped as `\xNN`.
pub(crate) struct Text<'a>(pub(crate) &'a [u8]);

impl fmt::Display for Text<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for chunk in self.0.utf8_chunks() {
            for (at, run) in chunk.valid().split('\\').enumerate() {
                if at > 0 {
                    f.write_str("\\\\")?;
                }
                f.write_str(run)?;
            }
            for byte in chunk.invalid() {
                write!(f, "\\x{byte:02x}")?;
            }
        }
        Ok(())
    }
}

impl fmt::Debug for Text<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_char('"')?;
        for chunk in self.0.utf8_chunks() {
            for character in chunk.valid().chars() {
                // A `str` writes a single quote as it is.
                if character == '\'' {
                    f.write_char(character)?;
                } else {
                    write!(f, "{}", character.escape_debug())?;
                }
            }
            for byte in chunk.invalid() {
                write!(f, "\\x{byte:02x}")?;
            }
        }
        f.write_char('"')
    }
}

#[cfg(test)]
mod tests {
    use std::string::ToString;
    use std::vec::Vec;
    use std::{format, vec};

    use super::*;

    #[test]
    fn a_name_is_written_as_text_and_debugged_as_bytes() {
        let name = Name::from(b"a\"b\xff\xc3".as_slice());
        assert_eq!(name.to_string(), r#"a"b\xff\xc3"#);
        assert_eq!(format!("{name:?}"), r#""a\"b\xff\xc3""#);
        let text = Name::from("it's é\t");
        assert_eq!(text.to_string(), "it's é\t");
        assert_eq!(format!("{text:?}"), format!("{:?}", "it's é\t"));
        assert_eq!(text.to_str(), Some("it's é\t"));
    }

    #[test]
    fn no_two_strings_of_bytes_are_written_alike() {
        // Each byte of a sequence that is not UTF-8 is written on its own:
        // a lead byte without its continuation, an encoded surrogate, an
        // overlong `/`.
        for (bytes, written) in [
            (&b"a\xc3"[..], r"a\xc3"),
            (b"\xed\xa0\x80", r"\xed\xa0\x80"),
            (b"\xc0\xaf", r"\xc0\xaf"),
            (b"\xe2\x82\xac\xe2\x82", "\u{20ac}\\xe2\\x82"),
            (br"a\xff", r"a\\xff"),
            (br"\", r"\\"),
            (b"\\\xff", r"\\\xff"),
        ] {
            assert_eq!(Text(bytes).to_string(), written, "{bytes:?}");
        }
        // Every string of up to four bytes from a small alphabet is written
        // otherwise than every other, and its bytes read back from what is
        // written.
        let alphabet: &[u8] = b"\\x f\xff\xc3\xa9";
        let mut strings: Vec<Vec<u8>> = vec![Vec::new()];
        for length in 1..=4 {
            let shorter: Vec<Vec<u8>> = strings
                .iter()
                .filter(|string| string.len() == length - 1)
                .cloned()
                .collect();
            for string in shorter {
                for &byte in alphabet {
                    let mut longer = string.clone();
                    longer.push(byte);
                    strings.push(longer);
                }
            }
        }
        let mut seen = std::collections::BTreeMap::new();
        for string in &strings {
            let written = Text(string).to_string();
            assert_eq!(&unescaped(&written), string, "{written}");
            if let Some(other) = seen.insert(written.clone(), string.clone()) {
                panic!("{other:?} and {string:?} are both written {written}");
            }
        }
    }

    /// The bytes `written` names, read back from `Text`'s rendering.
    fn unescaped(written: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut rest = written.as_bytes();
        while let Some((&byte, after)) = rest.split_first() {
            rest = after;
            if byte != b'\\' {
                bytes.push(byte);
                continue;
            }
            match rest {
                [b'\\', after @ ..] => {
                    bytes.push(b'\\');
                    rest = after;
                }
                [b'x', high, low, after @ ..] => {
                    let hex = [*high, *low];
                    bytes.push(u8::from_str_radix(std::str::from_utf8(&hex).unwrap(), 16).unwrap());
                    rest = after;
                }
                _ => panic!("{written} holds a lone backslash"),
            }
        }
        bytes
    }

    #[test]
    fn a_name_equals_the_bytes_it_holds() {
        let name = Name::from("musl");
        assert_eq!(name, "musl");
        assert_eq!("musl", name);
        assert_eq!(name, *b"musl");
        assert_eq!(*b"musl", name);
        assert_eq!(name, b"musl");
        assert_eq!(b"musl", name);
        assert_eq!(name, String::from("musl"));
        assert_eq!(String::from("musl"), name);
        assert_eq!(name, b"musl".to_vec());
        assert_eq!(b"musl".to_vec(), name);
        assert_ne!(name, "musl ");
        // A reference, as the accessors give, compared with each, from
        // either side.
        let name = &name;
        let (text, bytes) = (String::from("musl"), b"musl".to_vec());
        assert!(name == "musl" && "musl" == name);
        assert!(name == b"musl" && b"musl" == name);
        assert!(name == text && text == name);
        assert!(name == bytes && bytes == name);
        let names = vec![name, name];
        assert_eq!(names, ["musl", "musl"]);
        assert_eq!(names, [String::from("musl"), String::from("musl")]);
        assert_ne!(name, String::from("musl "));
        let (ascii, above) = (Name::from("a"), Name::from(b"\xff".as_slice()));
        assert!(ascii < above);
    }
}
