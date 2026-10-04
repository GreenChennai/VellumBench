//! 文本量测与整形(C4,ADR-0017 兑付;COUP-01 自 vb_render::text 上收):
//! fontique 字体发现 + swash 整形与轮廓缩放。CPU 导出用**真字形**替换
//! 占位条;画布文本继续由 vb_app 的 egui 覆盖层渲染(其自身已渲染真
//! 文本,见 ADR-0017)。
//!
//! 分层(2026-10-05 迭代审查 COUP-01,ADR-0053):本 crate 是布局与渲染
//! 共同依赖的**底层量测设施**——vb_layout 原为 `measure_text_weighted`
//! 一处调用而反向依赖 vb_render(vello/wgpu 编译面),现改为双方共同
//! 下依赖本 crate;`vb_render::text` 保留为纯 re-export 门面,下游调用
//! 点零改动。
//!
//! # 字体注册表实例化(COUP-06)
//!
//! 全部字体状态收进 [`FontRegistry`] 实例:项目 webfont(@font-face)表、
//! 字重感知选字缓存、fontique `Collection`/`SourceCache` 复用体、缺字体
//! 清单(DOC-10)。传递方式二选一:
//!
//! - **持实例**(推荐):`FontRegistry::new()` + 方法调用;导出任务持有
//!   自己的实例,随任务生灭,跨项目零残留,并行任务互不阻塞;
//! - **作用域上下文**:[`enter_font_scope`] 把实例压入当前线程作用域栈,
//!   下方列出的**便捷自由函数**在作用域内改走该实例(管道式代码不必逐层
//!   改签名,kiln 导出链即此用法)。Drop 时弹出,panic 安全。
//!
//! 自由函数(`shape_text_weighted`/`register_font_file`/…)是**兼容层**,
//! 语义上已废弃(deprecated):委托进程默认实例,仅适合「一进程一项目」
//! 的调用形态(vb_app 画布 / vb_agent CLI / vb_web 壳,均无实例改造余地
//! ——其公共 API 被禁改约束钉死)。多项目/多任务的调用方必须走实例或
//! 作用域,**不要**再依赖默认实例的进程级残留。
//!
//! v0.3 已知限制(诚实清单):
//! - 单字体运行:整个文本节点用一个字体覆盖(混合文字取第一个可覆盖
//!   全部的字体;覆盖不了出 .notdef)。逐字符回退留后续。
//! - LTR 方向;RTL 文本按 LTR 输出(落位由浏览器导出路径兜底)。
//! - 换行:v0.1 文本节点不自动换行(单行),与画布近似行为一致。

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use swash::shape::Direction;
use swash::{shape, text::Script, FontRef, GlyphId};

use vb_common::geom::{BezPath, Point};

// ---------- 字体注册表(COUP-06 实例化;DOC-10 不静默) ----------

/// 选字缓存键:(家庭小写, 字重, 参与覆盖检查的文本)。
/// 文本参与键是因为选字按「能否覆盖全部字符」取舍;缓存命中的第二收益是
/// 免去 fontique 全库扫描 + 整份字体字节拷贝(PERF-01/DOC-09 主热点)。
type ResolveKey = (String, u16, String);

/// 一个已解析字体:字体文件字节(共享) + ttc 内的 face 索引。
type ResolvedFont = (Arc<Vec<u8>>, usize);

/// 项目 webfont 表:家庭(小写)→ (字重 → 字体文件字节)。
type WebfontTable = HashMap<String, Vec<(u16, Arc<Vec<u8>>)>>;

/// 缓存条目上限(超出整体清空)。选字缓存服务的是「同一文本在布局/渲染/
/// 导出阶段被反复整形」的热路径,单任务内 distinct 文本数远小于此。
const RESOLVE_CACHE_CAP: usize = 256;

#[derive(Default)]
struct RegistryInner {
    /// 项目 webfont(@font-face)表。
    webfonts: WebfontTable,
    /// 字重感知选字缓存(含负缓存:系统也找不到的族反复查询是大头)。
    resolved: HashMap<ResolveKey, Option<ResolvedFont>>,
    /// fontique 枚举 + 字体源缓存(DOC-09:此前每次整形重建,全库重扫)。
    system: Option<(fontique::Collection, fontique::SourceCache)>,
    /// swash 整形上下文(内部缓冲复用;输出与每次新建逐位一致)。
    shape_ctx: shape::ShapeContext,
    /// swash 轮廓缩放上下文(同上)。
    scale_ctx: swash::scale::ScaleContext,
    /// DOC-10:@font-face 读盘/解析失败清单(不静默;kiln 并入导出告警,
    /// GUI 后续可接「字体缺失」对话框)。
    missing: Vec<String>,
}

/// 字体注册表实例:项目 webfont 注册 + 系统字体选字缓存 + 缺字体清单。
///
/// 一实例 = 一作用域(导出任务 / 项目会话)。所有方法 `&self`(内部
/// `Mutex`;锁中毒按恢复处理——字体注册不该因一次 panic 永久失能)。
pub struct FontRegistry {
    inner: Mutex<RegistryInner>,
}

impl Default for FontRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl FontRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(RegistryInner::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, RegistryInner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// 注册项目内字体文件(@font-face 导入侧调用;立即读盘)。
    ///
    /// DOC-10:读盘失败**不再静默成 0 字节字体**——返回 `Err`(消息含
    /// 家庭/字重/路径/原因),并记入 [`Self::missing_fonts`] 清单。
    pub fn register_font_file(
        &self,
        family: &str,
        weight: u16,
        path: PathBuf,
    ) -> Result<(), String> {
        let data = std::fs::read(&path).map_err(|e| {
            let msg = format!(
                "@font-face 字体读取失败: family={family} weight={weight} path={} : {e}",
                path.display()
            );
            let mut inner = self.lock();
            if !inner.missing.contains(&msg) {
                inner.missing.push(msg.clone());
            }
            msg
        })?;
        if data.is_empty() {
            let msg = format!(
                "@font-face 字体为空文件: family={family} weight={weight} path={}",
                path.display()
            );
            let mut inner = self.lock();
            if !inner.missing.contains(&msg) {
                inner.missing.push(msg.clone());
            }
            return Err(msg);
        }
        self.register_font_bytes(family, weight, data);
        Ok(())
    }

    /// 直接注册字体字节(wasm 无文件系统,web 壳经 fetch 拿字节后注册);
    /// 语义与 [`Self::register_font_file`] 一致(同键去重、就近字重匹配)。
    pub fn register_font_bytes(&self, family: &str, weight: u16, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        let key = family.trim().to_ascii_lowercase();
        let mut inner = self.lock();
        // 换字体源后旧选字缓存全部失效(字节可能是同名同重的另一文件)
        inner.resolved.clear();
        let slot = inner.webfonts.entry(key).or_default();
        if !slot.iter().any(|(w, _)| *w == weight) {
            slot.push((weight, Arc::new(bytes)));
            slot.sort_by_key(|(w, _)| *w);
        }
    }

    /// 清空注册表(换项目导入时)。系统字体缓存保留(与项目无关)。
    pub fn clear(&self) {
        let mut inner = self.lock();
        inner.webfonts.clear();
        inner.resolved.clear();
        inner.missing.clear();
    }

    /// 取本实例的缺字体清单(DOC-10;不消费,清单随实例存活)。
    pub fn missing_fonts(&self) -> Vec<String> {
        self.lock().missing.clone()
    }

    /// 按家庭+字重取字体字节(CSS 字重匹配:就近,先高后低;公开,PDF
    /// CID 嵌入用)。
    pub fn font_data_for(&self, family: &str, weight: u16) -> Option<(Arc<Vec<u8>>, usize)> {
        self.resolve_weighted(family, weight, " ")
    }

    /// 字重感知选字:项目注册表优先,系统字体回退;带缓存(PERF-01)。
    fn resolve_weighted(
        &self,
        family: &str,
        weight: u16,
        text: &str,
    ) -> Option<(Arc<Vec<u8>>, usize)> {
        let key = (family.trim().to_ascii_lowercase(), weight, text.to_string());
        let mut inner = self.lock();
        if let Some(hit) = inner.resolved.get(&key) {
            return hit.clone();
        }
        let out = resolve_uncached(&mut inner, family, weight, text);
        if inner.resolved.len() >= RESOLVE_CACHE_CAP {
            inner.resolved.clear();
        }
        inner.resolved.insert(key, out.clone());
        out
    }

    /// 文本 → 字形运行(单字体;C4 已知限制见模块注释)。weight 参与选字。
    pub fn shape_text_weighted(
        &self,
        text: &str,
        font_family: &str,
        font_size: f32,
        weight: u16,
    ) -> Option<ShapedRun> {
        if text.trim().is_empty() || font_size <= 0.0 {
            return None;
        }
        let (font_data, font_index) = self.resolve_weighted(font_family, weight, text)?;
        let font = FontRef::from_index(&font_data[..], font_index)?;

        // 统一走 Latin 引擎(cmap 映射 + 字距):CJK 无复杂整形需求,
        // Han 引擎在部分字体上产出空字形(实测);复杂脚本留后续。
        // 变量字体必须显式设 wght 轴:否则 NotoSerifSC-VF 之类永远取默认
        // 实例(≈400),`font-weight:900` 的大标题会渲染成细体(非变量字体为 no-op)。
        // 整形在实例锁内完成(swash ctx 复用 + FontRef 生命周期约束);
        // 锁随实例走,任务间不串行。
        let mut inner = self.lock();
        let mut shaper = inner
            .shape_ctx
            .builder(font)
            .size(font_size)
            .script(Script::Latin)
            .direction(Direction::LeftToRight)
            .variations([("wght", weight as f32)])
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
            // PERF-01:直接复用选字缓存的 Arc,不再整份再拷一次字体字节
            font_data,
            font_index,
            glyphs,
            ascent: metrics.ascent,
            descent: metrics.descent,
            weight,
        })
    }

    /// 字形 id → kurbo 路径(字体单位已按 size 缩放;Y 向下即直填)。
    pub fn glyph_outline(
        &self,
        font_data: &[u8],
        font_index: usize,
        size: f32,
        glyph_id: u16,
    ) -> Option<BezPath> {
        self.glyph_outline_weighted(font_data, font_index, size, glyph_id, 400)
    }

    /// 字重感知的字形轮廓:变量字体按 wght 轴实例化,与整形同一实例
    /// (否则整形用 900 的 advance、轮廓却取默认 400,字形与排布错配)。
    pub fn glyph_outline_weighted(
        &self,
        font_data: &[u8],
        font_index: usize,
        size: f32,
        glyph_id: u16,
        weight: u16,
    ) -> Option<BezPath> {
        let font = FontRef::from_index(font_data, font_index)?;
        let gid = GlyphId::from(glyph_id);
        let mut inner = self.lock();
        let mut scaler = inner
            .scale_ctx
            .builder(font)
            .size(size)
            .variations([("wght", weight as f32)])
            .build();
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

    /// 布局量测:与 cpu.rs 渲染同一贪心断行策略(CJK 逐字可断、空白/标点后断,
    /// 行首空白丢弃),另支持 `\n` 硬断行。返回(最长行宽 px, 行数)。
    /// `max_width` 为 f32::MAX 时不折行(max-content)。
    pub fn measure_text(
        &self,
        text: &str,
        font_family: &str,
        font_size: f32,
        max_width: f32,
        letter_spacing: f32,
    ) -> (f32, usize) {
        self.measure_text_weighted(text, font_family, font_size, 400, max_width, letter_spacing)
    }

    /// 布局量测(字重感知版):返回(最长行宽 px, 行数)。
    pub fn measure_text_weighted(
        &self,
        text: &str,
        font_family: &str,
        font_size: f32,
        weight: u16,
        max_width: f32,
        letter_spacing: f32,
    ) -> (f32, usize) {
        let hard_lines = self.layout_text_lines(
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
                // DOC-11:空行/越界索引守卫(此前的 expect("nonempty") 依赖
                // 调用方保证;守卫在 API 内,行为对非空行完全一致)
                let Some(&first_i) = line.first() else {
                    continue;
                };
                let Some(&last_i) = line.last() else {
                    continue;
                };
                let (Some(first), Some(last)) = (run.glyphs.get(first_i), run.glyphs.get(last_i))
                else {
                    continue;
                };
                let w = (last.x + last.advance) - first.x + letter_spacing;
                max_line_w = max_line_w.max(w);
            }
        }
        if count == 0 {
            count = 1;
        }
        (max_line_w, count)
    }

    /// 按硬行(`\n`)分段整形并断行:每硬行 = (行字符串, run, 视觉行字形索引集)。
    /// 同一硬行的多个视觉行共享 run(一次整形,断行只挑索引)。
    pub fn layout_text_lines(
        &self,
        text: &str,
        font_family: &str,
        font_size: f32,
        weight: u16,
        max_width: f32,
        letter_spacing: f32,
    ) -> Vec<(String, ShapedRun, Vec<Vec<usize>>)> {
        text.split('\n')
            .map(
                |hard| match self.shape_text_weighted(hard, font_family, font_size, weight) {
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
                            weight,
                        },
                        vec![Vec::new()],
                    ),
                },
            )
            .collect()
    }

    /// 视觉行遍历(写出器共用):按硬行整形 + 贪心断行,逐视觉行回调
    /// `(视觉行序, 硬行文本, run, 字形索引, 硬行字节基址)`。
    #[allow(clippy::too_many_arguments)]
    pub fn for_each_visual_line(
        &self,
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
            if let Some(run) = self.shape_text_weighted(hard, font_family, font_size, weight) {
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

    /// 富文本行布局:逐段用自己的字体整形。行划分仍用基础整形的断行结果;
    /// dom 路线行由 '\n' 硬换承载且画板够宽,不会触发软换(21 篇 T2)。
    #[allow(clippy::too_many_arguments)]
    pub fn for_each_styled_line(
        &self,
        text: &str,
        base_family: &str,
        base_size: f32,
        base_weight: u16,
        max_width: f32,
        ls: f32,
        spans: &[StyleSpan],
        mut f: impl FnMut(usize, &[StyledPart]),
    ) {
        let mut vi = 0usize;
        let mut byte_base = 0usize;
        for hard in text.split('\n') {
            let base_run = self.shape_text_weighted(hard, base_family, base_size, base_weight);
            let visual: Vec<Vec<usize>> = match &base_run {
                Some(run) => break_lines(hard, run, max_width, ls),
                None => vec![(0..hard.chars().count()).collect()],
            };
            // PERF-01:字符序 → 字节偏移前缀表(单趟),替代逐索引 O(n²) 求和
            let offs = char_offsets(hard);
            for line in &visual {
                if line.is_empty() {
                    vi += 1;
                    continue;
                }
                // 该视觉行的字节范围(相对 hard)
                let s0c = line[0];
                let e0c = line[line.len() - 1];
                let lb = char_byte(&offs, s0c, hard.len());
                let le = char_byte(&offs, e0c + 1, hard.len());
                let mut parts: Vec<StyledPart> = Vec::new();
                let mut x = 0f64;
                let mut b = lb;
                while b < le {
                    let abs = byte_base + b;
                    let span_i = spans.iter().position(|s| abs >= s.start && abs < s.end);
                    // 切片终点:段边界 ∩ 行尾
                    let mut e = le;
                    match span_i {
                        Some(si) => {
                            let se = spans[si].end.saturating_sub(byte_base);
                            e = e.min(se.max(b + 1));
                        }
                        None => {
                            if let Some(next) = spans
                                .iter()
                                .map(|s| s.start.saturating_sub(byte_base))
                                .filter(|&v| v > b)
                                .min()
                            {
                                e = e.min(next);
                            }
                        }
                    }
                    if e <= b {
                        break;
                    }
                    let slice = &hard[b..e];
                    let (fam, size, weight) = match span_i {
                        Some(si) => (
                            if spans[si].font_family.is_empty() {
                                base_family
                            } else {
                                spans[si].font_family.as_str()
                            },
                            spans[si].font_size.map(|v| v as f32).unwrap_or(base_size),
                            match spans[si].bold {
                                Some(true) => 700u16,
                                Some(false) => 400,
                                None => base_weight,
                            },
                        ),
                        None => (base_family, base_size, base_weight),
                    };
                    if let Some(sr) = self.shape_text_weighted(slice, fam, size, weight) {
                        let adv: f32 = sr.glyphs.iter().map(|g| g.advance).sum();
                        parts.push(StyledPart {
                            text: slice.to_string(),
                            seg: span_i,
                            x,
                            gids: sr.glyphs.iter().map(|g| g.id).collect(),
                            advances: sr.glyphs.iter().map(|g| g.advance).collect(),
                            font_family: fam.to_string(),
                            font_size: size,
                            weight,
                            ascent: sr.ascent,
                            descent: sr.descent,
                        });
                        x += (adv + ls * sr.glyphs.len() as f32) as f64;
                    }
                    b = e;
                }
                if !parts.is_empty() {
                    f(vi, &parts);
                }
                vi += 1;
            }
            byte_base += hard.len() + 1;
        }
    }
}

/// 系统字体选字(无缓存;调用方需持 [`FontRegistry`] 锁)。
fn resolve_uncached(
    inner: &mut RegistryInner,
    family: &str,
    weight: u16,
    text: &str,
) -> Option<(Arc<Vec<u8>>, usize)> {
    // 项目注册表优先
    if let Some(hit) = registry_font(inner, family, weight) {
        return Some(hit);
    }
    use fontique::{Collection, CollectionOptions, QueryStatus, SourceCache, SourceCacheOptions};

    // fontique 枚举/字体源缓存复用(DOC-09:此前每次整形全库重扫)
    if inner.system.is_none() {
        inner.system = Some((
            Collection::new(CollectionOptions::default()),
            SourceCache::new(SourceCacheOptions::default()),
        ));
    }
    let (collection, sources) = inner.system.as_mut()?;

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
    // 候选族跨平台兜底链:用户族缺失时按 Windows → macOS → Linux 顺序找
    // 真实存在的替代族。此前只有 Windows 名(YaHei/SimHei/Segoe/Arial),
    // linux/精简容器上一个都不存在 → 直接回落占位条(真字形管线白装)。
    // 顺序即优先级:CJK 字形必须优先命中,拉丁族只做最后兜底。
    let candidates = [
        user.clone(),
        "Microsoft YaHei".to_string(),
        "SimHei".to_string(),
        "PingFang SC".to_string(),
        "Hiragino Sans GB".to_string(),
        "Noto Sans CJK SC".to_string(),
        "Source Han Sans SC".to_string(),
        "WenQuanYi Micro Hei".to_string(),
        "Segoe UI".to_string(),
        "Arial".to_string(),
        "Helvetica".to_string(),
        "Liberation Sans".to_string(),
        "DejaVu Sans".to_string(),
        "Noto Sans".to_string(),
        "Times New Roman".to_string(),
    ];

    let mut picked: Option<(Arc<Vec<u8>>, usize)> = None;
    for fam in candidates {
        let mut full_coverage: Option<(Arc<Vec<u8>>, usize)> = None;
        let mut first: Option<(Arc<Vec<u8>>, usize)> = None;
        let mut query = collection.query(sources);
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

/// 按家庭+字重取注册字体(CSS 字重匹配:就近,先高后低)。
fn registry_font(
    inner: &RegistryInner,
    family: &str,
    weight: u16,
) -> Option<(Arc<Vec<u8>>, usize)> {
    let key = family.trim().to_ascii_lowercase();
    let faces = inner.webfonts.get(&key)?;
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

// ---------- 默认实例 + 线程作用域(COUP-06 兼容层) ----------

fn default_registry() -> &'static Arc<FontRegistry> {
    static INIT: OnceLock<Arc<FontRegistry>> = OnceLock::new();
    INIT.get_or_init(|| Arc::new(FontRegistry::new()))
}

thread_local! {
    /// 线程局部作用域栈:每个导出任务线程可持有自己的实例
    /// (kiln 动画分段并行 worker 各自注册各自的 @font-face,互不污染)。
    static SCOPE: RefCell<Vec<Arc<FontRegistry>>> = const { RefCell::new(Vec::new()) };
}

/// 便捷自由函数当前生效的实例:作用域栈顶 > 进程默认实例。
fn current_registry() -> Arc<FontRegistry> {
    SCOPE
        .with(|s| s.borrow().last().cloned())
        .unwrap_or_else(|| default_registry().clone())
}

/// [`enter_font_scope`] 返回的守卫:Drop 时弹出作用域(panic 安全)。
#[must_use = "作用域守卫被丢弃 = 作用域立即结束"]
pub struct FontScopeGuard {
    /// 防 FCO(formal construction outside);仅由 enter_font_scope 构造。
    _priv: (),
}

impl Drop for FontScopeGuard {
    fn drop(&mut self) {
        SCOPE.with(|s| {
            s.borrow_mut().pop();
        });
    }
}

/// 把实例压入当前线程作用域栈:守卫存活期间,本线程的**便捷自由函数**
/// 全部改走该实例。kiln 导出链用法:任务开始处进入作用域,函数结束
/// (含 panic)自动弹出,实例随任务生灭,跨项目零残留。
pub fn enter_font_scope(reg: Arc<FontRegistry>) -> FontScopeGuard {
    SCOPE.with(|s| {
        s.borrow_mut().push(reg);
    });
    FontScopeGuard { _priv: () }
}

// ---------- 便捷自由函数(兼容层;语义废弃,见模块注释) ----------
//
// 以下函数委托「当前线程生效实例」;仅供 vb_app/vb_agent/vb_web 等
// 单实例调用形态(vb_app 零改动约束)。新代码请持 FontRegistry 实例。

/// 注册项目内字体文件(@font-face 导入侧调用;立即读盘)。
/// **兼容便捷入口(已废弃语义)**:作用于默认实例;多项目调用方请持
/// [`FontRegistry`] 实例或用 [`enter_font_scope`]。
/// DOC-10:读盘失败返回 `Err` 并记入实例缺字体清单,不再静默。
pub fn register_font_file(family: &str, weight: u16, path: PathBuf) -> Result<(), String> {
    current_registry().register_font_file(family, weight, path)
}

/// 直接注册字体字节(K2 Web,05-11-2):wasm32 无文件系统,web 壳经
/// fetch / 文件选择器拿到字节后由此注册;语义与 [`register_font_file`]
/// 完全一致(同键去重、就近字重匹配)。桌面端不必使用。
/// **兼容便捷入口(已废弃语义)**:作用于默认实例。
pub fn register_font_bytes(family: &str, weight: u16, bytes: Vec<u8>) {
    current_registry().register_font_bytes(family, weight, bytes)
}

/// 清空注册表(换项目导入时)。**兼容便捷入口(已废弃语义)**。
pub fn clear_font_registry() {
    current_registry().clear()
}

/// 便捷入口的缺字体清单(DOC-10):默认实例(或当前作用域实例)自
/// [`register_font_file`] 以来记录的读取失败项。
pub fn missing_fonts() -> Vec<String> {
    current_registry().missing_fonts()
}

/// 字形(位置为**文本起点相对**坐标,Y 向下)。
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
    /// 整形所用字重(变量字体 wght 轴;字形轮廓必须用同一实例)。
    pub weight: u16,
}

/// swash FontRef 生命周期问题的解法:整形在锁内一次完成,输出拷贝。
/// 按家庭+字重取字体字节(公开;PDF CID 嵌入用)。**兼容便捷入口**。
pub fn font_data_for(family: &str, weight: u16) -> Option<(Arc<Vec<u8>>, usize)> {
    current_registry().font_data_for(family, weight)
}

/// 文本 → 字形运行(默认字重 400;兼容旧调用方)。**兼容便捷入口**。
pub fn shape_text(text: &str, font_family: &str, font_size: f32) -> Option<ShapedRun> {
    current_registry().shape_text_weighted(text, font_family, font_size, 400)
}

/// 文本 → 字形运行(单字体;C4 已知限制见模块注释)。weight 参与选字。
/// **兼容便捷入口**。
pub fn shape_text_weighted(
    text: &str,
    font_family: &str,
    font_size: f32,
    weight: u16,
) -> Option<ShapedRun> {
    current_registry().shape_text_weighted(text, font_family, font_size, weight)
}

/// 字形 id → kurbo 路径(字体单位已按 size 缩放;Y 向下即直填)。
/// **兼容便捷入口**。
pub fn glyph_outline(
    font_data: &[u8],
    font_index: usize,
    size: f32,
    glyph_id: u16,
) -> Option<BezPath> {
    current_registry().glyph_outline(font_data, font_index, size, glyph_id)
}

/// 字重感知的字形轮廓:变量字体按 wght 轴实例化,与整形同一实例。
/// **兼容便捷入口**。
pub fn glyph_outline_weighted(
    font_data: &[u8],
    font_index: usize,
    size: f32,
    glyph_id: u16,
    weight: u16,
) -> Option<BezPath> {
    current_registry().glyph_outline_weighted(font_data, font_index, size, glyph_id, weight)
}

/// 从 CSS font-family 提取首族名(供 shape_text;引擎共用)。
pub fn first_family(style_get: impl Fn(&str) -> Option<String>) -> String {
    style_get("font-family").unwrap_or_default()
}

/// 布局量测:与 cpu.rs 渲染同一贪心断行策略(CJK 逐字可断、空白/标点后断,
/// 行首空白丢弃),另支持 `\n` 硬断行。返回(最长行宽 px, 行数)。
/// `max_width` 为 f32::MAX 时不折行(max-content)。**兼容便捷入口**。
pub fn measure_text(
    text: &str,
    font_family: &str,
    font_size: f32,
    max_width: f32,
    letter_spacing: f32,
) -> (f32, usize) {
    current_registry().measure_text(text, font_family, font_size, max_width, letter_spacing)
}

/// 布局量测(字重感知版):返回(最长行宽 px, 行数)。**兼容便捷入口**。
pub fn measure_text_weighted(
    text: &str,
    font_family: &str,
    font_size: f32,
    weight: u16,
    max_width: f32,
    letter_spacing: f32,
) -> (f32, usize) {
    current_registry().measure_text_weighted(
        text,
        font_family,
        font_size,
        weight,
        max_width,
        letter_spacing,
    )
}

/// 按硬行(`\n`)分段整形并断行:每硬行 = (行字符串, run, 视觉行字形索引集)。
/// **兼容便捷入口**。
pub fn layout_text_lines(
    text: &str,
    font_family: &str,
    font_size: f32,
    weight: u16,
    max_width: f32,
    letter_spacing: f32,
) -> Vec<(String, ShapedRun, Vec<Vec<usize>>)> {
    current_registry().layout_text_lines(
        text,
        font_family,
        font_size,
        weight,
        max_width,
        letter_spacing,
    )
}

/// 视觉行遍历(写出器共用):按硬行整形 + 贪心断行,逐视觉行回调
/// `(视觉行序, 硬行文本, run, 字形索引, 硬行字节基址)`。**兼容便捷入口**。
pub fn for_each_visual_line(
    text: &str,
    font_family: &str,
    font_size: f32,
    weight: u16,
    max_width: f32,
    letter_spacing: f32,
    f: impl FnMut(usize, &str, &ShapedRun, &[usize], usize),
) {
    current_registry().for_each_visual_line(
        text,
        font_family,
        font_size,
        weight,
        max_width,
        letter_spacing,
        f,
    )
}

/// 富文本行布局:逐段用自己的字体整形(与「基础字体整行整形后切片」的
/// 旧口径不同——旧口径里 SegStyle 的字号/字重/字体全部失效,重点字被
/// 统一成正文,21 篇 T2)。**兼容便捷入口**。
#[allow(clippy::too_many_arguments)]
pub fn for_each_styled_line(
    text: &str,
    base_family: &str,
    base_size: f32,
    base_weight: u16,
    max_width: f32,
    ls: f32,
    spans: &[StyleSpan],
    f: impl FnMut(usize, &[StyledPart]),
) {
    current_registry().for_each_styled_line(
        text,
        base_family,
        base_size,
        base_weight,
        max_width,
        ls,
        spans,
        f,
    )
}

// ---------- 断行与行内切片(纯函数,不涉注册表) ----------

/// 共享贪心断行(与 cpu.rs 渲染同策略):CJK 逐字可断、空白/ASCII 标点后断、
/// 禁则処理(行首禁则字不换行,悬挂在上一行行末)。
/// 输入必须是单个硬行(不含 `\n`;调用方先用 [`layout_text_lines`] 拆分,
/// 避免整形器跳过控制字符导致的字形/字符错位)。返回每行的字形索引。
pub fn break_lines(
    text: &str,
    run: &ShapedRun,
    max_width: f32,
    letter_spacing: f32,
) -> Vec<Vec<usize>> {
    // PERF-01:字符索引一次性建表。此前的 `chars().nth(gi)` 是 O(n²)
    // (n=1000 时约 50 万次迭代/行);表查询语义逐位一致(越界仍得 ' ')
    let chars: Vec<char> = text.chars().collect();
    let mut lines: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut cur_w = 0.0f32;
    let finite = max_width.is_finite();
    for gi in 0..run.glyphs.len() {
        let ch = chars.get(gi).copied().unwrap_or(' ');
        let gw = run.glyphs[gi].advance + letter_spacing;
        let too_wide = finite && cur_w + gw > max_width && !cur.is_empty();
        let cjk = (ch as u32) > 0x2E00;
        if too_wide && (ch.is_whitespace() || cjk || ch.is_ascii_punctuation()) {
            // 禁则: 行首禁则字不换行,悬挂在上一行行末(允许微溢出)
            if is_forbidden_line_start(ch) {
                cur.push(gi);
                cur_w += gw;
                continue;
            }
            lines.push(std::mem::take(&mut cur));
            cur_w = 0.0;
            if ch.is_whitespace() {
                continue;
            }
        }
        cur.push(gi);
        cur_w += gw;
    }
    // 注:末尾 cur 为空也 push(行计数进入布局高度,牵动像素),
    // 有意保持旧语义逐位不变(见 ADR-0053 §1.3)
    lines.push(cur);
    lines
}

/// 禁则:不允许出现在行首的字符(CJK 闭标点 + 行尾符号)。
fn is_forbidden_line_start(ch: char) -> bool {
    matches!(
        ch,
        '。' | '、'
            | '！'
            | '？'
            | '：'
            | '；'
            | '）'
            | '】'
            | '》'
            | '〉'
            | '」'
            | '』'
            | '〕'
            | '〗'
            | '〙'
            | '〛'
            | '・'
            | '～'
            | ','
            | '.'
            | ':'
            | ';'
            | '!'
            | '?'
            | ')'
            | ']'
            | '}'
            | '%'
    )
}

/// 字符序 → 字节偏移前缀表(PERF-01 热路径工具,单趟 O(n) 建表):
/// `offs[i]` = 第 i 个字符的字节起点,`offs[chars().count()]` = `s.len()`。
/// 配套 [`char_byte`] 查表。此前热路径的 `chars().nth(gi)` /
/// `chars().take(gi).map(len_utf8).sum()` 都是逐次 O(n),合计 O(n²)。
pub fn char_offsets(s: &str) -> Vec<usize> {
    let mut v = Vec::with_capacity(s.chars().count() + 1);
    v.push(0);
    let mut off = 0usize;
    for c in s.chars() {
        off += c.len_utf8();
        v.push(off);
    }
    v
}

/// 查 [`char_offsets`] 表:第 `i` 个字符的字符内字节偏移;`i` 超出字符数时
/// 返回 `total`(与旧 `chars().take(i).map(len_utf8).sum()` 语义逐位一致)。
fn char_byte(offs: &[usize], i: usize, total: usize) -> usize {
    offs.get(i).copied().unwrap_or(total)
}

/// 第 `gi` 个字符(越界得 `' '`,与旧 `chars().nth(gi).unwrap_or(' ')` 一致)。
fn char_at(s: &str, offs: &[usize], gi: usize) -> char {
    match offs.get(gi).and_then(|&b| s.get(b..)) {
        Some(rest) => rest.chars().next().unwrap_or(' '),
        None => ' ',
    }
}

/// 行内富文本段切片:连续同段(或段外)字形合为一个部件。
pub struct LinePart<'a> {
    pub text: &'a str,
    /// 段在 `segments` 中的索引;None = 段外(继承节点样式)。
    pub seg: Option<usize>,
    /// 部件起点相对行首的 x 偏移(含字距)。
    pub x: f64,
    /// 部件覆盖的字形 id(与 text 字符一一对应;CID 嵌入用)。
    pub gids: Vec<u16>,
    /// 对应字形 advance(px)。
    pub advances: Vec<f32>,
}

/// 把一个视觉行按段边界切片(供 SVG/PDF/PPTX 逐段上色)。
/// `segments`: 字节区间表;`byte_base`: 硬行在全文中的字节基址;
/// `char_offsets`: 行内字形序 → 行内字符序的字节宽(逐字形累计)。
pub fn split_line_segments<'a>(
    hard: &'a str,
    line: &[usize],
    run: &ShapedRun,
    byte_base: usize,
    segments: &[(usize, usize)],
    ls: f32,
) -> Vec<LinePart<'a>> {
    // DOC-11:空行 / 首字形越界守卫(此前 `line[0]` 依赖调用方保证非空)
    let Some(&first_gi) = line.first() else {
        return Vec::new();
    };
    let Some(first_glyph) = run.glyphs.get(first_gi) else {
        return Vec::new();
    };
    let line_x0 = first_glyph.x as f64;
    // PERF-01:字形序 → 字节偏移一次建表(替代循环内 chars().nth + take().sum())
    let offs = char_offsets(hard);
    let mut parts: Vec<LinePart> = Vec::new();
    // 每字形的行内字节偏移
    let mut char_offsets_in_line: Vec<usize> = Vec::with_capacity(line.len());
    let mut acc = 0usize;
    for &gi in line {
        char_offsets_in_line.push(acc);
        acc += char_at(hard, &offs, gi).len_utf8();
    }
    let mut p0 = 0usize;
    while p0 < line.len() {
        let b = byte_base + char_offsets_in_line[p0];
        let seg = segments.iter().position(|sg| b >= sg.0 && b < sg.1);
        let mut p1 = p0 + 1;
        while p1 < line.len() {
            let b1 = byte_base + char_offsets_in_line[p1];
            let s1 = segments.iter().position(|sg| b1 >= sg.0 && b1 < sg.1);
            if s1 != seg {
                break;
            }
            p1 += 1;
        }
        let first = &run.glyphs[line[p0]];
        let start_in_hard = char_byte(&offs, line[p0], hard.len());
        let end_in_hard = char_byte(&offs, line[p1 - 1] + 1, hard.len());
        parts.push(LinePart {
            text: &hard[start_in_hard..end_in_hard],
            seg,
            x: (first.x as f64 + p0 as f64 * ls as f64) - line_x0,
            gids: line[p0..p1].iter().map(|&gi| run.glyphs[gi].id).collect(),
            advances: line[p0..p1]
                .iter()
                .map(|&gi| run.glyphs[gi].advance)
                .collect(),
        });
        p0 = p1;
    }
    parts
}

// ---------- 富文本逐段整形(T2,21 篇)----------

/// 行内段样式快照(写入器从 TextSpanHint 映射;字节区间同 split_line_segments)。
#[derive(Debug, Clone)]
pub struct StyleSpan {
    pub start: usize,
    pub end: usize,
    pub color: Option<[f32; 4]>,
    pub bold: Option<bool>,
    pub font_size: Option<f64>,
    /// 空 = 继承节点字体。
    pub font_family: String,
}

/// 逐段整形后的行内部件:每段用自己的字体/字号/字重,x 为段前累计推进。
pub struct StyledPart {
    pub text: String,
    /// 段索引;None = 段外(节点样式)。
    pub seg: Option<usize>,
    /// 相对行首的 x(含字距)。
    pub x: f64,
    pub gids: Vec<u16>,
    pub advances: Vec<f32>,
    pub font_family: String,
    pub font_size: f32,
    pub weight: u16,
    pub ascent: f32,
    pub descent: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PERF-01 等价性:表查询与旧 O(n²) 实现逐位一致(含越界/多字节)。
    #[test]
    fn char_offsets_match_naive_sum() {
        for s in ["", "a", "绘台 Vellum", "é中\0文🦀", "line\nbreak"] {
            let offs = char_offsets(s);
            let n = s.chars().count();
            assert_eq!(offs.len(), n + 1);
            for i in 0..=n + 2 {
                let naive: usize = s.chars().take(i).map(|c| c.len_utf8()).sum();
                assert_eq!(char_byte(&offs, i, s.len()), naive, "s={s:?} i={i}");
            }
        }
    }

    #[test]
    fn char_at_matches_naive_nth() {
        let s = "绘台a🦀b";
        let offs = char_offsets(s);
        for i in 0..s.chars().count() + 2 {
            let naive = s.chars().nth(i).unwrap_or(' ');
            assert_eq!(char_at(s, &offs, i), naive, "i={i}");
        }
    }

    /// COUP-06:实例隔离——A 实例注册的 webfont 在 B 实例不可见。
    /// (注意不能用 `font_data_for == None` 断言隔离:选字有系统回退链,
    /// 未知族也会命中系统字体。)
    #[test]
    fn registries_are_isolated() {
        let a = FontRegistry::new();
        let b = FontRegistry::new();
        a.register_font_bytes(
            "TestOnlyFamily",
            700,
            b"not a real font but nonempty".to_vec(),
        );
        assert!(a.lock().webfonts.contains_key("testonlyfamily"));
        assert!(!b.lock().webfonts.contains_key("testonlyfamily"));
        // 注册表命中返回注册字节本身
        assert_eq!(
            a.font_data_for("TestOnlyFamily", 700).map(|(d, _)| d.len()),
            Some(28)
        );
        // 负缓存不跨实例泄漏(b 的选字缓存里没有 a 的注册)
        let (bd, _i) = b
            .font_data_for("TestOnlyFamily", 700)
            .expect("系统回退必有命中");
        assert_ne!(bd.len(), 28, "b 命中的必须是系统回退而非 a 的注册");
    }

    /// COUP-06:作用域进出——自由函数在作用域内走实例,弹出后回默认。
    #[test]
    fn font_scope_routes_free_functions() {
        let reg = Arc::new(FontRegistry::new());
        let bogus = std::env::temp_dir().join(format!(
            "vb-textmeasure-scope-{}-{}.ttf",
            std::process::id(),
            std::time::SystemTime::now()
                .elapsed()
                .unwrap_or_default()
                .as_nanos()
        ));
        let default_missing_before = missing_fonts().len();
        {
            let _g = enter_font_scope(reg.clone());
            // 读盘失败落在作用域实例(自由函数被作用域改道)
            assert!(register_font_file("MissingFamily", 400, bogus).is_err());
            assert_eq!(missing_fonts().len(), default_missing_before + 1);
        }
        // 弹出后回默认实例:默认实例未受影响
        assert_eq!(missing_fonts().len(), default_missing_before);
        assert_eq!(reg.missing_fonts().len(), 1);
    }

    /// DOC-10:读盘失败 → Err + 清单记录,不再静默成 0 字节字体。
    #[test]
    fn missing_font_file_is_reported() {
        let reg = FontRegistry::new();
        let bogus = std::env::temp_dir().join(format!(
            "vb-textmeasure-missing-{}-{}.ttf",
            std::process::id(),
            std::time::SystemTime::now()
                .elapsed()
                .unwrap_or_default()
                .as_nanos()
        ));
        let r = reg.register_font_file("MissingFamily", 400, bogus);
        assert!(r.is_err(), "读盘失败必须返回 Err");
        let missing = reg.missing_fonts();
        assert!(
            missing.iter().any(|m| m.contains("MissingFamily")),
            "失败必须进缺字体清单: {missing:?}"
        );
        // 未注册出空字节字体条目(旧 unwrap_or_default 行为已根除)
        assert!(!reg.lock().webfonts.contains_key("missingfamily"));
    }

    /// PERF-01:选字缓存命中(同输入第二次 resolve 不再走全库扫描)。
    #[test]
    fn resolve_cache_hits_and_bounded() {
        let reg = FontRegistry::new();
        let first = reg.font_data_for("Microsoft YaHei", 400);
        let second = reg.font_data_for("Microsoft YaHei", 400);
        assert_eq!(
            first.map(|(d, i)| (d.len(), i)),
            second.map(|(d, i)| (d.len(), i))
        );
        // 上限:灌满后整体清空,不无限增长
        for i in 0..(RESOLVE_CACHE_CAP + 8) {
            reg.resolve_weighted("fam", 400, &format!("t{i}"));
        }
        assert!(reg.lock().resolved.len() <= RESOLVE_CACHE_CAP);
    }

    /// DOC-11:measure 空行/越界守卫不 panic,且计数行为不变。
    #[test]
    fn measure_guards_do_not_panic() {
        let reg = FontRegistry::new();
        // 空文本 / 全空白 / 纯换行:不 panic
        let (_w, n) = reg.measure_text_weighted("", "Microsoft YaHei", 16.0, 400, 100.0, 0.0);
        assert!(n >= 1);
        let (_w, n) = reg.measure_text_weighted("\n\n", "Microsoft YaHei", 16.0, 400, 100.0, 0.0);
        assert!(n >= 3);
    }

    /// 负缓存 + webfont 优先:注册后立即命中注册表,清空后回系统。
    #[test]
    fn webfont_overrides_system_then_clear() {
        let reg = FontRegistry::new();
        // 系统路径可解析(本机存在 YaHei;CI 精简容器退化为占位条,同样通过)
        let sys = reg.font_data_for("Microsoft YaHei", 400);
        reg.register_font_bytes("Microsoft YaHei", 400, b"override".to_vec());
        let hit = reg.font_data_for("Microsoft YaHei", 400);
        assert_eq!(hit.map(|(d, _)| d.len()), Some(8), "webfont 必须优先于系统");
        reg.clear();
        let after = reg.font_data_for("Microsoft YaHei", 400);
        assert_eq!(
            after.map(|(d, _)| d.len()),
            sys.map(|(d, _)| d.len()),
            "clear 后应回系统选字"
        );
    }
}
