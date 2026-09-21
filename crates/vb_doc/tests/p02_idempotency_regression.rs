//! P0-2 往返幂等回归(08b 报告 §3/§4 固化)。
//!
//! 根因:`finalize_classes` 的「主类冲突只给后撞者插生成类」旧规则 ——
//! 共享类被文档序首个节点占用为规则选择器,而规则内容是该节点**合并后的
//! 私有样式**;重导入时经 CSS 级联串写给其他携带该类的兄弟(opacity 串写),
//! Frozen 空元素被造出的类没有 HTML 载体,每轮成为孤儿规则再重新生成
//! (规则增殖)。
//!
//! 修复 = 选择器唯一性升级为「类成员级全局唯一」(冲突主类由唯一类承载后
//! 从成员上移除)+ Frozen 节点一律不造类。本文件固化两组最小复现。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
type Files = Vec<(String, String)>;

use vb_doc::export::render_project;
use vb_doc::import::import_project;

fn unique_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("vb-p02-{}-{tag}-{n}", std::process::id()))
}

/// 导入 → 导出(落盘)→ 再导入 → 再导出,返回两轮文件表。
fn roundtrip(html: &str) -> (Files, Files) {
    let dir = unique_dir("rt");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), html).unwrap();
    let r1 = import_project(&dir).unwrap();
    let out1 = render_project(&r1.doc);
    for (rel, content) in &out1.files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
    }
    let r2 = import_project(&dir).unwrap();
    let out2 = render_project(&r2.doc);
    let _ = std::fs::remove_dir_all(&dir);
    (out1.files, out2.files)
}

fn css(files: &[(String, String)]) -> String {
    files
        .iter()
        .find(|(p, _)| p == "styles/main.css")
        .map(|(_, c)| c.clone())
        .unwrap_or_default()
}

/// 症状 2:opacity 串写 —— 共享类 `.orbit` 被首节点(o3,带 opacity:0.45)
/// 占用为规则选择器;o1(无 opacity)重导入时经级联串写。修复后:冲突主类
/// 由唯一类承载并从成员上移除,o1 的规则不含 opacity,两轮 CSS 字节一致。
#[test]
fn p02_shared_class_opacity_does_not_leak() {
    let html = r##"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="utf-8"><title>轨道</title></head>
<body>
<section class="vb-artboard" data-vb-id="aa0000" data-vb-name="画板 1" style="position: relative; width: 800px; height: 600px">
  <div class="vb-orbit o3" data-vb-id="aa0003" data-vb-name="环三" style="position: absolute; left: 100px; top: 100px; width: 200px; height: 200px; opacity: 0.45"></div>
  <div class="vb-orbit o1" data-vb-id="aa0001" data-vb-name="环一" style="position: absolute; left: 300px; top: 100px; width: 560px; height: 560px"></div>
  <div class="vb-orbit o2" data-vb-id="aa0002" data-vb-name="环二" style="position: absolute; left: 100px; top: 300px; width: 400px; height: 400px"></div>
</section>
</body>
</html>
"##;
    let (f1, f2) = roundtrip(html);
    for (a, b) in f1.iter().zip(f2.iter()) {
        assert_eq!(a.1, b.1, "{} 两轮导出字节不同(共享类串写回归)", a.0);
    }
    let css1 = css(&f1);
    // 每个成员的规则内容互不串写:o1(.vb-el-aa0001,560px)不含 o3 的 opacity
    let start = css1.find(".vb-el-aa0001 {").expect("o1 规则存在");
    let end = css1[start..].find('}').unwrap() + start;
    let o1_block = &css1[start..=end];
    assert!(o1_block.contains("560px"), "o1 规则定位:{o1_block}");
    assert!(
        !o1_block.contains("opacity"),
        "o1 规则不得携带他人 opacity:{o1_block}"
    );
}

/// 症状 1:Frozen 空元素(`<hr>`)造类 → 孤儿规则增殖(1 → 2 → 3)。
/// 修复后:Frozen 一律不造类,规则数恒定。
#[test]
fn p02_frozen_empty_element_rule_does_not_multiply() {
    let html = r##"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="utf-8"><title>分隔</title></head>
<body>
<section class="vb-artboard" data-vb-id="aa0000" data-vb-name="画板 1" style="position: relative; width: 400px; height: 300px">
  <div class="box" data-vb-id="aa0001" data-vb-name="文块" style="position: absolute; left: 20px; top: 20px; width: 320px; height: 200px">第一段
<hr>
第二段</div>
</section>
</body>
</html>
"##;
    let dir = unique_dir("hr");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), html).unwrap();

    let mut counts = Vec::new();
    let mut prev_css = String::new();
    for round in 0..3 {
        let r = import_project(&dir).unwrap();
        let out = render_project(&r.doc);
        for (rel, content) in &out.files {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, content).unwrap();
        }
        let c = css(&out.files);
        let n = c.matches(".hr-").count();
        counts.push(n);
        if round > 0 {
            assert_eq!(
                c,
                prev_css,
                "第 {} 轮 CSS 与上轮不同(规则增殖回归;计数 {counts:?})",
                round + 1
            );
        }
        prev_css = c;
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        counts.iter().all(|&n| n == counts[0]),
        "Frozen 规则数应恒定: {counts:?}"
    );
}

/// Frozen 原生类与普通节点同名时的占名(08e 复核补):`<svg class="logo">` 是
/// Frozen 裸片段,不能改名 —— 它必须**占住** `logo` 这个规则选择器名,迫使
/// 同名的普通 `<div class="logo">` 改挂唯一类。否则两条规则同选择器(重复 +
/// 折叠),重导入后 Frozen 也拿到该样式,下一轮多出一条重复规则(两步收敛)。
#[test]
fn p02_frozen_class_occupies_selector_name() {
    let html = r##"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="utf-8"><title>冻结类占名</title></head>
<body>
<section class="vb-artboard" data-vb-id="aa0000" data-vb-name="画板 1" style="position: relative; width: 400px; height: 300px">
  <svg class="logo" width="20" height="20" data-vb-id="aa0001"><rect width="10" height="10" fill="#0a0"/></svg>
  <div class="logo" data-vb-id="aa0002" data-vb-name="牌" style="position: absolute; left: 10px; top: 10px; width: 50px; height: 50px; background-color: #f00"></div>
</section>
</body>
</html>
"##;
    let dir = unique_dir("frozen-cls");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), html).unwrap();

    let mut prev_css = String::new();
    let mut prev_html = String::new();
    for round in 0..3 {
        let r = import_project(&dir).unwrap();
        let out = render_project(&r.doc);
        for (rel, content) in &out.files {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, content).unwrap();
        }
        let c = css(&out.files);
        let html_out = out
            .files
            .iter()
            .find(|(p, _)| p == "index.html")
            .map(|(_, c)| c.clone())
            .unwrap_or_default();
        // 选择器不得重复(两条同名规则会互相折叠 → 下一轮增殖)
        let mut selectors: Vec<&str> = c.lines().filter(|l| l.ends_with(" {")).collect();
        let total = selectors.len();
        selectors.sort_unstable();
        selectors.dedup();
        assert_eq!(
            total,
            selectors.len(),
            "第 {} 轮出现重复规则选择器:{c}",
            round + 1
        );
        if round > 0 {
            assert_eq!(c, prev_css, "第 {} 轮 CSS 与上轮不同", round + 1);
            assert_eq!(html_out, prev_html, "第 {} 轮 HTML 与上轮不同", round + 1);
        }
        prev_css = c;
        prev_html = html_out;
    }
    let _ = std::fs::remove_dir_all(&dir);
    // 普通节点必须让出 `logo` 选择器(改挂唯一类),文档里只可能有 1 条 `.logo` 规则
    assert!(
        prev_css.matches(".logo {").count() <= 1,
        "`.logo` 规则至多一条:{prev_css}"
    );
}
