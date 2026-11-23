//! Bounds-checked big-endian reads. A read past the end returns `None`, so a malformed font
//! fails with an error instead of a panic.

/// A cursor over font data.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub(crate) fn at(data: &'a [u8], pos: usize) -> Option<Self> {
        (pos <= data.len()).then_some(Self { data, pos })
    }

    pub(crate) fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    pub(crate) fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let bytes = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(bytes)
    }

    pub(crate) fn skip(&mut self, n: usize) -> Option<()> {
        self.bytes(n).map(|_| ())
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.bytes(N)?.try_into().ok()
    }

    pub(crate) fn u8(&mut self) -> Option<u8> {
        self.array::<1>().map(|[b]| b)
    }

    pub(crate) fn i8(&mut self) -> Option<i8> {
        self.array().map(i8::from_be_bytes)
    }

    pub(crate) fn u16(&mut self) -> Option<u16> {
        self.array().map(u16::from_be_bytes)
    }

    pub(crate) fn i16(&mut self) -> Option<i16> {
        self.array().map(i16::from_be_bytes)
    }

    pub(crate) fn u24(&mut self) -> Option<u32> {
        self.array::<3>()
            .map(|[a, b, c]| u32::from_be_bytes([0, a, b, c]))
    }

    pub(crate) fn u32(&mut self) -> Option<u32> {
        self.array().map(u32::from_be_bytes)
    }

    pub(crate) fn i32(&mut self) -> Option<i32> {
        self.array().map(i32::from_be_bytes)
    }
}

pub(crate) fn u8_at(data: &[u8], pos: usize) -> Option<u8> {
    data.get(pos).copied()
}

pub(crate) fn u16_at(data: &[u8], pos: usize) -> Option<u16> {
    Reader::at(data, pos)?.u16()
}

pub(crate) fn i16_at(data: &[u8], pos: usize) -> Option<i16> {
    Reader::at(data, pos)?.i16()
}

pub(crate) fn u24_at(data: &[u8], pos: usize) -> Option<u32> {
    Reader::at(data, pos)?.u24()
}

pub(crate) fn u32_at(data: &[u8], pos: usize) -> Option<u32> {
    Reader::at(data, pos)?.u32()
}

/// A byte range of the font data, checked against its length when the font is parsed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl Span {
    /// The span's bytes, or an empty slice if it does not fit (it always fits the data it was
    /// checked against).
    pub(crate) fn of<'a>(&self, data: &'a [u8]) -> &'a [u8] {
        data.get(self.start..self.end).unwrap_or(&[])
    }
}
