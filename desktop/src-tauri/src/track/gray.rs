//! 灰度图和积分图：运动感知、模板追踪的基础。

/// 8 位灰度图。
#[derive(Clone, Debug)]
pub struct Gray {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Gray {
    pub fn new(w: usize, h: usize) -> Gray {
        Gray { w, h, px: vec![0; w * h] }
    }

    /// 从原始字节构造；长度对不上返回 None。
    pub fn from_raw(w: usize, h: usize, px: Vec<u8>) -> Option<Gray> {
        (w > 0 && h > 0 && px.len() == w * h).then_some(Gray { w, h, px })
    }

    #[inline]
    pub fn at(&self, x: usize, y: usize) -> u8 {
        self.px[y * self.w + x]
    }

    /// 长宽各缩小一半（2×2 取平均）。
    pub fn half(&self) -> Gray {
        let (w, h) = ((self.w / 2).max(1), (self.h / 2).max(1));
        let mut out = Gray::new(w, h);
        for y in 0..h {
            let (y0, y1) = ((y * 2).min(self.h - 1), (y * 2 + 1).min(self.h - 1));
            for x in 0..w {
                let (x0, x1) = ((x * 2).min(self.w - 1), (x * 2 + 1).min(self.w - 1));
                let s = u32::from(self.at(x0, y0)) + u32::from(self.at(x1, y0)) + u32::from(self.at(x0, y1)) + u32::from(self.at(x1, y1));
                out.px[y * w + x] = ((s + 2) / 4) as u8;
            }
        }
        out
    }

    /// 3×3 均值模糊（边缘取最近的像素）：压掉一点噪点再比较两帧。
    pub fn blur3(&self) -> Gray {
        let mut tmp = vec![0u16; self.w * self.h];
        for y in 0..self.h {
            for x in 0..self.w {
                let (xa, xb) = (x.saturating_sub(1), (x + 1).min(self.w - 1));
                tmp[y * self.w + x] = u16::from(self.at(xa, y)) + u16::from(self.at(x, y)) + u16::from(self.at(xb, y));
            }
        }
        let mut out = Gray::new(self.w, self.h);
        for y in 0..self.h {
            let (ya, yb) = (y.saturating_sub(1), (y + 1).min(self.h - 1));
            for x in 0..self.w {
                let s = u32::from(tmp[ya * self.w + x]) + u32::from(tmp[y * self.w + x]) + u32::from(tmp[yb * self.w + x]);
                out.px[y * self.w + x] = ((s + 4) / 9) as u8;
            }
        }
        out
    }
}

/// 金字塔：第 0 层是原图，每高一层长宽各缩小一半。
pub struct Pyramid {
    pub levels: Vec<Gray>,
}

impl Pyramid {
    pub fn new(base: &Gray, levels: usize) -> Pyramid {
        let mut v = vec![base.clone()];
        for _ in 0..levels {
            let next = v.last().expect("at least one level").half();
            v.push(next);
        }
        Pyramid { levels: v }
    }
}

/// 积分图：任意矩形内的像素和、平方和，用常数时间取得。
pub struct Integral {
    w: usize,
    sum: Vec<u64>,
    sq: Vec<u64>,
}

impl Integral {
    pub fn new(g: &Gray) -> Integral {
        let w1 = g.w + 1;
        let mut sum = vec![0u64; w1 * (g.h + 1)];
        let mut sq = vec![0u64; w1 * (g.h + 1)];
        for y in 0..g.h {
            let (mut rs, mut rq) = (0u64, 0u64);
            for x in 0..g.w {
                let v = u64::from(g.at(x, y));
                rs += v;
                rq += v * v;
                sum[(y + 1) * w1 + x + 1] = sum[y * w1 + x + 1] + rs;
                sq[(y + 1) * w1 + x + 1] = sq[y * w1 + x + 1] + rq;
            }
        }
        Integral { w: w1, sum, sq }
    }

    /// 矩形 `[x, x+w) × [y, y+h)` 的（像素和，平方和）。调用方保证矩形在图内。
    #[inline]
    pub fn rect(&self, x: usize, y: usize, w: usize, h: usize) -> (u64, u64) {
        let (a, b, c, d) = (y * self.w + x, y * self.w + x + w, (y + h) * self.w + x, (y + h) * self.w + x + w);
        (self.sum[d] + self.sum[a] - self.sum[b] - self.sum[c], self.sq[d] + self.sq[a] - self.sq[b] - self.sq[c])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(w: usize, h: usize) -> Gray {
        Gray { w, h, px: (0..w * h).map(|i| (i % 251) as u8).collect() }
    }

    #[test]
    fn integral_rect_sums_match_brute_force() {
        let g = ramp(23, 17);
        let ii = Integral::new(&g);
        for &(x, y, w, h) in &[(0, 0, 23, 17), (3, 2, 7, 9), (22, 16, 1, 1), (5, 5, 1, 12)] {
            let (mut s, mut q) = (0u64, 0u64);
            for yy in y..y + h {
                for xx in x..x + w {
                    let v = u64::from(g.at(xx, yy));
                    s += v;
                    q += v * v;
                }
            }
            assert_eq!(ii.rect(x, y, w, h), (s, q), "{x},{y},{w},{h}");
        }
    }

    #[test]
    fn half_averages_2x2_blocks_and_keeps_odd_edges() {
        let g = Gray { w: 3, h: 2, px: vec![10, 20, 90, 30, 40, 70] };
        let h = g.half();
        assert_eq!((h.w, h.h), (1, 1));
        assert_eq!(h.px, vec![25]);
        let p = Pyramid::new(&ramp(64, 36), 2);
        assert_eq!((p.levels[1].w, p.levels[1].h, p.levels[2].w, p.levels[2].h), (32, 18, 16, 9));
    }

    #[test]
    fn blur3_keeps_flat_areas_and_spreads_a_spike() {
        let mut g = Gray { w: 5, h: 5, px: vec![50; 25] };
        assert_eq!(g.blur3().px, vec![50; 25]);
        g.px[12] = 140;
        let b = g.blur3();
        assert_eq!(b.at(2, 2), 60);
        assert_eq!(b.at(1, 1), 60);
        assert_eq!(b.at(0, 0), 50);
    }

    #[test]
    fn from_raw_rejects_wrong_lengths() {
        assert!(Gray::from_raw(4, 4, vec![0; 15]).is_none());
        assert!(Gray::from_raw(0, 4, vec![]).is_none());
        assert!(Gray::from_raw(4, 4, vec![0; 16]).is_some());
    }
}
