//! 字符/段落面板 · 字段清单(design/03 §5.10 落位表;门 1 真相表)
//! 与 px/combo 字段规格常量。
//!
//! 06-1 自 `panels/charpara.rs` 按职责拆出(纯搬移,零行为变化)。

use vb_css::Decl;
use vb_doc::model::Document;

use super::model::decl_px;

// ═══════════════════ 4. 字段清单(design/03 §5.10 落位表;门 1 真相表) ═══════════════════

/// 节点 style 上的 px 数值取值(font-size / line-height / letter-spacing …)。
pub(super) fn style_font_px(style: &[Decl], prop: &str) -> Option<f64> {
    decl_px(style, prop)
}

/// px 数值字段的静态规格(消参数爆炸;`prop` 同时是 run 字段分派键)。
pub(super) struct PxFieldSpec {
    pub(super) label: &'static str,
    pub(super) prop: &'static str,
    pub(super) lo: f64,
    pub(super) hi: f64,
}

pub(super) const SIZE_SPEC: PxFieldSpec = PxFieldSpec {
    label: "大小",
    prop: "font-size",
    lo: 1.0,
    hi: 1000.0,
};
pub(super) const LH_SPEC: PxFieldSpec = PxFieldSpec {
    label: "行距",
    prop: "line-height",
    lo: 1.0,
    hi: 2000.0,
};
pub(super) const TRACK_SPEC: PxFieldSpec = PxFieldSpec {
    label: "字距",
    prop: "letter-spacing",
    lo: -50.0,
    hi: 500.0,
};
pub(super) const INDENT_L_SPEC: PxFieldSpec = PxFieldSpec {
    label: "左缩进",
    prop: "padding-left",
    lo: 0.0,
    hi: 1000.0,
};
pub(super) const INDENT_R_SPEC: PxFieldSpec = PxFieldSpec {
    label: "右缩进",
    prop: "padding-right",
    lo: 0.0,
    hi: 1000.0,
};
pub(super) const INDENT_FIRST_SPEC: PxFieldSpec = PxFieldSpec {
    label: "首行缩",
    prop: "text-indent",
    lo: -200.0,
    hi: 1000.0,
};
pub(super) const SPACE_BEFORE_SPEC: PxFieldSpec = PxFieldSpec {
    label: "段前",
    prop: "margin-top",
    lo: 0.0,
    hi: 2000.0,
};
pub(super) const SPACE_AFTER_SPEC: PxFieldSpec = PxFieldSpec {
    label: "段后",
    prop: "margin-bottom",
    lo: 0.0,
    hi: 2000.0,
};

/// 下拉字段的静态规格。
pub(super) struct ComboSpec {
    pub(super) label: &'static str,
    pub(super) salt: &'static str,
    pub(super) prop: &'static str,
    /// (值, 显示名);首项语义必须是「默认/无」= 移除声明。
    pub(super) options: &'static [(&'static str, &'static str)],
}

pub(super) const KINSOKU_SPEC: ComboSpec = ComboSpec {
    label: "避头尾",
    salt: "para_kinsoku",
    prop: "line-break",
    options: &[
        ("auto", "自动"),
        ("loose", "宽松"),
        ("normal", "一般"),
        ("strict", "严格"),
    ],
};
pub(super) const HYPHENS_SPEC: ComboSpec = ComboSpec {
    label: "连字",
    salt: "para_hyphens",
    prop: "hyphens",
    options: &[("manual", "手动(默认)"), ("auto", "自动"), ("none", "关")],
};
pub(super) const PUNCT_SPEC: ComboSpec = ComboSpec {
    label: "标点挤压",
    salt: "para_punct",
    prop: "hanging-punctuation",
    options: &[
        ("none", "无"),
        ("allow-end", "允许末行悬挂"),
        ("force-end", "强制末行悬挂"),
    ],
};

/// 文档中已使用的字体族(字符面板「文档已有」快选;扫描节点声明去重)。
pub(super) fn doc_font_families(doc: &Document) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (_, n) in doc.nodes.iter() {
        if let Some(f) = n.style_get("font-family") {
            let f = f.trim().to_string();
            if !f.is_empty() && !out.contains(&f) {
                out.push(f);
            }
        }
    }
    out
}

#[cfg(test)]
pub(super) mod field_tables {
    /// 字符面板字段清单:(字段 id,design/03 §5.10 字段名)。
    /// 冻结登记项以 `frozen.` 前缀标识 —— 只做 caption,不做假控件。
    pub(crate) const CHAR_FIELDS: &[(&str, &str)] = &[
        ("char.family", "字体"),
        ("char.bold", "样式(粗体)"),
        ("char.italic", "样式(斜体)"),
        ("char.size", "大小"),
        ("char.line_height", "行距"),
        ("char.tracking", "字距"),
        ("char.baseline", "基线偏移"),
        ("char.underline", "下划线"),
        ("char.strike", "删除线"),
        ("char.lang", "语言"),
        ("char.smoothing", "抗锯齿"),
        ("char.frozen.kerning", "字偶距"),
        ("char.frozen.vscale", "垂直缩放"),
        ("char.frozen.hscale", "水平缩放"),
        ("char.frozen.rotation", "字符旋转"),
    ];

    /// 段落面板字段清单。
    pub(crate) const PARA_FIELDS: &[(&str, &str)] = &[
        ("para.align", "对齐(9 式)"),
        ("para.indent_l", "左缩进"),
        ("para.indent_r", "右缩进"),
        ("para.indent_first", "首行缩进"),
        ("para.space_before", "段前"),
        ("para.space_after", "段后"),
        ("para.kinsoku", "避头尾"),
        ("para.hyphens", "连字"),
        ("para.punct_squeeze", "标点挤压"),
        ("para.area_fit", "区域溢出·自动扩高"),
    ];
}
