# Kiln-noGUI-CLI 发布包

单文件 Rust 原生导出器(约 18MB,零运行时依赖;MP4/GIF 动画桥可选 ffmpeg)。

## v0.9.0 变更

- **Edge 无头端点探测修复**:Windows 版 msedge 不往 stderr 打印
  `DevTools listening on ws://`,此前浏览器车道在只有 Edge 的机器上必然失败。
  现改为**双路取端口**——stderr 行 + `<user-data-dir>/DevToolsActivePort` 文件,
  任一先到即用。实测 Edge 153 直接可用。
- **降级可见**:浏览器车道失败而落到自研引擎时,结果 JSON 新增
  `engine_fallback:true` 并把 `degraded` 置真——自研引擎对真实海报页是废图
  (丢照片/遮罩/绝对定位),调用方必须看得见,不能 `ok:true` 静默混过去。
- **JSON 转义根修**:结果 JSON 由 `format!` 手拼,Windows 路径里的 `\` 不转义
  会产出非法 JSON,调用方 `json.loads` 必失败、`width/height/engine` 永远取不到。
  现经 `jesc()` 统一转义。
- `export --help` 补环境变量说明:`VB_BROWSER_PATH`(指定浏览器)、`PDFIUM_DLL`。

## 文件

- `Kiln-noGUI-CLI.exe` — 主程序(同 `cargo build -p vb_kiln --release --bin kiln-cli` 产物)
- `kiln-call.ps1` / `kiln-call.bat` — artboard 调用封装

## WPI 兼容参数面(artboard 可无缝切换)

```
Kiln-noGUI-CLI.exe export --source <html|dir> --output <file> [--format PNG|JPG|GIF|MP4|SVG|PDF|EPS|AI|PPTX]
                          [--width N] [--scale 1|2|4|8] [--transparent] [--max-wait S]
                          [--height N] [--engine auto|browser|native] [--vector auto|dom|chrome]
                          [--jpeg-quality 92] [--fps 25] [--duration 2] [--loop 0] [--bitrate 8000]
```

环境变量:`VB_BROWSER_PATH` 指定 Chrome/Edge 可执行文件(优先于自动探测);
`PDFIUM_DLL` 指定 pdfium.dll(import 子命令读 PDF/AI 时用)。

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
