use std::io::{self, Read};

const MAGIC_1: u8 = 0x1f;
const MAGIC_2: u8 = 0x9d;
const INIT_BITS: u8 = 9;
const CLEAR: u32 = 256;
const FIRST_FREE: u32 = 257;
const MAX_BITS: u8 = 16;
const INPUT_BUFFER_SIZE: usize = 8192;

struct Input<R> {
    reader: R,
    buffer: [u8; INPUT_BUFFER_SIZE],
    position: usize,
    filled: usize,
}

impl<R: Read> Input<R> {
    fn new(reader: R) -> Self {
        Self {
            reader,
            buffer: [0; INPUT_BUFFER_SIZE],
            position: 0,
            filled: 0,
        }
    }

    fn next_byte(&mut self) -> io::Result<Option<u8>> {
        if self.position == self.filled {
            self.filled = self.reader.read(&mut self.buffer)?;
            self.position = 0;
            if self.filled == 0 {
                return Ok(None);
            }
        }
        let byte = self.buffer[self.position];
        self.position += 1;
        Ok(Some(byte))
    }
}

/// A bounded-memory reader for the Unix `compress` (`.Z`) format.
///
/// The format changes code width at byte-group boundaries, and block-mode
/// streams align after a CLEAR relative to the previous width transition.
/// Keeping the bit position and alignment origin explicitly avoids the
/// desynchronisation that occurs when a reset follows a width change.
pub(crate) struct UnixCompressReader<R> {
    input: Input<R>,
    header_read: bool,
    block_mode: bool,
    maxbits: u8,
    prefix: Vec<u16>,
    suffix: Vec<u8>,
    free_ent: u32,
    nbits: u8,
    maxcode: u32,
    bit_acc: u64,
    bit_count: u8,
    bit_pos: u64,
    alignment_origin: u64,
    old_code: Option<u32>,
    finchar: u8,
    stack: Vec<u8>,
    pending: Vec<u8>,
    pending_pos: usize,
    finished: bool,
}

impl<R: Read> UnixCompressReader<R> {
    pub(crate) fn new(reader: R) -> Self {
        let dictionary_size = 1usize << MAX_BITS;
        Self {
            input: Input::new(reader),
            header_read: false,
            block_mode: true,
            maxbits: MAX_BITS,
            prefix: vec![0; dictionary_size],
            suffix: vec![0; dictionary_size],
            free_ent: FIRST_FREE,
            nbits: INIT_BITS,
            maxcode: (1u32 << INIT_BITS) - 1,
            bit_acc: 0,
            bit_count: 0,
            bit_pos: 0,
            alignment_origin: 0,
            old_code: None,
            finchar: 0,
            stack: Vec::new(),
            pending: Vec::new(),
            pending_pos: 0,
            finished: false,
        }
    }

    fn invalid(message: &'static str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, message)
    }

    fn read_header(&mut self) -> io::Result<()> {
        if self.header_read {
            return Ok(());
        }
        if self.input.next_byte()? != Some(MAGIC_1) || self.input.next_byte()? != Some(MAGIC_2) {
            return Err(Self::invalid("invalid Unix compress header"));
        }
        let flags = self
            .input
            .next_byte()?
            .ok_or_else(|| Self::invalid("truncated Unix compress header"))?;
        let maxbits = flags & 0x1f;
        if !(INIT_BITS..=MAX_BITS).contains(&maxbits) {
            return Err(Self::invalid("unsupported Unix compress code width"));
        }
        self.block_mode = flags & 0x80 != 0;
        self.maxbits = maxbits;
        self.free_ent = if self.block_mode { FIRST_FREE } else { 256 };
        self.header_read = true;
        Ok(())
    }

    fn read_code(&mut self) -> io::Result<Option<u32>> {
        while self.bit_count < self.nbits {
            let Some(byte) = self.input.next_byte()? else {
                return Ok(None);
            };
            self.bit_acc |= u64::from(byte) << self.bit_count;
            self.bit_count += 8;
        }
        let mask = (1u64 << self.nbits) - 1;
        let code = (self.bit_acc & mask) as u32;
        self.bit_acc >>= self.nbits;
        self.bit_count -= self.nbits;
        self.bit_pos += u64::from(self.nbits);
        Ok(Some(code))
    }

    fn discard_to(&mut self, target: u64) -> io::Result<bool> {
        debug_assert!(target >= self.bit_pos);
        while self.bit_pos < target {
            if self.bit_count == 0 {
                let Some(byte) = self.input.next_byte()? else {
                    return Ok(false);
                };
                self.bit_acc |= u64::from(byte) << self.bit_count;
                self.bit_count += 8;
            }
            let count = (target - self.bit_pos).min(u64::from(self.bit_count)) as u8;
            self.bit_acc >>= count;
            self.bit_count -= count;
            self.bit_pos += u64::from(count);
        }
        Ok(true)
    }

    fn align_to_group(&mut self) -> io::Result<bool> {
        debug_assert!(self.bit_pos > 0);
        let group_bits = u64::from(self.nbits) * 8;
        let previous_bit = self.bit_pos - 1;
        let relative = previous_bit - self.alignment_origin;
        let padding = group_bits - (relative + group_bits) % group_bits;
        let target = previous_bit + padding;
        if !self.discard_to(target)? {
            return Ok(false);
        }
        self.alignment_origin = target;
        Ok(true)
    }

    fn reset_dictionary(&mut self) {
        self.free_ent = if self.block_mode { FIRST_FREE } else { 256 };
        self.nbits = INIT_BITS;
        self.maxcode = (1u32 << INIT_BITS) - 1;
        self.old_code = None;
    }

    fn decode_string(&mut self, mut code: u32) -> io::Result<()> {
        self.stack.clear();
        if code == self.free_ent {
            self.stack.push(self.finchar);
            code = self
                .old_code
                .ok_or_else(|| Self::invalid("invalid Unix compress dictionary code"))?;
        }
        if code >= (1u32 << self.maxbits) {
            return Err(Self::invalid("invalid Unix compress dictionary code"));
        }
        while code >= 256 {
            self.stack.push(self.suffix[code as usize]);
            code = u32::from(self.prefix[code as usize]);
        }
        self.finchar = code as u8;
        self.stack.push(self.finchar);
        while let Some(byte) = self.stack.pop() {
            self.pending.push(byte);
        }
        Ok(())
    }

    /// Decode one code or reach the end of the compressed input.
    fn decode_one(&mut self) -> io::Result<()> {
        self.read_header()?;

        let Some(code) = self.read_code()? else {
            self.finished = true;
            return Ok(());
        };

        if self.old_code.is_none() {
            if code >= 256 {
                return Err(Self::invalid("invalid first Unix compress code"));
            }
            self.finchar = code as u8;
            self.pending.push(self.finchar);
            self.old_code = Some(code);
            return Ok(());
        }

        if self.block_mode && code == CLEAR {
            if !self.align_to_group()? {
                self.finished = true;
                return Ok(());
            }
            self.reset_dictionary();
            return Ok(());
        }

        let old_code = self.old_code.expect("checked above");
        if code > self.free_ent || code >= (1u32 << self.maxbits) {
            return Err(Self::invalid("invalid Unix compress dictionary code"));
        }
        self.decode_string(code)?;

        if self.free_ent < (1u32 << self.maxbits) {
            let index = self.free_ent as usize;
            self.prefix[index] = old_code as u16;
            self.suffix[index] = self.finchar;
            self.free_ent += 1;
            if self.free_ent > self.maxcode && self.nbits < self.maxbits {
                if !self.align_to_group()? {
                    self.finished = true;
                    return Ok(());
                }
                self.nbits += 1;
                self.maxcode = if self.nbits == self.maxbits {
                    1u32 << self.maxbits
                } else {
                    (1u32 << self.nbits) - 1
                };
            }
        }
        self.old_code = Some(code);
        Ok(())
    }
}

impl<R: Read> Read for UnixCompressReader<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        loop {
            if self.pending_pos < self.pending.len() {
                let count = (self.pending.len() - self.pending_pos).min(output.len());
                output[..count]
                    .copy_from_slice(&self.pending[self.pending_pos..self.pending_pos + count]);
                self.pending_pos += count;
                if self.pending_pos == self.pending.len() {
                    self.pending.clear();
                    self.pending_pos = 0;
                }
                return Ok(count);
            }
            if self.finished {
                return Ok(0);
            }
            self.decode_one()?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::UnixCompressReader;
    use std::io::{self, Read};

    fn append_code(data: &mut Vec<u8>, bit_pos: &mut usize, code: u32, width: usize) {
        while data.len() * 8 < *bit_pos + width {
            data.push(0);
        }
        for bit in 0..width {
            if code & (1 << bit) != 0 {
                data[(*bit_pos + bit) / 8] |= 1 << ((*bit_pos + bit) % 8);
            }
        }
        *bit_pos += width;
    }

    fn align(bit_pos: &mut usize, origin: &mut usize, width: usize) {
        let previous_bit = *bit_pos - 1;
        let group_bits = width * 8;
        let relative = previous_bit - *origin;
        let padding = group_bits - (relative + group_bits) % group_bits;
        *bit_pos = previous_bit + padding;
        *origin = *bit_pos;
    }

    struct OneByteReader {
        data: Vec<u8>,
        position: usize,
    }

    impl Read for OneByteReader {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            if self.position == self.data.len() {
                return Ok(0);
            }
            output[0] = self.data[self.position];
            self.position += 1;
            Ok(1)
        }
    }

    #[test]
    fn streams_across_width_change_and_dictionary_reset() {
        let mut data = vec![0x1f, 0x9d, 0x90];
        let mut body = Vec::new();
        let mut bit_pos = 0;
        let mut origin = 0;

        for code in 0..=255 {
            append_code(&mut body, &mut bit_pos, code, 9);
        }
        align(&mut bit_pos, &mut origin, 9);
        for code in 10..15 {
            append_code(&mut body, &mut bit_pos, code, 10);
        }
        append_code(&mut body, &mut bit_pos, 256, 10);
        align(&mut bit_pos, &mut origin, 10);
        for code in 99..102 {
            append_code(&mut body, &mut bit_pos, code, 9);
        }
        data.extend(body);

        let compressed_len = data.len();
        let mut reader = UnixCompressReader::new(OneByteReader { data, position: 0 });
        let mut first = [0; 1];
        assert_eq!(reader.read(&mut first).unwrap(), 1);
        assert_eq!(first[0], 0);
        assert!(reader.input.reader.position < compressed_len);

        let mut output = first.to_vec();
        reader.read_to_end(&mut output).unwrap();
        let mut expected: Vec<u8> = (0..=255).collect();
        expected.extend(10..15);
        expected.extend(99..102);
        assert_eq!(output, expected);
    }
}
