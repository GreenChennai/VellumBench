//! CPU 渲染快照测试(设计文档 10 篇 §四门禁 4 的 v0.1 版):
//! 导入示例落地页 → DrawList → CPU 光栅 → PNG 结构断言。
//! 像素级基线比对随 vello_cpu 统一接入(ADR-0016)。

use std::path::{Path, PathBuf};

use vb_doc::import::import_project;
use vb_render::cpu;

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = crates/vb_render
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn cpu_render_landing_hero_png() {
    let dir = repo_root().join("examples").join("landing");
    let r = import_project(&dir).expect("示例项目应可导入");
    assert!(!r.doc.artboards.is_empty());

    let hero = r.doc.artboards[0];
    let list = vb_render::encode::encode_artboard(&r.doc, hero).expect("编码");
    assert!(list.w > 0.0 && list.h > 0.0);
    assert!(list.items.len() >= 5, "Hero 画板应至少 5 个绘制项");

    let out = cpu::render_png(&list, 1.0, false, Some(&dir)).expect("CPU 渲染");
    // PNG 头
    assert_eq!(&out.png[..8], b"\x89PNG\r\n\x1a\n");
    assert!(out.png.len() > 10_000, "PNG 应有实质内容");

    // @2x:尺寸翻倍
    let out2 = cpu::render_png(&list, 2.0, false, Some(&dir)).expect("CPU 渲染 @2x");
    assert!(out2.png.len() > out.png.len() * 3 / 2, "@2x 内容应显著更多");

    // 解码验证尺寸
    let img = image::load_from_memory(&out2.png).expect("PNG 解码");
    assert_eq!(img.width(), (list.w * 2.0).round() as u32);
    assert_eq!(img.height(), (list.h * 2.0).round() as u32);
}

#[test]
fn cpu_render_text_and_frozen_warn() {
    // 文本 → 占位条 warning;脚本/冻结 → 占位 warning(ADR-0016/0017)
    let dir = std::env::temp_dir().join(format!("vb-render-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("index.html"),
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="UTF-8"><title>t</title></head>
<body>
  <section class="vb-artboard ab" data-vb-id="ab1234" data-vb-name="AB">
    <h1 class="t" data-vb-id="t00001" data-vb-name="标题" style="left: 10px; top: 10px; width: 300px; height: 60px; color: #fff; font-size: 48px;">你好</h1>
    <svg data-vb-id="s00001" viewBox="0 0 1 1"><path d="M0 0"></path></svg>
  </section>
</body>
</html>
"#,
    )
    .unwrap();

    let r = import_project(&dir).unwrap();
    let ab = r.doc.artboards[0];
    let list = vb_render::encode::encode_artboard(&r.doc, ab).unwrap();
    let out = cpu::render_png(&list, 1.0, true, Some(&dir)).unwrap();
    assert!(out.warnings.iter().any(|w| w.contains("占位条")));
    assert!(out.warnings.iter().any(|w| w.contains("冻结块")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_project_writes_files() {
    let dir = std::env::temp_dir().join(format!("vb-export-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("index.html"),
        "<!DOCTYPE html>\n<html><body></body></html>",
    )
    .unwrap();

    let r = import_project(&dir).unwrap();
    let written = vb_doc::export::write_project(&r.doc, &dir).unwrap();
    assert!(written.iter().any(|p| p.ends_with("index.html")));
    assert!(written.iter().any(|p| p.ends_with("main.css")));
    let _ = std::fs::remove_dir_all(&dir);
}
