//! 往返调试工具:对一个 HTML 文件跑两次导入→导出,打印不一致的文件。
//!
//! ```bash
//! cargo run -p vb_doc --example roundtrip_dump -- crates/vb_doc/tests/corpus/06-text-inline-mix.html
//! ```
//!
//! 语料门禁(`tests/corpus.rs`)报 L1 幂等失败时,用它看两次导出的完整差异。

use std::path::Path;

use vb_doc::export::render_project;
use vb_doc::import::import_project;

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("用法:roundtrip_dump <html 文件>");
        std::process::exit(2);
    };
    let src = std::fs::read_to_string(&path).expect("读不到源文件");
    let dir = std::env::temp_dir().join("vb-rt-dump");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), &src).unwrap();

    let r1 = import_project(&dir).unwrap();
    println!("=== 导入 1 警告 ===");
    for w in &r1.warnings {
        println!("  ! {w}");
    }
    let out1 = render_project(&r1.doc);
    for (rel, content) in &out1.files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
    }

    let r2 = import_project(&dir).unwrap();
    println!("=== 导入 2 警告 ===");
    for w in &r2.warnings {
        println!("  ! {w}");
    }
    println!("=== 导入 2 raw_css(共 {}) ===", r2.doc.raw_css.len());
    for b in &r2.doc.raw_css {
        println!("  RAW> {}", b.replace('\n', "\\n"));
    }
    println!("=== 导入 2 节点 class ===");
    let mut ids = Vec::new();
    for &ab in &r2.doc.artboards {
        r2.doc.subtree(ab, &mut ids);
    }
    for id in ids {
        if let Some(n) = r2.doc.nodes.get(id) {
            println!(
                "  {:>10} classes={:?} tag={:?} name={:?} geom=({},{},{},{})",
                n.sid.as_str(),
                n.classes,
                n.tag,
                n.name,
                n.geom.x,
                n.geom.y,
                n.geom.w,
                n.geom.h
            );
        }
    }
    let out2 = render_project(&r2.doc);

    let name = Path::new(&path).file_name().unwrap().to_string_lossy();
    println!("=== {name} ===");
    if std::env::var("VB_DUMP_SRC").is_ok() {
        for (rel, c) in &out1.files {
            println!("--- 第 1 次导出 {rel} ---\n{c}");
        }
    }
    for (rel, c1) in &out1.files {
        let c2 = out2
            .files
            .iter()
            .find(|(p, _)| p == rel)
            .map(|(_, c)| c.as_str())
            .unwrap_or("<缺失>");
        if c1 == c2 {
            println!("[OK]   {rel}({} 字节)", c1.len());
        } else {
            println!("[DIFF] {rel}");
            println!("--- 第 1 次导出 ---\n{c1}");
            println!("--- 第 2 次导出 ---\n{c2}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
