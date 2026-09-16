//! KilnReport:每次导出的告警与指标(对齐并超越 WPI 可观测性)。

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
    /// 是否降级(MP4→GIF 等)。
    pub degraded: bool,
    /// 渲染引擎标识(溯源)。
    pub engine: &'static str,
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
