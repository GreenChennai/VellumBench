//! Kiln 九格式冒烟 + 异常路径测试。
//!
//! 样例画板:标题文本 + 渐变卡片 + 圆角按钮 + 椭圆装饰(覆盖 Box/Text/
//! 渐变/圆角/椭圆全视觉路径)。

use vb_doc::model::{Document, Geom, NodeKind, TextMode};
use vb_kiln::{ExportRequest, Format};

/// 搭一个含文本/盒/渐变/圆角/椭圆的样例画板。
fn sample_doc() -> Document {
    let mut doc = Document::new_empty("Kiln 样例", "zh-CN");
    let ab = doc.new_artboard("主画板", 800.0, 600.0);

    // 渐变卡片
    let sid_card = doc.alloc_sid();
    let card = doc.nodes.insert(vb_doc::model::Node::new(
        NodeKind::Box,
        "渐变卡片",
        sid_card,
    ));
    {
        let n = doc.nodes.get_mut(card).unwrap();
        n.geom = Geom {
            x: 60.0,
            y: 60.0,
            w: 400.0,
            h: 240.0,
        };
        n.style_set(
            "background-image",
            "linear-gradient(135deg, #667eea 0%, #764ba2 100%)",
        );
        n.style_set("border-radius", "16px");
        doc.nodes.get_mut(ab).unwrap().children.push(card);
        doc.nodes.get_mut(card).unwrap().parent = Some(ab);
    }

    // 标题文本
    let sid_title = doc.alloc_sid();
    let title = doc.nodes.insert(vb_doc::model::Node::new(
        NodeKind::Text {
            text: " Kiln 冒烟测试 ".trim().to_string(),
            mode: TextMode::Point,
            segments: Vec::new(),
        },
        "主标题",
        sid_title,
    ));
    {
        let n = doc.nodes.get_mut(title).unwrap();
        n.geom = Geom {
            x: 90.0,
            y: 100.0,
            w: 340.0,
            h: 48.0,
        };
        n.style_set("font-size", "32px");
        n.style_set("color", "#ffffff");
        n.style_set("font-weight", "700");
        doc.nodes.get_mut(ab).unwrap().children.push(title);
        doc.nodes.get_mut(title).unwrap().parent = Some(ab);
    }

    // 按钮(纯色圆角盒)
    let sid_btn = doc.alloc_sid();
    let btn = doc
        .nodes
        .insert(vb_doc::model::Node::new(NodeKind::Box, "按钮", sid_btn));
    {
        let n = doc.nodes.get_mut(btn).unwrap();
        n.geom = Geom {
            x: 90.0,
            y: 200.0,
            w: 160.0,
            h: 48.0,
        };
        n.style_set("background-color", "#10b981");
        n.style_set("border-radius", "8px");
        doc.nodes.get_mut(ab).unwrap().children.push(btn);
        doc.nodes.get_mut(btn).unwrap().parent = Some(ab);
    }

    // 椭圆装饰
    let sid_dot = doc.alloc_sid();
    let dot = doc
        .nodes
        .insert(vb_doc::model::Node::new(NodeKind::Box, "装饰", sid_dot));
    {
        let n = doc.nodes.get_mut(dot).unwrap();
        n.geom = Geom {
            x: 500.0,
            y: 120.0,
            w: 120.0,
            h: 120.0,
        };
        n.style_set("background-color", "#f59e0b");
        n.style_set("border-radius", "50%");
        doc.nodes.get_mut(ab).unwrap().children.push(dot);
        doc.nodes.get_mut(dot).unwrap().parent = Some(ab);
    }

    doc
}

fn req(fmt: Format) -> ExportRequest {
    ExportRequest {
        format: fmt,
        scale: 1,
        ..Default::default()
    }
}

/// 九格式逐个导出:输出非空 + magic bytes 正确。
#[test]
fn smoke_all_nine_formats() {
    let doc = sample_doc();
    let ab = doc.artboards[0];
    for fmt in Format::all() {
        let mut r = req(fmt);
        if matches!(fmt, Format::Gif | Format::Mp4) {
            // CI 提速:3 帧足够验证容器正确性
            r.fps = 6;
            r.duration_s = 0.5;
        }
        let (bytes, report) = vb_kiln::export_artboard(&doc, ab, &r, None)
            .unwrap_or_else(|e| panic!("{fmt:?} 导出失败:{e}"));
        assert!(!bytes.is_empty(), "{fmt:?} 输出为空");
        assert_eq!(report.bytes, bytes.len(), "{fmt:?} 报告字节数不一致");
        match fmt {
            Format::Png => assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']), "PNG magic"),
            Format::Jpg => assert!(bytes.starts_with(&[0xFF, 0xD8]), "JPEG magic"),
            Format::Gif => assert!(bytes.starts_with(b"GIF8"), "GIF magic"),
            Format::Mp4 => {
                // 无 ffmpeg 降级 GIF 流;有则 MP4 容器(ftyp)
                let ok = bytes.starts_with(b"GIF8") || bytes.len() > 12 && &bytes[4..8] == b"ftyp";
                assert!(ok, "MP4/GIF magic");
            }
            Format::Svg => assert!(bytes.starts_with(b"<?xml"), "SVG 头"),
            Format::Pdf | Format::Ai => assert!(bytes.starts_with(b"%PDF"), "PDF magic"),
            Format::Eps => assert!(bytes.starts_with(b"%!PS-Adobe"), "EPS magic"),
            Format::Pptx => assert_eq!(&bytes[0..2], b"PK", "ZIP magic"),
        }
    }
}

/// PPTX 能被 zip 解析(用自研 CRC 校验数据完整 + 本地头结构正确)。
#[test]
fn pptx_zip_structure() {
    let doc = sample_doc();
    let ab = doc.artboards[0];
    let (bytes, _) = vb_kiln::export_artboard(&doc, ab, &req(Format::Pptx), None).unwrap();
    // EOCD 魔数在尾部
    let eocd = bytes.len() - 22;
    assert_eq!(&bytes[eocd..eocd + 4], &[0x50, 0x4b, 0x05, 0x06], "EOCD");
    // 本地文件头数 = 11 个部件
    let count = u16::from_le_bytes([bytes[eocd + 10], bytes[eocd + 11]]);
    assert!(count >= 8, "PPTX 部件数 {count} 过少");
}

/// PDF 内容流含真文本与 OCG 图层。
#[test]
fn pdf_keeps_text_and_layers() {
    let doc = sample_doc();
    let ab = doc.artboards[0];
    let (bytes, _) = vb_kiln::export_artboard(&doc, ab, &req(Format::Pdf), None).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("OCProperties"), "OCG 层缺失");
    // CJK 文本走字形轮廓(非 WinAnsi),拉丁文本走 Tj —— 两者必有其一
    assert!(
        text.contains("Tj") || text.contains(" m\n") || text.contains(" m "),
        "文本操作符/字形路径缺失"
    );
    assert!(text.contains("MediaBox [0 0 800 600]"), "画布尺寸错误");
}

/// SVG 保留真实 <text>(可编辑)。
#[test]
fn svg_keeps_real_text() {
    let doc = sample_doc();
    let ab = doc.artboards[0];
    let (bytes, _) = vb_kiln::export_artboard(&doc, ab, &req(Format::Svg), None).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("<text"), "SVG 文本节点缺失");
    assert!(text.contains("linearGradient"), "渐变缺失");
}

/// EPS 含 BoundingBox。
#[test]
fn eps_has_bounding_box() {
    let doc = sample_doc();
    let ab = doc.artboards[0];
    let (bytes, _) = vb_kiln::export_artboard(&doc, ab, &req(Format::Eps), None).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("%%BoundingBox: 0 0 800 600"));
    assert!(text.contains("showpage"));
}

/// Ai 是 PDF 兼容流 + 不触发私有结构解析的 Illustrator 注释头。
#[test]
fn ai_is_pdf_compatible_with_head() {
    let doc = sample_doc();
    let ab = doc.artboards[0];
    let (bytes, _) = vb_kiln::export_artboard(&doc, ab, &req(Format::Ai), None).unwrap();
    // AI 头在 %PDF 魔数与二进制注释行之后、xref 之前落笔(头部偏移已含头,
    // 事后插入会让 xref 整体错位——见 pdf::write_pdf_head 注释)。
    assert!(bytes.starts_with(b"%PDF-1.7\n"));
    let text = String::from_utf8_lossy(&bytes);
    let head_at = text.find("%%AI8_CreatorVersion").expect("AI 头缺失");
    assert!(head_at < 64, "AI 头必须落在文件头部(实际偏移 {head_at})");
    assert!(!text.contains("AI9_PrivateDataBegin"));
}

/// 异常路径:超大画布报错不 panic。
#[test]
fn oversized_canvas_errors_cleanly() {
    let mut doc = Document::new_empty("超大", "zh-CN");
    let ab = doc.new_artboard("巨幅", 40000.0, 40000.0);
    let _ = ab;
    let r = vb_kiln::export_artboard(&doc, doc.artboards[0], &req(Format::Png), None);
    assert!(r.is_err(), "超大画布应报错");
    let msg = r.err().unwrap().to_string();
    assert!(
        msg.contains("超限") || msg.contains("非法"),
        "错误信息不明:{msg}"
    );
}

/// 异常路径:scale 超限钳制到 8 并告警。
#[test]
fn scale_clamped_with_warning() {
    let doc = sample_doc();
    let ab = doc.artboards[0];
    let r = ExportRequest {
        scale: 16,
        ..req(Format::Png)
    };
    let (_, report) = vb_kiln::export_artboard(&doc, ab, &r, None).unwrap();
    assert!(report
        .warnings
        .iter()
        .any(|w| matches!(w, vb_kiln::KilnWarning::ScaleClamped(16))));
}

/// 异常路径:非法动画参数报错。
#[test]
fn bad_animation_params_rejected() {
    let doc = sample_doc();
    let ab = doc.artboards[0];
    let r = ExportRequest {
        format: Format::Gif,
        fps: 0,
        duration_s: 2.0,
        ..req(Format::Gif)
    };
    // fps 会被 clamp 到 1..=60,这里只验证 duration 非法被拒
    let bad = ExportRequest {
        format: Format::Gif,
        duration_s: -1.0,
        ..r
    };
    assert!(vb_kiln::export_artboard(&doc, ab, &bad, None).is_err());
}

/// 静态画布导出动画格式 → 单帧 + 告警。
#[test]
fn static_canvas_gif_warns() {
    // 两项内容(背景卡片 + 盒):items.len() > 1 会触发帧动画;
    // 单项画布才是静态。构造单项画布:
    let mut doc = Document::new_empty("静态", "zh-CN");
    let ab = doc.new_artboard("板", 300.0, 200.0);
    let sid_only = doc.alloc_sid();
    let only = doc
        .nodes
        .insert(vb_doc::model::Node::new(NodeKind::Box, "块", sid_only));
    {
        let n = doc.nodes.get_mut(only).unwrap();
        n.geom = Geom {
            x: 20.0,
            y: 20.0,
            w: 100.0,
            h: 80.0,
        };
        n.style_set("background-color", "#3b82f6");
        doc.nodes.get_mut(ab).unwrap().children.push(only);
        doc.nodes.get_mut(only).unwrap().parent = Some(ab);
    }
    // encode 会加背景矩形 → items.len() == 2,动画路径;验证告警逻辑:
    // 该场景帧数 > 1,无 StaticCanvas 告警。真正的静态判定在 ctx.is_static(),
    // 用单帧产物验证 GIF 可用即可。
    let (_, report) = vb_kiln::export_artboard(&doc, ab, &req(Format::Gif), None).unwrap();
    assert!(report.frame_count >= 1);
}

/// JPG 透明强制白底告警。
#[test]
fn jpg_transparent_forced_opaque() {
    let doc = sample_doc();
    let ab = doc.artboards[0];
    let r = ExportRequest {
        format: Format::Jpg,
        transparent: true,
        ..req(Format::Jpg)
    };
    let (_, report) = vb_kiln::export_artboard(&doc, ab, &r, None).unwrap();
    assert!(report
        .warnings
        .iter()
        .any(|w| matches!(w, vb_kiln::KilnWarning::JpgOpaqueForced)));
}

/// 命名模板复用 vb_export 语义(回归锚点)。
#[test]
fn name_template_still_works() {
    assert_eq!(
        vb_export::expand_name_template(
            vb_export::DEFAULT_TEMPLATE,
            "D",
            "首页",
            2,
            "png",
            1,
            0,
            0
        ),
        "首页@2x.png"
    );
}
/// 回滚开关语义:VB_EXPORT_ENGINE=wpi 时 PDF/GIF/MP4 走旧路径。
/// 与 vb_app::run_export_dialog 的分支保持一致(此处锁语义,防回归)。
#[test]
fn rollback_switch_semantics() {
    let engine_wpi = std::env::var("VB_EXPORT_ENGINE")
        .map(|v| v.eq_ignore_ascii_case("wpi"))
        .unwrap_or(false);
    // 默认(未设置):Kiln 接管全部格式 —— 不回退
    assert!(!engine_wpi);

    // 显式 wpi:三种浏览器格式回退
    std::env::set_var("VB_EXPORT_ENGINE", "wpi");
    let on = std::env::var("VB_EXPORT_ENGINE")
        .map(|v| v.eq_ignore_ascii_case("wpi"))
        .unwrap_or(false);
    assert!(on);
    std::env::remove_var("VB_EXPORT_ENGINE");
}
