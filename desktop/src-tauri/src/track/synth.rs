//! 测试用的合成画面：有纹理的静态背景 + 一个有纹理的圆形物体沿给定路径运动，可以加镜头平移、遮挡和噪点。
//! 追踪和运动感知的单元测试、端到端测试（编码成真实视频）都用它。

use super::gray::Gray;

fn hash(x: i64, y: i64, seed: u64) -> u32 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B1).wrapping_add((y as u64).wrapping_mul(0x85EB_CA6B)).wrapping_add(seed.wrapping_mul(0xC2B2_AE35));
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0xFFFF) as u32
}

/// 平滑的值噪声（0–1），`scale` 越大纹理越粗。
fn value_noise(x: f64, y: f64, scale: f64, seed: u64) -> f64 {
    let (fx, fy) = (x / scale, y / scale);
    let (x0, y0) = (fx.floor() as i64, fy.floor() as i64);
    let (tx, ty) = (fx - x0 as f64, fy - y0 as f64);
    let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let v = |dx: i64, dy: i64| f64::from(hash(x0 + dx, y0 + dy, seed)) / 65535.0;
    let top = v(0, 0) * (1.0 - sx) + v(1, 0) * sx;
    let bot = v(0, 1) * (1.0 - sx) + v(1, 1) * sx;
    top * (1.0 - sy) + bot * sy
}

pub struct Scene {
    pub w: usize,
    pub h: usize,
    /// 背景比画面每边大 `PAD` 像素，镜头平移时从里面取
    bg: Vec<u8>,
    bg_w: usize,
    /// 物体的直径
    pub size: usize,
}

pub const PAD: usize = 24;

pub struct Shot {
    /// 物体中心（像素，画面坐标）
    pub cx: f64,
    pub cy: f64,
    /// 镜头平移（像素）：背景和物体一起往反方向移动
    pub cam: (i32, i32),
    /// 遮挡物（左上角 x, y, 宽, 高）：画一块纯灰色盖住
    pub wall: Option<(usize, usize, usize, usize)>,
    /// 噪点幅度（0 = 没有）和随机种子
    pub noise: f64,
    pub seed: u64,
    /// 没有物体（只有背景）
    pub no_object: bool,
}

impl Shot {
    pub fn at(cx: f64, cy: f64) -> Shot {
        Shot { cx, cy, cam: (0, 0), wall: None, noise: 0.0, seed: 0, no_object: false }
    }
}

impl Scene {
    pub fn new(w: usize, h: usize, size: usize) -> Scene {
        let bg_w = w + 2 * PAD;
        let bg_h = h + 2 * PAD;
        let mut bg = vec![0u8; bg_w * bg_h];
        for y in 0..bg_h {
            for x in 0..bg_w {
                let (fx, fy) = (x as f64, y as f64);
                let v = 70.0 + 90.0 * value_noise(fx, fy, 14.0, 1) + 50.0 * value_noise(fx, fy, 5.0, 2) - 25.0;
                bg[y * bg_w + x] = v.clamp(0.0, 255.0) as u8;
            }
        }
        Scene { w, h, bg, bg_w, size }
    }

    fn object_px(&self, dx: f64, dy: f64) -> Option<u8> {
        let r = self.size as f64 / 2.0;
        if dx * dx + dy * dy > r * r {
            return None;
        }
        // 和背景明显不同的纹理：粗条纹 + 细纹
        let v = 120.0 + 100.0 * (value_noise(dx + 500.0, dy + 500.0, 3.0, 7) - 0.5) * 2.0 + 35.0 * (((dx + r) / 4.0).floor() % 2.0 - 0.5) * 2.0;
        Some(v.clamp(0.0, 255.0) as u8)
    }

    pub fn frame(&self, s: &Shot) -> Gray {
        let mut g = Gray::new(self.w, self.h);
        for y in 0..self.h {
            for x in 0..self.w {
                let bx = (x as i64 + PAD as i64 + i64::from(s.cam.0)).clamp(0, self.bg_w as i64 - 1) as usize;
                let by = (y as i64 + PAD as i64 + i64::from(s.cam.1)).clamp(0, (self.h + 2 * PAD) as i64 - 1) as usize;
                let mut v = self.bg[by * self.bg_w + bx];
                if !s.no_object {
                    // 物体跟着镜头一起移动：画面里的位置 = 世界位置 − 镜头
                    let (dx, dy) = (x as f64 + 0.5 - s.cx, y as f64 + 0.5 - s.cy);
                    if let Some(o) = self.object_px(dx, dy) {
                        v = o;
                    }
                }
                if let Some((wx, wy, ww, wh)) = s.wall {
                    if x >= wx && x < wx + ww && y >= wy && y < wy + wh {
                        v = 128;
                    }
                }
                if s.noise > 0.0 {
                    let n = (f64::from(hash(x as i64, y as i64, s.seed + 99)) / 65535.0 - 0.5) * 2.0 * s.noise;
                    v = (f64::from(v) + n).clamp(0.0, 255.0) as u8;
                }
                g.px[y * self.w + x] = v;
            }
        }
        g
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenes_are_deterministic_and_textured() {
        let sc = Scene::new(96, 54, 14);
        let a = sc.frame(&Shot::at(30.0, 20.0));
        let b = sc.frame(&Shot::at(30.0, 20.0));
        assert_eq!(a.px, b.px);
        let moved = sc.frame(&Shot::at(60.0, 20.0));
        assert_ne!(a.px, moved.px);
        let mean = a.px.iter().map(|&v| f64::from(v)).sum::<f64>() / a.px.len() as f64;
        let var = a.px.iter().map(|&v| (f64::from(v) - mean).powi(2)).sum::<f64>() / a.px.len() as f64;
        assert!(var.sqrt() > 15.0, "背景要有足够的纹理：{}", var.sqrt());
    }
}
