//! KilnReport:每次导出的告警与指标(对齐并超越 WPI 可观测性)。

use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct KilnReport {
    /// 告警(不中断导出;错误见 KilnError)。
    pub warnings: Vec<crate::error::KilnWarning>,
    /// 编码耗时(毫秒)。
    pub encode_ms: u64,
    /// 输出字节数。
    pub bytes: usize,
    /// 帧数(动画格式)。
    pub frame_count: usize,
    /// 是否降级(MP4→GIF、原语丢弃等;输出与源不等价)。
    pub degraded: bool,
    /// 渲染引擎标识(溯源)。
    pub engine: &'static str,
    /// 动画覆盖矩阵(VB-3;无动画声明时为 None)。
    pub anim_coverage: Option<crate::anim::AnimCoverage>,
    /// 逐实例帧间隔统计(硬骨头 #18;浏览器动画车道逐实例回传,自研
    /// 车道 / 静态格式为空)。kiln-cli 把它以 `instances:[…]` 并入结果
    /// JSON;老 warnings 文本行(「实例 N 统计:…」)保持原样不删。
    pub instances: Vec<InstanceStats>,
}

impl KilnReport {
    pub fn new() -> Self {
        KilnReport {
            engine: "kiln-cpu/tiny-skia",
            ..Default::default()
        }
    }

    pub fn warning_lines(&self) -> Vec<String> {
        self.warnings.iter().map(|w| w.message()).collect()
    }

    /// 按类别聚合告警计数(VB-5;派生值恒与 warnings 一致,无失步可能)。
    /// 键为 [`KilnWarning::kind`] 的稳定小写蛇形键,供下游门禁:
    /// 如「unsupported_dropped > 0 则拒绝交付矢量稿」。
    pub fn warnings_by_kind(&self) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        for w in &self.warnings {
            *out.entry(w.kind().to_string()).or_insert(0) += 1;
        }
        out
    }

    /// 单类别告警条数(0 = 无)。
    pub fn count_of(&self, kind: &str) -> usize {
        self.warnings_by_kind().get(kind).copied().unwrap_or(0)
    }

    /// 单行摘要(状态栏/日志)。
    pub fn summary(&self) -> String {
        let mut s = format!("{} KB / {}ms", self.bytes / 1024, self.encode_ms);
        if self.frame_count > 1 {
            s.push_str(&format!(" / {}帧", self.frame_count));
        }
        if self.degraded {
            s.push_str(" / 降级");
        }
        if !self.warnings.is_empty() {
            s.push_str(&format!(" / {}条警告", self.warnings.len()));
        }
        s
    }
}

/// 帧间隔直方图(硬骨头 #18:跨实例串行点终判的自证数据)。
///
/// **判读方法**(结论备忘见 ADR-0047,数据出口即本结构):
/// - p95 集中在**固定倍数间隔**(~16.7 / 33.3 / 66.7ms,即 60/30/15fps
///   的标称帧间隔)且不随实例数变化 → GPU 读回驱动级串行;
/// - p95 与直方图主体随实例数**右移**(桶整体向慢档迁移)→ CPU 真饱和;
/// - 双峰/长尾(>133 桶占比高)→ 冷启动/编码抢点等瞬态,先看 max 定位。
#[derive(Debug, Clone, PartialEq)]
pub struct FrameIntervalStats {
    /// 中位帧间隔(毫秒)。
    pub p50: f64,
    /// 95 分位帧间隔(毫秒;判读主指标)。
    pub p95: f64,
    /// 99 分位帧间隔(毫秒)。
    pub p99: f64,
    /// 最大帧间隔(毫秒;瞬态尖刺定位)。
    pub max: f64,
    /// 间隔分桶计数(键序固定:<=16 / 17-33 / 34-66 / 67-133 / >133)。
    pub histogram: IntervalHistogram,
}

impl FrameIntervalStats {
    /// 全零占位(单帧/零间隔样本时保持 JSON 形状稳定)。
    pub const EMPTY: FrameIntervalStats = FrameIntervalStats {
        p50: 0.0,
        p95: 0.0,
        p99: 0.0,
        max: 0.0,
        histogram: IntervalHistogram {
            le16: 0,
            r17_33: 0,
            r34_66: 0,
            r67_133: 0,
            gt133: 0,
        },
    };
}

/// 帧间隔分桶。
///
/// 桶界 = 60/30/15/7.5 fps 的标称帧间隔(16.67/33.33/66.67/133.33ms)
/// 各加 0.05ms 容差,使整倍率采样节奏正好落本桶(60fps 的 16.67ms 计入
/// `<=16` 而非 `17-33`)。纯 Rust f64 比较,跨平台判定一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IntervalHistogram {
    /// ≤16.72ms(约 ≥60fps)。
    pub le16: u64,
    /// 16.72–33.38ms(约 30–60fps)。
    pub r17_33: u64,
    /// 33.38–66.72ms(约 15–30fps)。
    pub r34_66: u64,
    /// 66.72–133.38ms(约 7.5–15fps)。
    pub r67_133: u64,
    /// >133.38ms(显著慢档/尖刺)。
    pub gt133: u64,
}

impl IntervalHistogram {
    /// 桶界(毫秒):与各档 fps 标称间隔对齐(见类型注释)。
    pub const BOUNDS: [f64; 4] = [16.72, 33.38, 66.72, 133.38];

    /// 单样本入桶。
    pub fn bucket(&mut self, interval_ms: f64) {
        let b = Self::BOUNDS;
        match interval_ms {
            v if v <= b[0] => self.le16 += 1,
            v if v <= b[1] => self.r17_33 += 1,
            v if v <= b[2] => self.r34_66 += 1,
            v if v <= b[3] => self.r67_133 += 1,
            _ => self.gt133 += 1,
        }
    }

    /// 全桶计数之和(自检:应等于帧间隔样本数)。
    pub fn total(&self) -> u64 {
        self.le16 + self.r17_33 + self.r34_66 + self.r67_133 + self.gt133
    }

    /// 固定键序 JSON 片段(`"<=16":n,"17-33":n,…`;手工拼装保证键序,
    /// 与导出报告 JSON 的稳定形态一致)。
    pub fn to_json(&self) -> String {
        format!(
            "{{\"<=16\":{},\"17-33\":{},\"34-66\":{},\"67-133\":{},\">133\":{}}}",
            self.le16, self.r17_33, self.r34_66, self.r67_133, self.gt133
        )
    }
}

/// 逐实例统计(硬骨头 #18;截图车道的每个 headless 实例一条)。
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceStats {
    /// 实例序号(= 分段 worker 序号 wi;GIF 内存车道恒 0)。
    pub index: usize,
    /// 本实例渲染帧数。
    pub frames: usize,
    /// 本实例墙钟耗时(秒)。
    pub wall_seconds: f64,
    /// 本实例实测吞吐(帧/秒 = frames / wall_seconds)。
    pub fps: f64,
    /// 帧间隔分布(判读字段,见 [`FrameIntervalStats`] 注释)。
    pub frame_interval_ms: FrameIntervalStats,
}

impl InstanceStats {
    /// 稳定形态单行 JSON(并入 kiln-cli 结果 JSON 的 `instances` 数组)。
    pub fn to_json(&self) -> String {
        let fi = &self.frame_interval_ms;
        format!(
            "{{\"index\":{},\"frames\":{},\"wall_seconds\":{:.3},\"fps\":{:.2},\"frame_interval_ms\":{{\"p50\":{:.1},\"p95\":{:.1},\"p99\":{:.1},\"max\":{:.1},\"histogram\":{}}}}}",
            self.index,
            self.frames,
            self.wall_seconds,
            self.fps,
            fi.p50,
            fi.p95,
            fi.p99,
            fi.max,
            fi.histogram.to_json(),
        )
    }

    /// `instances` 数组 JSON(空 → `[]`)。
    pub fn array_json(instances: &[InstanceStats]) -> String {
        if instances.is_empty() {
            return "[]".into();
        }
        let items: Vec<String> = instances.iter().map(|s| s.to_json()).collect();
        format!("[{}]", items.join(","))
    }
}

/// 帧间隔样本 → 分位数 + 直方图(纯函数;空样本返回 None)。
///
/// 分位数取**最近秩(nearest-rank)**:p = 有序样本第 `ceil(p*n)` 个。
/// n=1 时三个分位与 max 全等于该样本。输入不改(内部排序副本)。
pub fn summarize_intervals(intervals_ms: &[f64]) -> Option<FrameIntervalStats> {
    if intervals_ms.is_empty() {
        return None;
    }
    let mut sorted = intervals_ms.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let pick = |p: f64| -> f64 {
        let rank = ((p * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
        sorted[rank - 1]
    };
    let mut histogram = IntervalHistogram::default();
    for &v in intervals_ms {
        histogram.bucket(v);
    }
    Some(FrameIntervalStats {
        p50: pick(0.50),
        p95: pick(0.95),
        p99: pick(0.99),
        max: sorted[sorted.len() - 1],
        histogram,
    })
}

/// 文本摘要(图层名;超 12 字截断)。
pub fn brief_text(t: &str) -> String {
    let t = t.trim();
    if t.chars().count() <= 12 {
        t.to_string()
    } else {
        let out: String = t.chars().take(12).collect();
        format!("{out}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 60/30/15/7.5fps 标称间隔 → 应各落本桶(桶界带容差)。
    #[test]
    fn histogram_buckets_catch_nominal_fps_cadences() {
        let mut h = IntervalHistogram::default();
        for v in [16.667, 16.0, 1.0] {
            h.bucket(v);
        }
        assert_eq!(h.le16, 3);
        for v in [17.0, 33.333, 20.0] {
            h.bucket(v);
        }
        assert_eq!(h.r17_33, 3);
        for v in [34.0, 66.667] {
            h.bucket(v);
        }
        assert_eq!(h.r34_66, 2);
        for v in [67.0, 133.333] {
            h.bucket(v);
        }
        assert_eq!(h.r67_133, 2);
        h.bucket(133.4);
        h.bucket(1000.0);
        assert_eq!(h.gt133, 2);
        assert_eq!(h.total(), 12);
    }

    #[test]
    fn histogram_json_has_stable_key_order() {
        let h = IntervalHistogram {
            le16: 1,
            r17_33: 2,
            r34_66: 3,
            r67_133: 4,
            gt133: 5,
        };
        assert_eq!(
            h.to_json(),
            "{\"<=16\":1,\"17-33\":2,\"34-66\":3,\"67-133\":4,\">133\":5}"
        );
    }

    #[test]
    fn summarize_empty_is_none_single_is_flat() {
        assert!(summarize_intervals(&[]).is_none(), "空输入必须 None");
        let s = summarize_intervals(&[42.0]).unwrap();
        assert_eq!((s.p50, s.p95, s.p99, s.max), (42.0, 42.0, 42.0, 42.0));
        assert_eq!(s.histogram.total(), 1);
    }

    #[test]
    fn summarize_nearest_rank_percentiles() {
        // 1..=100:nearest-rank p50=ceil(0.5*100)=50 个 → 50;p95=95;p99=99
        let data: Vec<f64> = (1..=100).map(f64::from).collect();
        let s = summarize_intervals(&data).unwrap();
        assert_eq!(s.p50, 50.0);
        assert_eq!(s.p95, 95.0);
        assert_eq!(s.p99, 99.0);
        assert_eq!(s.max, 100.0);
        // 4 样本 [10,20,30,40]:p50 = ceil(0.5*4)=2 个 → 20;p95 = ceil(3.8)=4 → 40
        let s = summarize_intervals(&[40.0, 10.0, 30.0, 20.0]).unwrap();
        assert_eq!(s.p50, 20.0);
        assert_eq!(s.p95, 40.0);
        assert_eq!(s.max, 40.0);
    }

    #[test]
    fn summarize_input_not_mutated_and_histogram_sums_to_len() {
        let data = vec![200.0, 5.0, 50.0, 25.0, 90.0];
        let snapshot = data.clone();
        let s = summarize_intervals(&data).unwrap();
        assert_eq!(data, snapshot, "输入不得被改写");
        assert_eq!(s.histogram.total(), 5);
        assert_eq!(s.histogram.le16, 1);
        assert_eq!(s.histogram.gt133, 1);
    }

    #[test]
    fn instance_stats_json_matches_report_contract() {
        let st = InstanceStats {
            index: 2,
            frames: 50,
            wall_seconds: 12.3456,
            fps: 4.051,
            frame_interval_ms: summarize_intervals(&[16.667, 200.0, 33.333]).unwrap(),
        };
        let j = st.to_json();
        // 字段与键序 = 任务规格:index/frames/wall_seconds/fps/frame_interval_ms
        assert!(
            j.starts_with("{\"index\":2,\"frames\":50,\"wall_seconds\":12.346,\"fps\":4.05,"),
            "{j}"
        );
        // 有序样本 [16.667, 33.333, 200]:nearest-rank p50 = ceil(1.5)=2 → 33.333
        assert!(j.contains("\"p50\":33.3"), "{j}");
        assert!(j.contains("\"p95\":200.0"), "{j}");
        assert!(j.contains("\"p99\":200.0"), "{j}");
        assert!(j.contains("\"max\":200.0"), "{j}");
        assert!(
            j.contains(
                "\"histogram\":{\"<=16\":1,\"17-33\":1,\"34-66\":0,\"67-133\":0,\">133\":1}"
            ),
            "{j}"
        );
        // 数组与空数组形态
        assert_eq!(InstanceStats::array_json(&[]), "[]");
        let arr = InstanceStats::array_json(&[st]);
        assert!(
            arr.starts_with("[{\"index\":2,") && arr.ends_with("}]"),
            "{arr}"
        );
    }

    #[test]
    fn kiln_report_default_has_no_instances() {
        let r = KilnReport::new();
        assert!(r.instances.is_empty());
        assert_eq!(InstanceStats::array_json(&r.instances), "[]");
    }
}
