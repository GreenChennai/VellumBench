//! 画板声明尺寸探测(P0-3 尺寸门修复)。
//!
//! 浏览器两道(截图车道 B / dom 快照矢量道)此前对「源文件声明了画板尺寸」
//! 毫不知情:视口宽兜底 1080,把 1920 宽内容裁掉 44%(PDF),或让 750 宽
//! 画板被整页拉伸成 1080(PNG)。本模块在采集前从源码侧取得**目标画板的
//! 声明尺寸**,识别口径与 `vb_doc::import` 完全同一(显式 `vb-artboard`
//! 标记 → P0-1 启发式顶层容器 → body 显式约束的合成画板),**不另造第二套
//! 识别**——择道逻辑因此不因「无标记」误判,缺标记只影响 degraded 标注,
//! 不再影响尺寸。
//!
//! 探测失败(打不开/无声明)一律返回 `None` / `None` 尺寸,调用方回退
//! 既有 1080 兜底;本模块绝不panic、绝不阻塞导出。

use std::path::Path;

use vb_doc::import::import_project;

/// 画板声明探测结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtboardProbe {
    /// 画板声明宽(逻辑 px;未声明 = None)。
    pub width: Option<u32>,
    /// 画板声明高(逻辑 px;未声明 = None)。
    pub height: Option<u32>,
    /// true = 显式 `vb-artboard`(含旧前缀)标记;false = 启发式/合成
    /// (degraded_artboard 口径:诚实标注,但不再影响取景尺寸)。
    pub explicit: bool,
    /// 识别到的画板总数(多画板按各自尺寸,本结构为首画板)。
    pub count: usize,
}

/// 探测源的第一个画板声明尺寸。source 与 kiln-cli `--source` 同义
/// (项目目录或单个 HTML 文件)。
pub fn probe_artboard(source: &Path) -> Option<ArtboardProbe> {
    let imported = import_project(source).ok()?;
    let ab = imported.doc.artboards.first().copied()?;
    let n = imported.doc.node(ab)?;
    // authored[2]/[3] = 宽/高是否来自作者声明(显式标记、启发式容器、
    // body 显式约束三者同口径);合成兜底值(1440×900)不算声明。
    let px = |declared: bool, v: f64| declared.then(|| v.round().max(1.0) as u32);
    // 缺任何画板标记告警 ⇔ 显式标记路径(与 kiln-cli 原生车道的判定同串)
    let explicit = !imported.warnings.iter().any(|w| w.contains("画板标记"));
    Some(ArtboardProbe {
        width: px(n.authored[2], n.geom.w),
        height: px(n.authored[3], n.geom.h),
        explicit,
        count: imported.doc.artboards.len(),
    })
}

/// 采集视口计划:显式参数优先 → 画板声明(探测)→ 历史兜底 1080。
///
/// 返回 `(视口宽, 视口高, 画板取景)`。画板取景 = 宽来自画板声明而非显式
/// `--width`(用户显式给宽 = 自定取景,保持 WPI 通用行为,不重置页边距)。
pub fn viewport_plan(source: &Path, width: u32, height: u32) -> (u32, u32, bool) {
    let probe = if width == 0 || height == 0 {
        probe_artboard(source)
    } else {
        None
    };
    let w = if width > 0 {
        width
    } else {
        probe.as_ref().and_then(|p| p.width).unwrap_or(1080)
    };
    let h = if height > 0 {
        height
    } else {
        probe.as_ref().and_then(|p| p.height).unwrap_or(w)
    };
    let artboard = width == 0 && probe.as_ref().is_some_and(|p| p.width.is_some());
    (w, h, artboard)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn untagged_poster_declared_size() {
        // 无标记 + 顶层 .poster 声明 1920×1080(探针 s2-kv 形态)
        let p = probe_artboard(&fixture("p03_untagged")).expect("应识别出画板");
        assert_eq!(p.width, Some(1920));
        assert_eq!(p.height, Some(1080));
        assert!(!p.explicit, "无标记 = 启发式,诚实降级");
        assert_eq!(p.count, 1);
    }

    #[test]
    fn tagged_artboard_explicit() {
        // 显式 vb-artboard 标记 750×1334(examples/poster 形态)
        let p = probe_artboard(&fixture("p03_tagged")).expect("应识别出画板");
        assert_eq!(p.width, Some(750));
        assert_eq!(p.height, Some(1334));
        assert!(p.explicit, "显式标记不算降级");
    }

    #[test]
    fn no_declaration_returns_none_dims() {
        // 纯流式内容:合成画板无声明 → 尺寸 None,调用方回退兜底
        let p = probe_artboard(&fixture("p03_nodecl")).expect("导入应成功");
        assert_eq!(p.width, None);
        assert_eq!(p.height, None);
        assert!(!p.explicit);
    }

    #[test]
    fn missing_source_is_none() {
        assert_eq!(probe_artboard(&fixture("不存在的目录")), None);
    }

    #[test]
    fn viewport_plan_prefers_declared_over_fallback() {
        let src = fixture("p03_untagged");
        // auto(默认 0):声明尺寸接管
        assert_eq!(viewport_plan(&src, 0, 0), (1920, 1080, true));
        // 显式 --width 优先,高度仍取声明;显式给宽 = 自定取景
        assert_eq!(viewport_plan(&src, 800, 0), (800, 1080, false));
        assert_eq!(viewport_plan(&src, 800, 600), (800, 600, false));
    }

    #[test]
    fn viewport_plan_fallback_without_declaration() {
        // 无声明:历史 1080 兜底,非画板取景
        let src = fixture("p03_nodecl");
        assert_eq!(viewport_plan(&src, 0, 0), (1080, 1080, false));
    }
}
