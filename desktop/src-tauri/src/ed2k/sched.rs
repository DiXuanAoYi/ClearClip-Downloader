//! 下载调度：把文件拆成分块（9.28 MB）和请求块（180 KB），按对方拥有的分块分配任务，
//! 记录每块的状态和每个分块的贡献者（分块校验失败时据此惩罚来源）。

use std::collections::BTreeSet;

use super::hash::{self, BLOCK_SIZE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    Todo,
    Inflight(u64),
    Done,
}

#[derive(Debug)]
struct Part {
    start: u64,
    end: u64,
    blocks: Vec<Block>,
    /// 全部数据块已到齐，正在或等待校验
    full: bool,
    verified: bool,
    contributors: BTreeSet<u64>,
}

/// 分给某个来源的一块数据：区间是 [start, end)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Claim {
    pub part: usize,
    pub block: usize,
    pub start: u64,
    pub end: u64,
}

#[derive(Debug)]
pub struct Sched {
    parts: Vec<Part>,
}

impl Sched {
    pub fn new(size: u64) -> Sched {
        let parts = (0..hash::data_parts(size))
            .map(|i| {
                let (start, end) = hash::part_range(size, i);
                let n = (end - start).div_ceil(BLOCK_SIZE) as usize;
                Part { start, end, blocks: vec![Block::Todo; n], full: false, verified: false, contributors: BTreeSet::new() }
            })
            .collect();
        Sched { parts }
    }

    pub fn part_count(&self) -> usize {
        self.parts.len()
    }

    /// 已校验通过的分块（续传时从记录里恢复）。
    pub fn mark_verified(&mut self, part: usize) {
        if let Some(p) = self.parts.get_mut(part) {
            p.blocks.fill(Block::Done);
            p.full = true;
            p.verified = true;
        }
    }

    pub fn verified_parts(&self) -> Vec<usize> {
        self.parts.iter().enumerate().filter(|(_, p)| p.verified).map(|(i, _)| i).collect()
    }

    pub fn is_complete(&self) -> bool {
        self.parts.iter().all(|p| p.verified)
    }

    /// 已经落盘的字节数（含尚未校验的完整数据块）。
    pub fn received(&self) -> u64 {
        self.parts
            .iter()
            .map(|p| {
                p.blocks
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| **b == Block::Done)
                    .map(|(i, _)| {
                        let s = p.start + i as u64 * BLOCK_SIZE;
                        (s + BLOCK_SIZE).min(p.end) - s
                    })
                    .sum::<u64>()
            })
            .sum()
    }

    /// 还有没有人领取的数据块（在对方拥有的分块里）。
    pub fn wants(&self, has: &[bool]) -> bool {
        self.parts.iter().enumerate().any(|(i, p)| !p.full && has.get(i).copied().unwrap_or(false) && p.blocks.contains(&Block::Todo))
    }

    /// 给来源 `peer` 分配最多 `max` 个数据块：优先补完已经开始的分块，这样分块能尽早校验。
    pub fn claim(&mut self, peer: u64, has: &[bool], max: usize) -> Vec<Claim> {
        let mut order: Vec<usize> = (0..self.parts.len()).filter(|i| has.get(*i).copied().unwrap_or(false) && !self.parts[*i].full).collect();
        order.sort_by_key(|i| {
            let p = &self.parts[*i];
            let todo = p.blocks.iter().filter(|b| **b == Block::Todo).count();
            let started = p.blocks.iter().any(|b| *b != Block::Todo);
            // 已开始的在前，其中剩得少的在前；没开始的按序号
            (!started, if started { todo } else { 0 }, *i)
        });
        let mut out = vec![];
        for i in order {
            let p = &mut self.parts[i];
            for b in 0..p.blocks.len() {
                if out.len() >= max {
                    return out;
                }
                if p.blocks[b] == Block::Todo {
                    p.blocks[b] = Block::Inflight(peer);
                    let start = p.start + b as u64 * BLOCK_SIZE;
                    out.push(Claim { part: i, block: b, start, end: (start + BLOCK_SIZE).min(p.end) });
                }
            }
        }
        out
    }

    /// 一块数据已写入磁盘。如果它让整个分块到齐，返回分块序号，调用方应去校验。
    pub fn complete(&mut self, peer: u64, c: Claim) -> Option<usize> {
        let p = self.parts.get_mut(c.part)?;
        p.blocks[c.block] = Block::Done;
        p.contributors.insert(peer);
        if !p.full && p.blocks.iter().all(|b| *b == Block::Done) {
            p.full = true;
            return Some(c.part);
        }
        None
    }

    /// 分块校验结果。通过：标记完成；失败：整块重来，返回贡献过数据的来源。
    pub fn verified(&mut self, part: usize, ok: bool) -> Vec<u64> {
        let Some(p) = self.parts.get_mut(part) else { return vec![] };
        if ok {
            p.verified = true;
            return vec![];
        }
        p.blocks.fill(Block::Todo);
        p.full = false;
        std::mem::take(&mut p.contributors).into_iter().collect()
    }

    /// 来源断开：它手里没收完的数据块放回待领取。
    pub fn release_peer(&mut self, peer: u64) {
        for p in &mut self.parts {
            for b in &mut p.blocks {
                if *b == Block::Inflight(peer) {
                    *b = Block::Todo;
                }
            }
        }
    }

    pub fn release(&mut self, c: Claim) {
        if let Some(b) = self.parts.get_mut(c.part).and_then(|p| p.blocks.get_mut(c.block)) {
            if matches!(*b, Block::Inflight(_)) {
                *b = Block::Todo;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ed2k::hash::PART_SIZE;

    fn all(n: usize) -> Vec<bool> {
        vec![true; n]
    }

    #[test]
    fn geometry_and_claims() {
        let size = PART_SIZE + 1_000_000;
        let mut s = Sched::new(size);
        assert_eq!(s.part_count(), 2);
        let c = s.claim(1, &all(2), 3);
        assert_eq!(c.len(), 3);
        assert_eq!((c[0].part, c[0].start, c[0].end), (0, 0, BLOCK_SIZE));
        assert_eq!(c[2].end, 3 * BLOCK_SIZE);
        // 另一个来源领到后面的块，不会重复
        let d = s.claim(2, &all(2), 3);
        assert_eq!(d[0].start, 3 * BLOCK_SIZE);
        // 最后一个分块的最后一块是短块
        let mut s = Sched::new(size);
        let mut last = None;
        loop {
            let c = s.claim(1, &[false, true], 3);
            if c.is_empty() {
                break;
            }
            last = c.last().copied();
            for x in c {
                s.complete(1, x);
            }
        }
        assert_eq!(last.unwrap().end, size);
    }

    #[test]
    fn respects_what_the_peer_has() {
        let mut s = Sched::new(PART_SIZE * 2);
        assert!(s.claim(1, &[false, false], 3).is_empty());
        let c = s.claim(1, &[false, true], 3);
        assert!(c.iter().all(|x| x.part == 1));
        assert!(!s.wants(&[false, false]));
        assert!(s.wants(&[true, false]));
    }

    #[test]
    fn finishes_parts_before_starting_new_ones() {
        let mut s = Sched::new(PART_SIZE * 2);
        let first = s.claim(1, &all(2), 1);
        assert_eq!(first[0].part, 0);
        // 来源 2 只有分块 1，来源 1 释放了块 0 之后，已开始的分块 0 优先
        s.release(first[0]);
        let again = s.claim(3, &all(2), 1);
        assert_eq!(again[0].part, 0);
    }

    #[test]
    fn part_completion_and_verification() {
        let mut s = Sched::new(BLOCK_SIZE * 2 + 5);
        let cs = s.claim(7, &all(1), 3);
        assert_eq!(cs.len(), 3);
        assert_eq!(s.complete(7, cs[0]), None);
        assert_eq!(s.complete(7, cs[1]), None);
        assert_eq!(s.received(), 2 * BLOCK_SIZE);
        assert_eq!(s.complete(7, cs[2]), Some(0));
        assert_eq!(s.received(), 2 * BLOCK_SIZE + 5);
        // 校验失败：整块重来，并报告贡献者
        assert_eq!(s.verified(0, false), vec![7]);
        assert_eq!(s.received(), 0);
        assert!(!s.is_complete());
        let again = s.claim(8, &all(1), 3);
        assert_eq!(again.len(), 3);
        for c in again {
            s.complete(8, c);
        }
        s.verified(0, true);
        assert!(s.is_complete());
        assert_eq!(s.verified_parts(), vec![0]);
    }

    #[test]
    fn releasing_a_peer_returns_its_blocks() {
        let mut s = Sched::new(BLOCK_SIZE * 4);
        let a = s.claim(1, &all(1), 3);
        s.complete(1, a[0]);
        s.release_peer(1);
        // 已完成的保留，没完成的回到待领取
        let b = s.claim(2, &all(1), 5);
        assert_eq!(b.len(), 3);
        assert_eq!(s.received(), BLOCK_SIZE);
    }

    #[test]
    fn restored_parts_are_complete() {
        let mut s = Sched::new(PART_SIZE * 2 + 10);
        s.mark_verified(0);
        s.mark_verified(1);
        assert_eq!(s.received(), PART_SIZE * 2);
        let c = s.claim(1, &all(3), 1);
        assert_eq!(c[0].part, 2);
    }
}
