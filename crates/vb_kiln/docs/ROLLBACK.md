# Kiln 回滚手册(WPI 一键回退)

## 回退开关(无需改代码)

```bash
# Windows(cmd)
set VB_EXPORT_ENGINE=wpi

# PowerShell
$env:VB_EXPORT_ENGINE = "wpi"
```

- 生效范围:PDF / GIF / MP4 三种格式回退到 WPI 浏览器路径
  (PNG/JPG/SVG/EPS/Ai/PPTX 仍走 Kiln —— WPI 本就不支持这些格式)
- 前置条件:`VB_WPI_DIR` 指向 WPI 仓库(或默认路径
  `E:\平日资料\GitHub\WPI` 存在 `src/cli.py`),Python + playwright 可用
- 清除开关(恢复 Kiln 默认):`set VB_EXPORT_ENGINE=`(空)

## 回退语义(有回归测试锁定)

`crates/vb_kiln/tests/kiln_smoke.rs::rollback_switch_semantics`
- 未设置环境变量 ⇒ Kiln 接管全部九格式
- `VB_EXPORT_ENGINE=wpi` ⇒ 三种浏览器格式回退

## 代码级回滚(极端情况)

vb_kiln 是独立 crate,不影响既有功能;若需整包回退到 v0.3 导出行为:

```bash
git checkout v0.3.0 -- crates/vb_app/src/app.rs
git checkout v0.3.0 -- crates/vb_app/Cargo.toml
cargo build -p vb_app --release
```

WPI 模块本体(`crates/vb_export/src/wpi.rs`)在 Kiln 合入前后未做任何
修改,旧路径行为与 v0.3 完全一致。

## 演练记录

2026-09-16 本机演练:
1. `VB_EXPORT_ENGINE=wpi` 设置后开关判定为回退 ✅
2. WPI CLI 实跑 PNG(4467ms)/GIF(3529ms)/MP4(2739ms)/PDF(4789ms)全通 ✅
3. 清除环境变量后恢复 Kiln 默认 ✅
4. 语义回归测试 `rollback_switch_semantics` 通过 ✅
