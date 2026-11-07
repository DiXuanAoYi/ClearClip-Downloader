//! 内置的“软件术语库”起步包：屏幕翻译某个软件时，只带上这个软件用得到的术语，译法统一、提示词也更短。
//! 术语取自这些软件官方中文界面常见的叫法，只是起点——软件版本不同叫法会有出入，装好后可以在术语库里随意改。
//! 生效范围写 `app:软件名`（`glossary::app_key` 的写法），只在屏幕翻译这个软件时生效。

use crate::glossary::{app_key, Glossary, Term};

pub struct Pack {
    pub id: &'static str,
    pub name: &'static str,
    /// 这个软件的进程名（可能有多个叫法）
    pub exes: &'static [&'static str],
    /// 每行 `原文|译文`
    pub terms: &'static str,
}

pub const PACKS: &[Pack] = &[
    Pack {
        id: "photoshop",
        name: "Adobe Photoshop",
        exes: &["Photoshop.exe"],
        terms: "Layer|图层\nLayers|图层\nBackground|背景\nLayer Mask|图层蒙版\nMask|蒙版\nClipping Mask|剪贴蒙版\nAdjustment Layer|调整图层\nSmart Object|智能对象\nSmart Filter|智能滤镜\nFilter|滤镜\nBlending Mode|混合模式\nOpacity|不透明度\nFill|填充\nFlow|流量\nFeather|羽化\nSelection|选区\nSelect|选择\nMarquee|选框\nLasso|套索\nMagic Wand|魔棒\nQuick Selection|快速选择\nCrop|裁剪\nBrush|画笔\nEraser|橡皮擦\nClone Stamp|仿制图章\nHealing Brush|修复画笔\nSpot Healing Brush|污点修复画笔\nGradient|渐变\nPaint Bucket|油漆桶\nCurves|曲线\nLevels|色阶\nHue/Saturation|色相/饱和度\nBrightness/Contrast|亮度/对比度\nColor Balance|色彩平衡\nChannels|通道\nPaths|路径\nHistory|历史记录\nSwatches|色板\nArtboard|画板\nCanvas Size|画布大小\nImage Size|图像大小\nRasterize|栅格化\nLayer Style|图层样式\nDrop Shadow|投影\nStroke|描边\nFree Transform|自由变换\nContent-Aware Fill|内容识别填充\nGenerative Fill|生成式填充\nLiquify|液化\nExport As|导出为\nSave for Web|存储为 Web 所用格式\nActions|动作\nProperties|属性\nCharacter|字符\nParagraph|段落",
    },
    Pack {
        id: "premiere",
        name: "Adobe Premiere Pro",
        exes: &["Adobe Premiere Pro.exe"],
        terms: "Sequence|序列\nTimeline|时间轴\nProject|项目\nBin|素材箱\nProgram Monitor|节目监视器\nSource Monitor|源监视器\nEffect Controls|效果控件\nEffects|效果\nEssential Graphics|基本图形\nEssential Sound|基本声音\nLumetri Color|Lumetri 颜色\nAudio Track Mixer|音轨混合器\nKeyframe|关键帧\nRipple Delete|波纹删除\nRipple Edit|波纹编辑\nRolling Edit|滚动编辑\nRazor Tool|剃刀工具\nSelection Tool|选择工具\nSlip Tool|外滑工具\nSlide Tool|内滑工具\nRate Stretch Tool|比率拉伸工具\nPen Tool|钢笔工具\nType Tool|文字工具\nMarker|标记\nIn Point|入点\nOut Point|出点\nLift|提升\nExtract|提取\nInsert|插入\nOverwrite|覆盖\nVideo Transitions|视频过渡\nCross Dissolve|交叉溶解\nDip to Black|黑场过渡\nProxy|代理\nExport|导出\nScratch Disks|暂存盘\nNest|嵌套\nMulti-Camera|多机位\nTime Remapping|时间重映射\nOpacity|不透明度\nBlend Mode|混合模式\nWarp Stabilizer|变形稳定器\nAuto Reframe|自动重构",
    },
    Pack {
        id: "aftereffects",
        name: "Adobe After Effects",
        exes: &["AfterFX.exe"],
        terms: "Composition|合成\nPrecompose|预合成\nLayer|图层\nSolid|纯色\nNull Object|空对象\nAdjustment Layer|调整图层\nShape Layer|形状图层\nText Layer|文本图层\nCamera|摄像机\nLight|灯光\nMask|蒙版\nTrack Matte|轨道遮罩\nAlpha Matte|Alpha 遮罩\nKeyframe|关键帧\nExpression|表达式\nGraph Editor|图表编辑器\nRender Queue|渲染队列\nPreview|预览\nRAM Preview|内存预览\nParent & Link|父级和链接\nTransform|变换\nAnchor Point|锚点\nPosition|位置\nScale|缩放\nRotation|旋转\nOpacity|不透明度\nEffect|效果\nEffects & Presets|效果和预设\nMotion Blur|运动模糊\nTime Remapping|时间重映射\nWork Area|工作区\nTracker|跟踪器",
    },
    Pack {
        id: "blender",
        name: "Blender",
        exes: &["blender.exe"],
        terms: "Object Mode|物体模式\nEdit Mode|编辑模式\nSculpt Mode|雕刻模式\nPose Mode|姿态模式\nVertex|顶点\nVertices|顶点\nEdge|边\nFace|面\nMesh|网格\nModifier|修改器\nMaterial|材质\nTexture|纹理\nShader|着色器\nNode|节点\nRender|渲染\nViewport|视图\nOutliner|大纲视图\nProperties|属性\nTimeline|时间线\nKeyframe|关键帧\nArmature|骨架\nBone|骨骼\nRigging|绑定\nUV Editing|UV 编辑\nUnwrap|展开\nCollection|集合\nCamera|摄像机\nLight|灯光\nSubdivision Surface|细分曲面\nMirror|镜像\nArray|阵列\nBevel|倒角\nExtrude|挤出\nInset|内插\nLoop Cut|环切\nBoolean|布尔\nGeometry Nodes|几何节点\nParticle System|粒子系统\nShape Keys|形态键\nOrigin|原点\nSnapping|吸附\nProportional Editing|衰减编辑\nPivot Point|变换中心",
    },
    Pack {
        id: "resolve",
        name: "DaVinci Resolve",
        exes: &["Resolve.exe"],
        terms: "Media Pool|媒体池\nTimeline|时间线\nInspector|检查器\nEdit|剪辑\nColor|调色\nDeliver|交付\nNode|节点\nSerial Node|串行节点\nParallel Node|并行节点\nQualifier|限定器\nScopes|示波器\nCurves|曲线\nPrimaries|一级校色\nLift|暗部\nGamma|中灰\nGain|亮部\nOffset|偏移\nColor Wheels|色轮\nRender Queue|渲染队列\nProxy|代理\nOptimized Media|优化媒体\nSmart Bin|智能媒体夹\nBin|媒体夹\nMarker|标记\nTrim|修剪\nRipple|波纹\nBlade|刀片\nCompound Clip|复合片段\nNested Timeline|嵌套时间线\nKeyframe|关键帧\nTransition|转场\nGenerator|生成器\nTitles|标题",
    },
];

fn terms_of(p: &Pack) -> Vec<Term> {
    p.terms
        .lines()
        .filter_map(|l| {
            let (s, t) = l.split_once('|')?;
            let (s, t) = (s.trim(), t.trim());
            (!s.is_empty()).then(|| Term { source: s.into(), target: t.into(), note: String::new() })
        })
        .collect()
}

/// 这个软件的术语库在 `matches` 里的写法。
pub fn matches_of(p: &Pack) -> Vec<String> {
    let mut v: Vec<String> = p.exes.iter().map(|e| format!("app:{}", app_key(e))).collect();
    v.dedup();
    v
}

pub fn pack(id: &str) -> Option<&'static Pack> {
    PACKS.iter().find(|p| p.id == id)
}

/// 内置包对应的术语库（装进术语库时用）。`remote_id` 记 `app-pack:编号`，用来判断装过没有。
pub fn glossary_of(p: &Pack, now: i64) -> Glossary {
    Glossary {
        id: crate::glossary::new_id(),
        name: format!("{} 界面术语", p.name),
        description: "内置起步术语，只在屏幕翻译这个软件时生效。软件版本不同叫法会有出入，可以直接修改。".into(),
        enabled: true,
        origin: "community".into(),
        remote_id: format!("app-pack:{}", p.id),
        matches: matches_of(p),
        lang: "zh-CN".into(),
        terms: terms_of(p),
        updated: now,
        folder: "软件界面".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glossary::{select_terms, site_matches};

    #[test]
    fn every_pack_parses_and_has_unique_sources() {
        let mut ids = std::collections::HashSet::new();
        for p in PACKS {
            assert!(ids.insert(p.id), "编号重复：{}", p.id);
            let t = terms_of(p);
            assert!(t.len() >= 25, "{} 术语太少", p.id);
            assert!(t.iter().all(|x| !x.source.is_empty() && !x.target.is_empty()), "{} 有空的原文或译文", p.id);
            let mut seen = std::collections::HashSet::new();
            for x in &t {
                assert!(seen.insert(x.source.to_lowercase()), "{} 里 {} 重复", p.id, x.source);
            }
            assert!(!matches_of(p).is_empty());
        }
    }

    #[test]
    fn a_pack_only_applies_to_its_own_app() {
        let g = glossary_of(pack("photoshop").unwrap(), 1);
        assert_eq!(g.matches, vec!["app:photoshop".to_string()]);
        assert!(site_matches("app:photoshop", "app://photoshop"));
        assert!(site_matches("photoshop.exe", "app://photoshop"));
        assert!(!site_matches("app:photoshop", "app://blender"));
        assert!(!site_matches("app:photoshop", "https://photoshop.com/"));
        // 软件名里有空格：Adobe Premiere Pro.exe
        let pr = glossary_of(pack("premiere").unwrap(), 1);
        assert_eq!(pr.matches, vec!["app:adobe-premiere-pro".to_string()]);
        assert!(site_matches(&pr.matches[0], &crate::glossary::app_url("Adobe Premiere Pro.exe")));
        // 选术语：在 Photoshop 里取得到，在 Blender 里取不到
        let all = vec![g];
        assert!(select_terms(&all, &crate::glossary::app_url("Photoshop.exe"), "zh").iter().any(|t| t.source == "Layer"));
        assert!(select_terms(&all, &crate::glossary::app_url("blender.exe"), "zh").is_empty());
        // 目标语言不是中文时不带
        assert!(select_terms(&all, &crate::glossary::app_url("Photoshop.exe"), "en").is_empty());
    }

    #[test]
    fn app_keys_are_forgiving() {
        assert_eq!(app_key("C:\\Program Files\\Adobe\\Photoshop.exe"), "photoshop");
        assert_eq!(app_key("app://Photoshop"), "photoshop");
        assert_eq!(app_key("APP:photoshop.EXE"), "photoshop");
        assert_eq!(crate::glossary::app_url(""), "");
    }
}
