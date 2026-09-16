//! 真文本管线(C4,ADR-0017 兑付):fontique 字体发现 + swash 整形与
//! 轮廓缩放。CPU 导出用**真字形**替换占位条;画布文本继续由 vb_app 的
//! egui 覆盖层渲染(其自身已渲染真文本,见 ADR-0017)。
//!
//! v0.3 已知限制(诚实清单):
//! - 单字体运行:整个文本节点用一个字体覆盖(混合文字取第一个可覆盖
//!   全部的字体;覆盖不了出 .notdef)。逐字符回退留后续。
//! - LTR 方向;RTL 文本按 LTR 输出(落位由浏览器导出路径兜底)。
//! - 换行:v0.1 文本节点不自动换行(单行),与画布近似行为一致。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use swash::shape::Direction;
use swash::{shape, text::Script, FontRef, GlyphId};

use vb_common::geom::{BezPath, Point};

/// 项目 webfont 注册表(@font-face):家庭(小写)→ (字重 → 字体文件字节)。
type FontRegistry = HashMap<String, Vec<(u16, Arc<Vec<u8>>)>>;

fn font_registry() -> &'static Mutex<FontRegistry> {
    static INIT: OnceLock<Mutex<FontRegistry>> = OnceLock::new();
    INIT.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 注册项目内字体文件(@font-face 导入侧调用;懒读盘)。
pub fn register_font_file(family: &str, weight: u16, path: PathBuf) {
    let key = family.trim().to_ascii_lowercase();
    if let Ok(mut reg) = font_registry().lock() {
        let slot = reg.entry(key).or_default();
        if !slot.iter().any(|(w, _)| *w == weight) {
            slot.push((weight, Arc::new(std::fs::read(&path).unwrap_or_default())));
            slot.sort_by_key(|(w, _)| *w);
        }
    }
}

/// 清空注册表(换项目导入时)。
pub fn clear_font_registry() {
    if let Ok(mut reg) = font_registry().lock() {
        reg.clear();
    }
}

/// 按家庭+字重取注册字体(CSS 字重匹配:就近,先高后低)。
fn registry_font(family: &str, weight: u16) -> Option<(Arc<Vec<u8>>, usize)> {
    let key = family.trim().to_ascii_lowercase();
    let reg = font_registry().lock().ok()?;
    let faces = reg.get(&key)?;
    if faces.is_empty() {
        return None;
    }
    let pick = faces
        .iter()
        .min_by_key(|(w, _)| {
            // CSS 5.2 简化:就近字重(差值最小;同差取较小字重)
            let d = (*w as i32 - weight as i32).abs();
            (d, *w)
        })
        .map(|(_, bytes)| bytes)?;
    if pick.is_empty() {
        return None;
    }
    Some((pick.clone(), 0))
}

/// 一个字形(位置为**文本起点相对**坐标,Y 向下)。
#[derive(Debug, Clone)]
pub struct ShapedGlyph {
    pub id: u16,
    pub x: f32,
    pub y: f32,
    pub advance: f32,
}

/// 一个字形运行(单一字体)。
#[derive(Debug, Clone)]
pub struct ShapedRun {
    pub font_data: Arc<Vec<u8>>,
    pub font_index: usize,
    pub glyphs: Vec<ShapedGlyph>,
    pub ascent: f32,
    pub descent: f32,
}

/// 字体源缓存:fontique 枚举有成本,进程内复用。
fn font_collection() -> &'static Mutex<()> {
    static INIT: OnceLock<Mutex<()>> = OnceLock::new();
    INIT.get_or_init(|| Mutex::new(()))
}

/// swash FontRef 生命周期问题的解法:整形在锁内一次完成,输出拷贝。
/// 字重感知选字:项目注册表优先,系统字体回退。
fn resolve_font_weighted(family: &str, weight: u16, text: &str) -> Option<(Arc<Vec<u8>>, usize)> {
    if let Some(hit) = registry_font(family, weight) {
        return Some(hit);
    }
    use fontique::{Collection, CollectionOptions, QueryStatus, SourceCache, SourceCacheOptions};

    let _guard = font_collection().lock().ok()?;
    let mut collection = Collection::new(CollectionOptions::default());
    let mut sources = SourceCache::new(SourceCacheOptions::default());

    // 候选族:用户指定族 → 常见中英文族(CJK 兜底,Windows 优先名)
    let trimmed = family.trim().trim_matches('"').trim_matches('\'');
    let user = if trimmed.is_empty() {
        "Segoe UI".to_string()
    } else {
        // font-family 可能是逗号列表:取首族
        trimmed
            .split(',')
            .next()
            .unwrap_or(trimmed)
            .trim()
            .to_string()
    };
    let candidates = [
        user.clone(),
        "Microsoft YaHei".to_string(),
        "SimHei".to_string(),
        "Segoe UI".to_string(),
        "Arial".to_string(),
    ];

    let mut picked: Option<(Arc<Vec<u8>>, usize)> = None;
    for fam in candidates {
        let mut full_coverage: Option<(Arc<Vec<u8>>, usize)> = None;
        let mut first: Option<(Arc<Vec<u8>>, usize)> = None;
        let mut query = collection.query(&mut sources);
        query.set_families([fam.as_str()]);
        query.matches_with(|qf| {
            let data = qf.blob.data();
            let Some(font) = FontRef::from_index(data, qf.index as usize) else {
                return QueryStatus::Continue;
            };
            let entry = (Arc::new(data.to_vec()), qf.index as usize);
            if first.is_none() {
                first = Some(entry.clone());
            }
            // 覆盖检查:全部字符都有字形才算完全命中
            if text
                .chars()
                .filter(|c| !c.is_control())
                .all(|c| font.charmap().map(c) != 0)
            {
                full_coverage = Some(entry);
                QueryStatus::Stop
            } else {
                QueryStatus::Continue
            }
        });
        if let Some(e) = full_coverage.or_else(|| picked.clone()) {
            return Some(e);
        }
        if picked.is_none() {
            picked = first;
        }
    }
    picked
}

/// 文本 → 字形运行(默认字重 400;兼容旧调用方)。
pub fn shape_text(text: &str, font_family: &str, font_size: f32) -> Option<ShapedRun> {
    shape_text_weighted(text, font_family, font_size, 400)
}

/// 文本 → 字形运行(单字体;C4 已知限制见模块注释)。weight 参与选字。
pub fn shape_text_weighted(
    text: &str,
    font_family: &str,
    font_size: f32,
    weight: u16,
) -> Option<ShapedRun> {
    if text.trim().is_empty() || font_size <= 0.0 {
        return None;
    }
    let (font_data, font_index) = resolve_font_weighted(font_family, weight, text)?;
    let font = FontRef::from_index(&font_data[..], font_index)?;

    let mut shape_ctx = shape::ShapeContext::new();
    // 统一走 Latin 引擎(cmap 映射 + 字距):CJK 无复杂整形需求,
    // Han 引擎在部分字体上产出空字形(实测);复杂脚本留后续
    let mut shaper = shape_ctx
        .builder(font)
        .size(font_size)
        .script(Script::Latin)
        .direction(Direction::LeftToRight)
        .build();
    let metrics = shaper.metrics();
    let mut glyphs: Vec<ShapedGlyph> = Vec::new();
    let mut pen_x = 0.0f32;
    shaper.add_str(text);
    shaper.shape_with(|cluster| {
        for g in cluster.glyphs {
            glyphs.push(ShapedGlyph {
                id: g.id,
                x: pen_x + g.x,
                y: g.y,
                advance: g.advance,
            });
            pen_x += g.advance;
        }
    });
    if glyphs.is_empty() {
        return None;
    }
    Some(ShapedRun {
        font_data: Arc::new(font_data.to_vec()),
        font_index,
        glyphs,
        ascent: metrics.ascent,
        descent: metrics.descent,
    })
}

/// 字形 id → kurbo 路径(字体单位已按 size 缩放;Y 向下即直填)。
pub fn glyph_outline(
    font_data: &[u8],
    font_index: usize,
    size: f32,
    glyph_id: u16,
) -> Option<BezPath> {
    let font = FontRef::from_index(font_data, font_index)?;
    let gid = GlyphId::from(glyph_id);
    let mut ctx = swash::scale::ScaleContext::new();
    let mut scaler = ctx.builder(font).size(size).build();
    let outline = scaler.scale_outline(gid)?;
    let mut out = BezPath::new();
    use zeno::PathData;
    for cmd in outline.path().commands() {
        // swash 轮廓是 Y 向上(字体约定),这里翻成画布 Y 向下
        let p = |pt: zeno::Point| Point::new(pt.x as f64, -(pt.y as f64));
        match cmd {
            zeno::Command::MoveTo(p0) => out.move_to(p(p0)),
            zeno::Command::LineTo(p0) => out.line_to(p(p0)),
            zeno::Command::QuadTo(c, p0) => out.quad_to(p(c), p(p0)),
            zeno::Command::CurveTo(c1, c2, p0) => out.curve_to(p(c1), p(c2), p(p0)),
            zeno::Command::Close => out.close_path(),
        }
    }
    Some(out)
}

/// 从 CSS font-family 提取首族名(供 shape_text;引擎共用)。
pub fn first_family(style_get: impl Fn(&str) -> Option<String>) -> String {
    style_get("font-family").unwrap_or_default()
}

/// 布局量测:与 cpu.rs 渲染同一贪心断行策略(CJK 逐字可断、空白/标点后断,
/// 行首空白丢弃),另支持 `\n` 硬断行。返回(最长行宽 px, 行数)。
/// `max_width` 为 f32::MAX 时不折行(max-content)。
pub fn measure_text(
    text: &str,
    font_family: &str,
    font_size: f32,
    max_width: f32,
    letter_spacing: f32,
) -> (f32, usize) {
    measure_text_weighted(text, font_family, font_size, 400, max_width, letter_spacing)
}

/// 共享贪心断行(与 cpu.rs 渲染同策略):CJK 逐字可断、空白/ASCII 标点后断。
/// 输入必须是单个硬行(不含 `\n`;调用方先用 [`split_hard_lines`] 拆分,
/// 避免整形器跳过控制字符导致的字形/字符错位)。返回每行的字形索引。
pub fn break_lines(
    text: &str,
    run: &ShapedRun,
    max_width: f32,
    letter_spacing: f32,
) -> Vec<Vec<usize>> {
    let glyph_char = |gi: usize| -> char { text.chars().nth(gi).unwrap_or(' ') };
    let mut lines: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut cur_w = 0.0f32;
    let finite = max_width.is_finite();
    for gi in 0..run.glyphs.len() {
        let ch = glyph_char(gi);
        let gw = run.glyphs[gi].advance + letter_spacing;
        let too_wide = finite && cur_w + gw > max_width && !cur.is_empty();
        let cjk = (ch as u32) > 0x2E00;
        if too_wide && (ch.is_whitespace() || cjk || ch.is_ascii_punctuation()) {
            lines.push(std::mem::take(&mut cur));
            cur_w = 0.0;
            if ch.is_whitespace() {
                continue;
            }
        }
        cur.push(gi);
        cur_w += gw;
    }
    lines.push(cur);
    lines
}

/// 按硬行(`\n`)分段整形并断行:每硬行 = (行字符串, run, 视觉行字形索引集)。
/// 同一硬行的多个视觉行共享 run(一次整形,断行只挑索引)。
pub fn layout_text_lines(
    text: &str,
    font_family: &str,
    font_size: f32,
    weight: u16,
    max_width: f32,
    letter_spacing: f32,
) -> Vec<(String, ShapedRun, Vec<Vec<usize>>)> {
    text.split('\n')
        .map(
            |hard| match shape_text_weighted(hard, font_family, font_size, weight) {
                Some(run) => {
                    let lines = break_lines(hard, &run, max_width, letter_spacing);
                    (hard.to_string(), run, lines)
                }
                None => (
                    hard.to_string(),
                    ShapedRun {
                        font_data: Arc::new(Vec::new()),
                        font_index: 0,
                        glyphs: Vec::new(),
                        ascent: font_size * 0.8,
                        descent: font_size * 0.2,
                    },
                    vec![Vec::new()],
                ),
            },
        )
        .collect()
}

/// 视觉行遍历(写出器共用):按硬行整形 + 贪心断行,逐视觉行回调
/// `(视觉行序, 硬行文本, run, 字形索引, 硬行字节基址)`。
pub fn for_each_visual_line(
    text: &str,
    font_family: &str,
    font_size: f32,
    weight: u16,
    max_width: f32,
    letter_spacing: f32,
    mut f: impl FnMut(usize, &str, &ShapedRun, &[usize], usize),
) {
    let mut vi = 0usize;
    let mut byte_base = 0usize;
    for hard in text.split('\n') {
        if let Some(run) = shape_text_weighted(hard, font_family, font_size, weight) {
            for line in break_lines(hard, &run, max_width, letter_spacing) {
                if !line.is_empty() {
                    f(vi, hard, &run, &line, byte_base);
                }
                vi += 1;
            }
        } else {
            vi += 1;
        }
        byte_base += hard.len() + 1;
    }
}

/// 行内富文本段切片:连续同段(或段外)字形合为一个部件。
pub struct LinePart<'a> {
    pub text: &'a str,
    /// 段在 `segments` 中的索引;None = 段外(继承节点样式)。
    pub seg: Option<usize>,
    /// 部件起点相对行首的 x 偏移(含字距)。
    pub x: f64,
}

/// 把一个视觉行按段边界切片(供 SVG/PDF/PPTX 逐段上色)。
/// `segments`: 字节区间表;`byte_base`: 硬行在全文中的字节基址;
/// `char_bytes`: 行内字形序 → 行内字符序的字节宽(逐字形累计)。
pub fn split_line_segments<'a>(
    hard: &'a str,
    line: &[usize],
    run: &ShapedRun,
    byte_base: usize,
    segments: &[(usize, usize)],
    ls: f32,
) -> Vec<LinePart<'a>> {
    let line_x0 = run.glyphs[line[0]].x as f64;
    let mut parts: Vec<LinePart> = Vec::new();
    // 每字形的行内字节偏移
    let mut char_offsets: Vec<usize> = Vec::with_capacity(line.len());
    let mut acc = 0usize;
    for &gi in line {
        char_offsets.push(acc);
        let ch = hard.chars().nth(gi).unwrap_or(' ');
        acc += ch.len_utf8();
    }
    let mut p0 = 0usize;
    while p0 < line.len() {
        let b = byte_base + char_offsets[p0];
        let seg = segments.iter().position(|sg| b >= sg.0 && b < sg.1);
        let mut p1 = p0 + 1;
        while p1 < line.len() {
            let b1 = byte_base + char_offsets[p1];
            let s1 = segments.iter().position(|sg| b1 >= sg.0 && b1 < sg.1);
            if s1 != seg {
                break;
            }
            p1 += 1;
        }
        let first = &run.glyphs[line[p0]];
        let last = &run.glyphs[line[p1 - 1]];
        let start_in_hard: usize = hard.chars().take(line[p0]).map(|c| c.len_utf8()).sum();
        let end_in_hard: usize = hard
            .chars()
            .take(line[p1 - 1] + 1)
            .map(|c| c.len_utf8())
            .sum();
        parts.push(LinePart {
            text: &hard[start_in_hard..end_in_hard],
            seg,
            x: (first.x as f64 + p0 as f64 * ls as f64) - line_x0,
        });
        let _ = last;
        p0 = p1;
    }
    parts
}

/// 布局量测(字重感知版):返回(最长行宽 px, 行数)。
pub fn measure_text_weighted(
    text: &str,
    font_family: &str,
    font_size: f32,
    weight: u16,
    max_width: f32,
    letter_spacing: f32,
) -> (f32, usize) {
    let hard_lines = layout_text_lines(
        text,
        font_family,
        font_size,
        weight,
        max_width,
        letter_spacing,
    );
    let mut max_line_w = 0.0f32;
    let mut count = 0usize;
    for (_, run, visual_lines) in &hard_lines {
        for line in visual_lines {
            count += 1;
            if line.is_empty() {
                continue;
            }
            let first = &run.glyphs[line[0]];
            let last = &run.glyphs[*line.last().expect("nonempty")];
            let w = (last.x + last.advance) - first.x + letter_spacing;
            max_line_w = max_line_w.max(w);
        }
    }
    if count == 0 {
        count = 1;
    }
    (max_line_w, count)
}
