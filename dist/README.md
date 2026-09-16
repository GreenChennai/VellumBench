# Kiln-noGUI-CLI 发布包

单文件 Rust 原生导出器(11.6MB,零运行时依赖;MP4/GIF 动画桥可选 ffmpeg)。

## 文件

- `Kiln-noGUI-CLI.exe` — 主程序(同 `cargo build -p vb_kiln --release --bin kiln-cli` 产物)
- `kiln-call.ps1` / `kiln-call.bat` — artboard 调用封装

## WPI 兼容参数面(artboard 可无缝切换)

```
Kiln-noGUI-CLI.exe export --source <html|dir> --output <file> [--format PNG|JPG|GIF|MP4|SVG|PDF|EPS|AI|PPTX]
                          [--width N] [--scale 1|2|4|8] [--transparent] [--max-wait S]
                          [--jpeg-quality 92] [--fps 25] [--duration 2] [--loop 0] [--bitrate 8000]
```

输出:单行 JSON `{"ok":true,"format":"PNG","path":"...","width":2560,"height":1600,...,"engine":"kiln"}`
错误:stderr 单行 JSON `{"ok":false,"error":"..."}`,退出码非 0。

## 自检

```
Kiln-noGUI-CLI.exe selfcheck
# {"ok":true,"engine":"kiln","formats":9,"ms":219}
```

## 与 WPI 的差异

| 项 | WPI | Kiln |
|---|---|---|
| 依赖 | Python+Playwright+浏览器 | 无(单文件) |
| 格式 | PNG/GIF/MP4/PDF | 九格式 |
| PNG@2x | ~4.5s | ~0.5s |
| PDF | 浏览器打印流 | 自研矢量(真文本+OCG) |

构建命令(源码):`cargo build -p vb_kiln --release --bin kiln-cli`
