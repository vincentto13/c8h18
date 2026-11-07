//! RNC ProPack method 1 decompressor (spec 01 §1.2).

pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xA001 } else { crc >> 1 };
        }
    }
    crc
}

pub fn is_rnc(data: &[u8]) -> bool {
    data.len() >= 18 && &data[0..3] == b"RNC"
}

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u32,
    count: u32,
}

impl<'a> Bits<'a> {
    fn u32le(&self, p: usize) -> u32 {
        let mut v = 0u32;
        for i in 0..4 {
            let b = *self.data.get(p + i).unwrap_or(&0) as u32;
            v |= b << (8 * i);
        }
        v
    }
    fn new(data: &'a [u8], pos: usize) -> Self {
        let mut b = Bits { data, pos, buf: 0, count: 16 };
        b.buf = b.u32le(pos);
        b
    }
    fn peek(&self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.buf & ((1u32 << n) - 1) }
    }
    fn advance(&mut self, n: u32) {
        self.buf = if n >= 32 { 0 } else { self.buf >> n };
        self.count -= n;
        if self.count < 16 {
            self.pos += 2;
            self.buf |= self.u32le(self.pos) << self.count;
            self.count += 16;
        }
    }
    fn read(&mut self, n: u32) -> u32 {
        let v = self.peek(n);
        self.advance(n);
        v
    }
    fn refill_after_literal(&mut self) {
        self.count -= 16;
        self.buf &= if self.count == 0 { 0 } else { (1u32 << self.count) - 1 };
        self.buf |= self.u32le(self.pos) << self.count;
        self.count += 16;
    }
}

struct Huff {
    // (length, reversed code, symbol)
    codes: Vec<(u32, u32, u32)>,
}

fn reverse(mut v: u32, n: u32) -> u32 {
    let mut r = 0;
    for _ in 0..n {
        r = (r << 1) | (v & 1);
        v >>= 1;
    }
    r
}

impl Huff {
    fn read(b: &mut Bits) -> Huff {
        let n = b.read(5);
        let mut lens = Vec::new();
        for _ in 0..n {
            lens.push(b.read(4));
        }
        let mut codes = Vec::new();
        let mut code: u32 = 0;
        for l in 1..=16u32 {
            for (j, &len) in lens.iter().enumerate() {
                if len == l {
                    codes.push((l, reverse(code, l), j as u32));
                    code += 1;
                }
            }
            code <<= 1;
        }
        Huff { codes }
    }
    fn value(&self, b: &mut Bits) -> Result<u32, String> {
        for &(l, c, s) in &self.codes {
            if b.peek(l) == c {
                b.advance(l);
                return Ok(if s < 2 { s } else { (1 << (s - 1)) | b.read(s - 1) });
            }
        }
        Err("rnc: bad huffman code".into())
    }
}

pub fn unpack(data: &[u8]) -> Result<Vec<u8>, String> {
    if !is_rnc(data) {
        return Err("not RNC".into());
    }
    if data[3] != 1 {
        return Err("rnc: unsupported method".into());
    }
    let be32 = |p: usize| u32::from_be_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]]);
    let unpacked_len = be32(4) as usize;
    let packed_len = be32(8) as usize;
    let crc_unpacked = u16::from_be_bytes([data[12], data[13]]);
    let crc_packed = u16::from_be_bytes([data[14], data[15]]);
    if 18 + packed_len > data.len() {
        return Err("rnc: truncated".into());
    }
    if crc16(&data[18..18 + packed_len]) != crc_packed {
        return Err("rnc: packed CRC mismatch".into());
    }
    let mut out: Vec<u8> = Vec::with_capacity(unpacked_len);
    let mut b = Bits::new(data, 18);
    b.advance(2);
    while out.len() < unpacked_len {
        let raw = Huff::read(&mut b);
        let dist = Huff::read(&mut b);
        let len = Huff::read(&mut b);
        let mut count = b.read(16) as u16;
        loop {
            let n = raw.value(&mut b)? as usize;
            if n > 0 {
                for i in 0..n {
                    out.push(*data.get(b.pos + i).unwrap_or(&0));
                }
                b.pos += n;
                b.refill_after_literal();
            }
            count = count.wrapping_sub(1);
            if count == 0 || out.len() >= unpacked_len {
                break;
            }
            let d = dist.value(&mut b)? as usize + 1;
            let m = len.value(&mut b)? as usize + 2;
            if d > out.len() {
                return Err("rnc: bad distance".into());
            }
            let start = out.len() - d;
            for i in 0..m {
                let v = out[start + i];
                out.push(v);
            }
        }
    }
    out.truncate(unpacked_len);
    if crc16(&out) != crc_unpacked {
        return Err("rnc: unpacked CRC mismatch".into());
    }
    Ok(out)
}

/// Unpack if RNC, else return as is.
pub fn maybe_unpack(data: Vec<u8>) -> Result<Vec<u8>, String> {
    if is_rnc(&data) { unpack(&data) } else { Ok(data) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn crc() {
        assert_eq!(super::crc16(b"123456789"), 0xBB3D);
    }
}
