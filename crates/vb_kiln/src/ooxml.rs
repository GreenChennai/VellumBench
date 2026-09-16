//! PPTX(OOXML)写入器:自研最小 OOXML + stored zip。
//!
//! 结构:[Content_Types].xml + _rels + ppt/presentation.xml + slide +
//! slideLayout + slideMaster + theme。每个 DrawItem → p:sp shape:
//! 文本真实 <a:t>(PowerPoint 可编辑),几何 emu 精确落位,图层 = z 序。

use std::time::Instant;

use vb_render::encode::{DrawItem, DrawKind, FillDef};

use crate::context::ExportContext;
use crate::error::KilnResult;
use crate::report::KilnReport;
use crate::writer::{common_warnings, fill_hex, Format, FormatWriter};

pub struct PptxWriter;

impl FormatWriter for PptxWriter {
    fn format(&self) -> Format {
        Format::Pptx
    }

    fn write(&self, ctx: &ExportContext, out: &mut Vec<u8>) -> KilnResult<KilnReport> {
        let t = Instant::now();
        let warnings = common_warnings(ctx, Format::Pptx);
        let bytes = write_pptx(ctx)?;
        out.extend_from_slice(&bytes);
        let mut r = crate::writer::report_with(warnings, t, bytes.len());
        r.frame_count = 1;
        Ok(r)
    }
}

/// EMU:1px = 9525 EMU(96dpi)。
fn emu(px: f64) -> i64 {
    (px * 9525.0).round() as i64
}

pub fn write_pptx(ctx: &ExportContext) -> KilnResult<Vec<u8>> {
    let w_emu = emu(ctx.logical_w);
    let h_emu = emu(ctx.logical_h);

    let content_types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>
<Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>
<Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/>
<Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/>
<Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>
</Types>"#;

    let root_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>
</Relationships>"#;

    let pres_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>
<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/>
</Relationships>"#;

    let presentation = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:sldMasterIdLst><p:sldMasterId id="2147483648" r:embed="rId2"/></p:sldMasterIdLst>
<p:sldIdLst><p:sldId id="256" r:embed="rId1"/></p:sldIdLst>
<p:sldSz cx="{w}" cy="{h}"/>
<p:notesSz cx="6858000" cy="9144000"/>
</p:presentation>"#,
        w = w_emu,
        h = h_emu
    );

    let master_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/>
</Relationships>"#;

    let slide_master = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill><a:effectLst/></p:bgPr></p:bg><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld>
<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>
<p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:embed="rId1"/></p:sldLayoutIdLst>
</p:sldMaster>"#;

    let slide_layout = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="blank">
<p:cSld name="Blank"><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld>
<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>
</p:sldLayout>"#;

    let layout_rels2 = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
</Relationships>"#;

    let theme = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Kiln">
<a:themeElements>
<a:clrScheme name="Kiln"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2><a:accent1><a:srgbClr val="4472C4"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4><a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme>
<a:fontScheme name="Kiln"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme>
<a:fmtScheme name="Kiln"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst>
</a:themeElements>
</a:theme>"#;

    // slide:DrawList → shape 树
    let slide = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill><a:effectLst/></p:bgPr></p:bg>
<p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name="画板:{name}"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{w}" cy="{h}"/></a:xfrm></p:grpSpPr>{shapes}</p:spTree></p:cSld>
<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>
</p:sld>"#,
        name = xml_escape(&ctx.artboard_name),
        w = w_emu,
        h = h_emu,
        shapes = render_shapes(ctx)
    );

    let slide_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
</Relationships>"#;

    let files: Vec<(&str, String)> = vec![
        ("[Content_Types].xml", content_types.into()),
        ("_rels/.rels", root_rels.into()),
        ("ppt/_rels/presentation.xml.rels", pres_rels.into()),
        ("ppt/presentation.xml", presentation),
        ("ppt/slides/slide1.xml", slide),
        ("ppt/slides/_rels/slide1.xml.rels", slide_rels.into()),
        ("ppt/slideLayouts/slideLayout1.xml", slide_layout.into()),
        ("ppt/slideLayouts/_rels/slideLayout1.xml.rels", layout_rels2.into()),
        ("ppt/slideMasters/slideMaster1.xml", slide_master.into()),
        ("ppt/slideMasters/_rels/slideMaster1.xml.rels", master_rels.into()),
        ("ppt/theme/theme1.xml", theme.into()),
    ];

    Ok(zip_store(&files))
}

fn render_shapes(ctx: &ExportContext) -> String {
    let h = ctx.logical_h;
    let mut shapes = String::with_capacity(16 * 1024);
    for (i, item) in ctx.list.items.iter().enumerate() {
        // PDF 同款 Y 翻转:画布 Y 向下 → 幻灯片 Y 向下(一致,无需翻转)
        // 但基线计算与 PDF 相同以对齐视觉
        shapes.push_str(&draw_item_shape(i + 2, item, h));
    }
    shapes
}

fn draw_item_shape(id: usize, item: &DrawItem, page_h: f64) -> String {
    let [x, y, w, h] = item.rect;
    let (xe, ye, we, he) = (emu(x), emu(y), emu(w), emu(h));
    let name = match item.kind {
        DrawKind::Text => item
            .label
            .as_ref()
            .map(|l| format!("文本:{}", crate::report::brief_text(&l.text)))
            .unwrap_or_else(|| "文本".into()),
        DrawKind::Image => "图片".to_string(),
        DrawKind::VectorPath => "矢量".to_string(),
        DrawKind::Box => "形状".to_string(),
        DrawKind::FrozenPlaceholder => "冻结块".to_string(),
    };
    let mut geom = format!(
        r#"<a:xfrm rot="{}"><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></a:xfrm>"#,
        ((-item.rot * 60000.0).round() as i64).rem_euclid(21600000),
        xe, ye, we, he
    );
    let _ = page_h;

    let (prst, fill_xml, ln_xml) = shape_visual(item);
    let _ = &mut geom;

    let text_xml = if let Some(label) = &item.label {
        let size_hundred = (label.font_size * 100.0).round() as i64;
        let color = rgb_hex(&label.color);
        let bold = if label.weight_bold { " b=\"1\"" } else { "" };
        let text = xml_escape(&label.text);
        format!(
            r#"<p:txBody><a:bodyPr wrap="none" lIns="0" tIns="0" rIns="0" bIns="0" anchor="t"/><a:lstStyle/><a:p><a:r><a:rPr lang="zh-CN" sz="{size}" {bold} dirty="0"><a:solidFill><a:srgbClr val="{color}"/></a:solidFill></a:rPr><a:t>{text}</a:t></a:r></a:p></p:txBody>"#,
            size = size_hundred,
            bold = bold,
            color = color,
            text = text
        )
    } else {
        r#"<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="zh-CN"/></a:p></p:txBody>"#.to_string()
    };

    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{name}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{geom}{prst}{fill}{ln}</p:spPr>{text}</p:sp>"#,
        id = id,
        name = xml_escape(&name),
        geom = geom,
        prst = prst,
        fill = fill_xml,
        ln = ln_xml,
        text = text_xml
    )
}

fn shape_visual(item: &DrawItem) -> (String, String, String) {
    let prst = if item.ellipse {
        r#"<a:prstGeom prst="ellipse"><a:avLst/></a:prstGeom>"#.to_string()
    } else if item.radii.iter().any(|r| *r > 0.0) {
        let adj = (item.radii[0] / (item.rect[2].min(item.rect[3]) / 2.0) * 50000.0).clamp(0.0, 50000.0) as i64;
        format!(
            r#"<a:prstGeom prst="roundRect"><a:avLst><a:gd name="adj" fmla="val {}"/></a:avLst></a:prstGeom>"#,
            adj
        )
    } else {
        r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>"#.to_string()
    };
    let fill = match &item.fill {
        Some(FillDef::Solid(c)) => {
            if item.kind == DrawKind::Text {
                "<a:noFill/>".to_string()
            } else {
                format!(r#"<a:solidFill><a:srgbClr val="{}"/></a:solidFill>"#, rgb_hex(c))
            }
        }
        Some(FillDef::LinearGradient { stops, .. }) | Some(FillDef::RadialGradient { stops, .. }) => {
            let gs: Vec<String> = stops
                .iter()
                .map(|s| {
                    format!(
                        r#"<a:gs pos="{}"><a:srgbClr val="{}"/></a:gs>"#,
                        (s.pos * 100000.0) as i64,
                        rgb_hex(&s.color)
                    )
                })
                .collect();
            format!(
                r#"<a:gradFill><a:gsLst>{}</a:gsLst></a:gradFill>"#,
                gs.join("")
            )
        }
        None => "<a:noFill/>".to_string(),
    };
    let ln = match &item.border {
        Some(b) => format!(
            r#"<a:ln w="{}"><a:solidFill><a:srgbClr val="{}"/></a:solidFill></a:ln>"#,
            emu(b.width),
            rgb_hex(&b.color)
        ),
        None => "<a:ln><a:noFill/></a:ln>".to_string(),
    };
    (prst, fill, ln)
}

fn rgb_hex(c: &[f32; 4]) -> String {
    format!(
        "{:02X}{:02X}{:02X}",
        (c[0] * 255.0) as u8,
        (c[1] * 255.0) as u8,
        (c[2] * 255.0) as u8
    )
}

fn xml_escape(t: &str) -> String {
    t.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// 最小 zip(stored 不压缩;CRC32 手写,零依赖)。
fn zip_store(files: &[(&str, String)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 * 1024);
    let mut central: Vec<u8> = Vec::new();
    let n = files.len() as u16;
    for (i, (name, content)) in files.iter().enumerate() {
        let name_b = name.as_bytes();
        let data = content.as_bytes();
        let crc = crc32(data);
        let offset = out.len() as u32;
        // local file header
        out.extend_from_slice(&0x04034b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version
        out.extend_from_slice(&0u16.to_le_bytes()); // flags (UTF-8 由语言卡;此处无)
        out.extend_from_slice(&0u16.to_le_bytes()); // method stored
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&0x21u16.to_le_bytes()); // date (1980-1-1 合法)
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name_b.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra len
        out.extend_from_slice(name_b);
        out.extend_from_slice(data);
        // central directory entry
        central.extend_from_slice(&0x02014b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0x21u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name_b.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name_b);
        let _ = i;
    }
    let cd_offset = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend_from_slice(&central);
    // end of central directory
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// CRC32(IEEE 802.3;zip 标准表驱动)。
pub fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, e) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB88320 ^ (c >> 1) } else { c >> 1 };
        }
        *e = c;
    }
    let mut crc = 0xFFFFFFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFFFFFF
}

/// fill_hex 透出给 zip 内部(未用;消 dead_code)。
pub fn _fill_hex(f: &FillDef) -> String {
    fill_hex(f)
}
