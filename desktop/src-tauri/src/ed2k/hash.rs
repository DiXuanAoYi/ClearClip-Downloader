//! eD2k 的分块与哈希规则：文件按 9,728,000 字节分块，每块算 MD4；文件哈希是各块哈希拼接后再算 MD4
//! （只有一块时就是这块的哈希）。大小刚好是块大小整数倍的文件，按 eMule 的做法末尾多算一个空块。

use std::io::Read;

use super::md4::{md4, Md4};

pub type Hash = [u8; 16];

pub const PART_SIZE: u64 = 9_728_000;
/// 向对方请求数据时每次要的块大小（与 eMule 一致）
pub const BLOCK_SIZE: u64 = 184_320;

/// 文件的分块哈希个数（含大小刚好整除时末尾的空块）。
pub fn part_count(size: u64) -> u64 {
    size / PART_SIZE + 1
}

/// 实际包含数据的分块个数。
pub fn data_parts(size: u64) -> u64 {
    size.div_ceil(PART_SIZE)
}

/// 第 `idx` 个分块的字节范围 [起, 止)。
pub fn part_range(size: u64, idx: u64) -> (u64, u64) {
    let start = idx * PART_SIZE;
    (start.min(size), (start + PART_SIZE).min(size))
}

pub fn file_hash(parts: &[Hash]) -> Hash {
    match parts {
        [one] => *one,
        many => md4(&many.concat()),
    }
}

/// 读完整个文件，得到各分块哈希。
pub fn hash_parts<R: Read>(mut r: R, size: u64) -> std::io::Result<Vec<Hash>> {
    let mut out = Vec::with_capacity(part_count(size) as usize);
    let mut buf = vec![0u8; 256 * 1024];
    for idx in 0..part_count(size) {
        let (s, e) = part_range(size, idx);
        let mut left = e - s;
        let mut m = Md4::new();
        while left > 0 {
            let want = left.min(buf.len() as u64) as usize;
            r.read_exact(&mut buf[..want])?;
            m.update(&buf[..want]);
            left -= want as u64;
        }
        out.push(m.finish());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_geometry() {
        assert_eq!(part_count(0), 1);
        assert_eq!(part_count(1), 1);
        assert_eq!(part_count(PART_SIZE - 1), 1);
        assert_eq!(part_count(PART_SIZE), 2);
        assert_eq!(part_count(PART_SIZE + 1), 2);
        assert_eq!(data_parts(PART_SIZE), 1);
        assert_eq!(data_parts(PART_SIZE + 1), 2);
        assert_eq!(part_range(PART_SIZE + 10, 1), (PART_SIZE, PART_SIZE + 10));
        assert_eq!(part_range(PART_SIZE, 1), (PART_SIZE, PART_SIZE));
    }

    #[test]
    fn small_file_hash_is_plain_md4() {
        let data = b"hello ed2k".to_vec();
        let parts = hash_parts(&data[..], data.len() as u64).unwrap();
        assert_eq!(parts, vec![md4(&data)]);
        assert_eq!(file_hash(&parts), md4(&data));
    }

    #[test]
    fn multi_part_hash_combines_part_hashes() {
        let size = PART_SIZE + 1234;
        let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        let parts = hash_parts(&data[..], size).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], md4(&data[..PART_SIZE as usize]));
        assert_eq!(parts[1], md4(&data[PART_SIZE as usize..]));
        assert_eq!(file_hash(&parts), md4(&[parts[0], parts[1]].concat()));
    }

    #[test]
    fn exact_multiple_gets_an_empty_trailing_part() {
        let data = vec![7u8; PART_SIZE as usize];
        let parts = hash_parts(&data[..], PART_SIZE).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1], md4(b""));
    }
}
