//! COUP-01 依赖方向钉子(G-UI7 式,2026-10-05 迭代审查):
//! `vb_layout` 源码必须**零** `vb_render` 引用 —— 布局层不得依赖渲染层。
//!
//! 历史缺陷:vb_layout 仅为 `measure_text_weighted` 一处文本量测反向依赖
//! vb_render,把纯几何布局 crate 拖进 vello/wgpu/swash/fontique 编译面。
//! 修复:文本量测上收为共同底层 `vb_textmeasure`(ADR-0053)。
//! 本测试在源码文本层钉死方向,防止回潮(编译期 Cargo.toml 缺依赖已
//! 会报错,这里是显式人读断言 + 覆盖注释/文档字符串里的软引用)。
//!
//! 注:`Cargo.toml` 的 `[dev-dependencies]` 允许 vb_render —— 仅
//! `artboard_declared_height.rs` 跨层几何回归使用(布局结果必须以渲染
//! 视角复验);库本体依赖方向不受影响。

use std::path::Path;

#[test]
fn layout_sources_never_reference_vb_render() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        let entries =
            std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("读目录 {}: {e}", dir.display()));
        for entry in entries {
            let p = entry.unwrap_or_else(|e| panic!("目录项: {e}")).path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if p.extension().and_then(|x| x.to_str()) != Some("rs") {
                continue;
            }
            let text = std::fs::read_to_string(&p)
                .unwrap_or_else(|e| panic!("读源码 {}: {e}", p.display()));
            if text.contains("vb_render") {
                offenders.push(p.display().to_string());
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "COUP-01:布局层源码不得引用渲染层(vb_render),违例: {offenders:?}"
    );
}

#[test]
fn layout_manifest_library_deps_exclude_vb_render() {
    let manifest =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
            .expect("读 Cargo.toml");
    // 只查 [dependencies] 段(dev-dependencies 允许跨层几何回归,见文件头)
    let deps = manifest
        .split("[dev-dependencies]")
        .next()
        .expect("manifest 非空");
    assert!(
        !deps.contains("vb_render"),
        "COUP-01:[dependencies] 不得含 vb_render(布局不依赖渲染)"
    );
}
