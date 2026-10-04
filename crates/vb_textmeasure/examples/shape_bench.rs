//! PERF-01/DOC-09 基准:1000 字符文本整形(前/后对比)。
//!
//! 运行:`cargo run -p vb_textmeasure --example shape_bench --release`
//!
//! 场景 = 真实热路径近似:同一 1000 字符文本在「布局量测 + 渲染」阶段
//! 会被反复整形(布局若干次、CPU 导出若干次),故按固定次数重复整形同一
//! 文本,报告总量与均值。字段:热身 1 次(计入总量,体现首排成本)。

use std::time::Instant;

use vb_textmeasure::{measure_text_weighted, shape_text_weighted};

fn bench_text() -> String {
    // 1000 字符:CJK 为主混拉丁/数字/标点,贴近真实稿件
    let unit = "绘台基准文本:VellumBench 渲染管线 PERF-01——字体解析、整形与断行(2026)。";
    let mut s = String::new();
    while s.chars().count() < 1000 {
        s.push_str(unit);
    }
    s.chars().take(1000).collect()
}

fn main() {
    let text = bench_text();
    let n_chars = text.chars().count();
    let iters = 30;
    eprintln!("文本 {n_chars} 字符 × {iters} 次整形");

    let t0 = Instant::now();
    let mut glyph_total = 0usize;
    for _ in 0..iters {
        let run = shape_text_weighted(&text, "Microsoft YaHei", 24.0, 400)
            .expect("系统字体 Microsoft YaHei 应可解析");
        glyph_total += run.glyphs.len();
    }
    let shape_total = t0.elapsed();

    let t1 = Instant::now();
    let mut line_total = 0usize;
    for _ in 0..iters {
        let (_w, lines) = measure_text_weighted(&text, "Microsoft YaHei", 24.0, 400, 640.0, 0.0);
        line_total += lines;
    }
    let measure_total = t1.elapsed();

    // 校验输出稳定(防止基准空转):字形数与行数必须一致
    eprintln!("字形总数 {glyph_total} 行总数 {line_total}(每轮应相同)");
    println!(
        "PERF-BENCH shape: total_ms={} avg_ms={:.2} | measure: total_ms={} avg_ms={:.2}",
        shape_total.as_millis(),
        shape_total.as_millis() as f64 / iters as f64,
        measure_total.as_millis(),
        measure_total.as_millis() as f64 / iters as f64,
    );
}
