//! MD4（RFC 1320）。eD2k 用它给每个分块和整个文件算哈希；市面上的哈希库大多不再带 MD4，这里自己实现。

#[derive(Clone)]
pub struct Md4 {
    state: [u32; 4],
    buf: [u8; 64],
    buf_len: usize,
    total: u64,
}

impl Default for Md4 {
    fn default() -> Self {
        Self::new()
    }
}

#[inline]
fn f(x: u32, y: u32, z: u32) -> u32 {
    (x & y) | (!x & z)
}
#[inline]
fn g(x: u32, y: u32, z: u32) -> u32 {
    (x & y) | (x & z) | (y & z)
}
#[inline]
fn h(x: u32, y: u32, z: u32) -> u32 {
    x ^ y ^ z
}

impl Md4 {
    pub fn new() -> Self {
        Md4 { state: [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476], buf: [0; 64], buf_len: 0, total: 0 }
    }

    fn block(state: &mut [u32; 4], data: &[u8]) {
        let mut x = [0u32; 16];
        for (i, w) in x.iter_mut().enumerate() {
            *w = u32::from_le_bytes([data[i * 4], data[i * 4 + 1], data[i * 4 + 2], data[i * 4 + 3]]);
        }
        let [mut a, mut b, mut c, mut d] = *state;
        // 第 1 轮
        for i in (0..16).step_by(4) {
            a = a.wrapping_add(f(b, c, d)).wrapping_add(x[i]).rotate_left(3);
            d = d.wrapping_add(f(a, b, c)).wrapping_add(x[i + 1]).rotate_left(7);
            c = c.wrapping_add(f(d, a, b)).wrapping_add(x[i + 2]).rotate_left(11);
            b = b.wrapping_add(f(c, d, a)).wrapping_add(x[i + 3]).rotate_left(19);
        }
        // 第 2 轮
        for i in 0..4 {
            a = a.wrapping_add(g(b, c, d)).wrapping_add(x[i]).wrapping_add(0x5a82_7999).rotate_left(3);
            d = d.wrapping_add(g(a, b, c)).wrapping_add(x[i + 4]).wrapping_add(0x5a82_7999).rotate_left(5);
            c = c.wrapping_add(g(d, a, b)).wrapping_add(x[i + 8]).wrapping_add(0x5a82_7999).rotate_left(9);
            b = b.wrapping_add(g(c, d, a)).wrapping_add(x[i + 12]).wrapping_add(0x5a82_7999).rotate_left(13);
        }
        // 第 3 轮
        for &i in &[0usize, 2, 1, 3] {
            a = a.wrapping_add(h(b, c, d)).wrapping_add(x[i]).wrapping_add(0x6ed9_eba1).rotate_left(3);
            d = d.wrapping_add(h(a, b, c)).wrapping_add(x[i + 8]).wrapping_add(0x6ed9_eba1).rotate_left(9);
            c = c.wrapping_add(h(d, a, b)).wrapping_add(x[i + 4]).wrapping_add(0x6ed9_eba1).rotate_left(11);
            b = b.wrapping_add(h(c, d, a)).wrapping_add(x[i + 12]).wrapping_add(0x6ed9_eba1).rotate_left(15);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.total = self.total.wrapping_add(data.len() as u64);
        if self.buf_len > 0 {
            let take = (64 - self.buf_len).min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len == 64 {
                let b = self.buf;
                Self::block(&mut self.state, &b);
                self.buf_len = 0;
            }
        }
        while data.len() >= 64 {
            Self::block(&mut self.state, &data[..64]);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
    }

    pub fn finish(mut self) -> [u8; 16] {
        let bits = self.total.wrapping_mul(8);
        let mut pad = vec![0x80u8];
        let rem = (self.buf_len + 1) % 64;
        pad.resize(pad.len() + if rem <= 56 { 56 - rem } else { 120 - rem }, 0);
        pad.extend_from_slice(&bits.to_le_bytes());
        let total = self.total;
        self.update(&pad);
        debug_assert_eq!(self.buf_len, 0);
        self.total = total;
        let mut out = [0u8; 16];
        for (i, w) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        out
    }
}

pub fn md4(data: &[u8]) -> [u8; 16] {
    let mut m = Md4::new();
    m.update(data);
    m.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(d: [u8; 16]) -> String {
        d.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn rfc1320_vectors() {
        assert_eq!(hex(md4(b"")), "31d6cfe0d16ae931b73c59d7e0c089c0");
        assert_eq!(hex(md4(b"a")), "bde52cb31de33e46245e05fbdbd6fb24");
        assert_eq!(hex(md4(b"abc")), "a448017aaf21d8525fc10ae87aa6729d");
        assert_eq!(hex(md4(b"message digest")), "d9130a8164549fe818874806e1c7014b");
        assert_eq!(hex(md4(b"abcdefghijklmnopqrstuvwxyz")), "d79e1c308aa5bbcdeea8ed63df412da9");
        assert_eq!(hex(md4(b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789")), "043f8582f241db351ce627e153e7f0e4");
        assert_eq!(hex(md4(b"12345678901234567890123456789012345678901234567890123456789012345678901234567890")), "e33b4ddc9c38f2199c3e7b164fcc0536");
    }

    #[test]
    fn chunked_updates_match_one_shot() {
        let data: Vec<u8> = (0..10_000u32).map(|i| (i * 13 % 251) as u8).collect();
        let whole = md4(&data);
        for step in [1usize, 7, 63, 64, 65, 1000] {
            let mut m = Md4::new();
            for c in data.chunks(step) {
                m.update(c);
            }
            assert_eq!(m.finish(), whole, "step {step}");
        }
        // 刚好跨越填充边界的长度
        for n in [55usize, 56, 57, 63, 64, 119, 120] {
            let d = vec![b'x'; n];
            let mut m = Md4::new();
            m.update(&d[..n / 2]);
            m.update(&d[n / 2..]);
            assert_eq!(m.finish(), md4(&d), "len {n}");
        }
    }
}
