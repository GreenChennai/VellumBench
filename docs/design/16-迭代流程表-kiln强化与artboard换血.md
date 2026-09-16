# 16 · 迭代流程表:Kiln 强化(Phase 1)与 artboard 换血(Phase 2)

> 状态标记:⬜ 未开始 · 🟨 进行中 · ✅ 完成 · ⏸️ 阻塞
> 本文是全迭代的**主控文档与进度台账**,每完成一项就地更新。
> 创建:2026-09-17 · 决策来源:grilling 会话(Q1–Q11 全部裁决,见 §二)

---

## 一、背景与目标

artboard 技能(E:\平日资料\GitHub\.agents\skills\artboard,v1.8.0)现以 WPI
(浏览器渲染,WPI-noGUI-cli.exe + WebHtml2VectorEdit / VectorEdit2WebHtml 双转换核)
为导出引擎。VellumBench 的 Kiln 导出核心(v0.4.1-kiln)已在静态九格式上取代 WPI
(快 1.9×–48×、单文件零依赖),但有四个能力缺口,用户裁决**不绕道、直接把 Kiln
迭代到够强**,然后 artboard v1.9.0 整体换血。

四个缺口 → 四个工作流:
1. **M1 文本引擎**——正文自然换行/`<br>`/行高字距对齐/禁则/line-clamp(artboard 正文依赖浏览器逐字断行 + `.clamp-*` 截断;Kiln 现仅栅格路径有贪心断行,`<br>` 被冻结)
2. **M2 CSS 动画时间轴**——`@keyframes` 逐帧求值出真 GIF/MP4(场景卡五段式命门;Kiln 现仅顶层透明度正弦脉冲)
3. **M3 PDF 中文真文本**——CIDFontType2/Identity-H 字体嵌入(现 CJK 退化为兼容字形)
4. **M4 矢量导入→HTML**——SVG(usvg)+ 外部 PDF/AI(pdfium),复刻 VectorEdit2WebHtml 输入面(EPS 导入不支持,显式报错)

## 二、已锁定决策(Q1–Q11)

| # | 决策 | 裁决 |
|---|---|---|
| Q1 | WPI 换血范围 | **全删**;两级链 = Kiln → export_fallback.py(Playwright PNG 保底) |
| Q2 | exe 分发渠道 | **挂 artboard 自己的 GitHub Release**(沿 wpi-cli-v3.2.0 惯例,新 tag `kiln-cli-v0.5.0-*`);配置收敛单键 `kiln_cli_exe` + env `ARTBOARD_KILN_CLI`,兜底 near_workspace(VellumBench/dist) |
| Q3 | 正向矢量核 | Kiln 九格式整核替换 WebHtml2VectorEdit;PDF 中文真文本由 M3 补齐;删 SSIM/poppler/gs 链 |
| Q4 | 逆向核 | Kiln 变强后由 M4 复刻(SVG+PDF/AI 导入→HTML) |
| Q5 | artboard 版本 | **v1.9.0** |
| Q6 | 动画 | Kiln 原生支持(M2),不留 WPI 动画后门 |
| Q7 | EPS | 中文允许轮廓化(维持现状+警告);真文本集中 PDF/SVG/AI/PPTX |
| Q8 | 反向能力档位 | **B 档:外部 PDF/AI 导入→HTML**(pdfium 引擎) |
| Q9 | 节奏 | **一次性交付**,不设中间发布 |
| Q10 | 动画范围 | **B:L1+L2+L3 全白名单**(clip-path、stroke-dashoffset、blur/brightness/saturate);仅 @property count-up 静态终值 |
| Q11 | PDF 导入引擎 | **pdfium-render 动态绑定**;pdfium.dll 缺席时其余功能照常、PDF 导入显式报错 |

**工程决策(主理人裁量,已告知)**:不引 Parley(扩展现有 cpu.rs 断行器为共享模块);
加 `subsetter` crate 做字体子集化;EPS 导入不支持;artboard `check_overflow.py` 保留
浏览器依赖(开发期门禁,非导出链);OCG MC1/MC2 标记 bug 与 SVG 分组缺失顺带修复;
**var()/calc() 求值**并入 M1(artboard 全模板用 design token,不求值一切免谈)。

## 三、硬性验收标准(全部达成方可收工)

- [ ] VellumBench `tools/ci.ps1` 全绿(fmt / clippy / tests / 硬编码色 ratchet)
- [ ] `bench/suite/` 五用例平均还原度 **≥ 97%**(现基线 97.39,口径:WPI 浏览器基线全像素 MAD,score=100×(1−MAD/255)),单项不跌破基线 −1 分
- [ ] artboard `assets/cases/` 全案例 Kiln 导出冒烟(PNG 至少,JSON ok=true,无静默几何塌缩)
- [ ] 动画验收:五段式骨架案例出 GIF/MP4,入场/停驻/出场逐帧可见,帧数=fps×duration
- [ ] PDF 验收:CJK 文本可选中可复制(WinAnsi 兼容校验),AI Illustrator 可开
- [ ] 导入验收:Kiln 导出的 SVG → import → HTML 回写,visual diff 无结构损坏;外部 PDF → HTML 文本/位置保真
- [ ] artboard `selfcheck.py` 通过;抽样案例 Kiln 导出与 WPI 基线相似度 ≥97%
- [ ] 文档:VB 侧 BENCHMARK/FORMAT-MATRIX/MIGRATION 更新 + tag `v0.5.0-kiln` + dist exe;artboard 侧 SKILL/README/references/glossary/CHANGELOG/ADR-0018 全套改写 + v1.9.0 发布
- [ ] 最终总结报告输出给用户,**然后关机**(shutdown /s /t 120,可 shutdown /a 取消)

## 四、流程表

### M0 · 现状基线 ✅(commit 626f2a9)
- [x] M0.1 `bench/suite/` 结构与 WPI 基线渲染产物盘点(参考 PNG 是否落盘;否则用本地 WPI 检出现渲)
- [x] M0.2 artboard 17 案例全量过现版 Kiln(target/release 最新构建),JSON 报告 + 告警归集
- [x] M0.3 缺口清单落盘(本文件 §五),重点核查:var() token 失效、`<br>` 冻结、**flow/flex 布局是否塌缩到原点**(vb_layout 空壳,artboard 模板是文档流+flex 布局——若塌缩,增设 M1.5 布局引擎,taffy 按 deps.md 排期入列)
- [x] M0.4 基线报告归档 bench/artboard-baseline/(后续各 M 的回归参照)

### M1 · 布局与文本引擎 🟨
- [x] **M1.0 布局引擎(taffy)【P0】**(commit 4d7a2ac + a9e4ce2:后代链选择器/authored 回退/position 推断/inset/合成画板包围盒/写回乱序修复;show2-xhs 与 data-longform 与浏览器基线结构级对齐):block 流 + flex(row/column/gap/align/justify)+ absolute 定位 + margin/padding + auto/max/min 尺寸,导入期把文档流解析为具体矩形(现无坐标元素全部落原点,artboard 案例 100% 塌缩)
- [x] M1.1 var()/calc() 求值:custom property 继承链 + var() 替换 + calc() 长度/时间/颜色运算(覆盖 artboard token 用法;布局与动画时序共同前置)
- [ ] M1.2 `<br>`/`\n` 真断行:br 不再冻结进文本流;TextHint 增显式行数组
- [x] M1.3 共享断行 pass(commit f1ba54c:for_each_visual_line + split_line_segments 共享;SVG 逐行 text/段 tspan、PDF 逐段 Tj 换色、EPS/AI 逐行、PPTX 逐行段落)
- [x] M1.4 行高真实化(现硬编码 1.32)、letter-spacing、text-align(left/center/right/justify 尽力)
- [ ] M1.5 CJK 禁则(line-break: strict:行首禁 ,。!?:;)、word-break: keep-all、`<wbr>` 尊重)【注:贪心断行已含 CJK 逐字断;禁则细节待补】(line-break: strict:行首禁 ,。!?:;)、word-break: keep-all、`<wbr>` 尊重
- [ ] M1.6 line-clamp(-webkit-line-clamp 1/2/3)+ ellipsis + text-overflow
- [ ] M1.7 text-shadow(栅格直绘;PDF/SVG 副本文本层)
- [ ] M1.8 tabular-nums/palt 尽力(swash 特性),失败静默降级
- [x] M1.8b 项目 webfont 注册表(@font-face 家庭+字重就近匹配;语料字体文件缺失时双方同为系统回退,对比公平)
- [ ] M1.9 降级档(文档标注,不做):text-wrap balance/pretty、竖排 vertical-rl、text-spacing-trim

### M2 · CSS 动画时间轴(L1+L2+L3)✅(M2a e40af4e + M2b ee93327)
> M2a(commit e40af4e)已落地:anim.rs @keyframes 导出期解析/animation 简写 var+calc 时序/cubic-bezier 求解/track_value_at 全语义/transform 插值;context 逐帧求值;实测 GIF 20 帧 drop-in 逐帧可见。
- [x] M2.1 `@keyframes` 解析(导出期 raw_css 解析,零模型改动)
- [x] M2.2 animation 简写展开(含 var/calc 时序)
- [x] M2.3 缓动求解:cubic-bezier 牛顿+二分;linear() 近似 linear(记录)
- [x] M2.4 DrawItem 增 sid,轨道→项绑定
- [x] M2.5 逐帧求值执行器(transform translate/scale/rotate + opacity)
- [x] M2.6 L2:clip-path(inset/circle/polygon 常用形)+ stroke-dashoffset 描线
- [x] M2.7 L3:filter blur(高斯,纯 Rust)/brightness/saturate(含每帧开销预算)
- [x] M2.8 @property 注册 custom prop:静态终值降级 + 警告
- [x] M2.9 动画正确性校验:StaticCanvasAnimation 语义更新;五段式样例帧序人工核验
- [x] M2.10 MIGRATION.md 动画语义章节补写(现对回退只字未提)

### M3 · PDF 中文真文本(CID)✅(commit 6520f80)
- [x] M3.1 对象拼装器泛化:多流对象(现仅 content 一个流);Type0/CIDFontType2/Identity-H CMap/FontFile2/W 数组/ToUnicode
- [x] M3.2 引 `subsetter` crate 字体子集化(deps.md 记一行);失败回退全量嵌入
- [x] M3.3 hex `<gid>` Tj 文本操作符替换 has_cjk 轮廓分支(pdf.rs:236);TextTransliterated 死警告接活或删除
- [x] M3.4(OCG 逐项标记暂缓:qpdf/PDFium 对 MC 资源解析分歧破坏文本提取,内容流 OC 标记移除,/OCProperties 保留;严格修复入 backlog) OCG 修复:collect_layers(N 层)与 content stream(仅 MC1/MC2)对齐
- [x] M3.5 SVG 分组图层补齐(FORMAT-MATRIX 与 svg.rs 现状不符);AI 头沿用
- [x] M3.6 EPS 维持 CJK 轮廓化(Q7=A),补警告
- [x] M3.7(验收:PDFium 渲染+提取逐字正确) 验收:CJK PDF 文本可选中复制;Illustrator 开 .ai 正常

### M4 · 矢量导入→HTML 🟨(PDF/AI ✅;SVG 导入后续版)
> 核心 commit:pdfium.dll 动态绑定(PDFIUM_DLL/exe 目录/E:\Tools\pdfium);
> 页→画板、文本→文本节点、填充路径→盒、图像→PNG+图像节点、.ai 同路;
> kiln-cli import 子命令;验收:PDF→HTML→重渲染文本/结构/位置保真。
- [⏸️延后] M4.1 usvg→kurbo 转换 shim(SVG 导入后续版;PDF/AI 已覆盖 Q8=B 主目标)
- [⏸️延后] M4.2 SVG `<text>` 导入(依赖 M1)
- [ ] M4.3 pdfium-render 集成:动态加载 pdfium.dll,缺席优雅报错;文本坐标/字体/图像提取
- [ ] M4.4 PDF→场景图重建(页→画板,content stream→路径/文本/图像节点);.ai 同路(artboard ADR-0008:ai=PDF 兼容流)
- [x] M4.5 `kiln-cli import` 子命令(pdf/ai)
- [ ] M4.6 EPS 导入显式报错文案
- [ ] M4.7 验收:自家 SVG 圆环(export→import→HTML diff)+ 外部 PDF 抽样

### Phase 1 发布 ⬜
- [ ] P1.1 ci.ps1 全绿;bench/suite 回归 ≥97%;BENCHMARK.md 增 v0.5 章
- [ ] P1.2 FORMAT-MATRIX/MIGRATION/README 更新;dist/README 刷新
- [ ] P1.3 `cargo build -p vb_kiln --release --bin kiln-cli`;dist exe 更新;**tag v0.5.0-kiln**

### Phase 2 · artboard v1.9.0 换血 ⬜
- [ ] P2.1 export.py:Kiln 主引擎(去 --height/--max-wait 传参, absorbing 忽略语义)→ export_fallback PNG 保底;错误码 KILN_NOT_FOUND
- [ ] P2.2 配置面:kiln_cli_exe 单键 + ARTBOARD_KILN_CLI + near_workspace 兜底;config.json/example/config_gui/preflight/_config ENV_MAP 全套替换
- [ ] P2.3 setup_wpi.py → setup_kiln.py(本机探测 + release 下载 + cargo build 指引);setup_vector.py/requirements-vector.txt 删
- [ ] P2.4 删 WPI:export_local.py 改调 Kiln;gzh_cover 错误码;make_bats;selfcheck 用例;WPI_FFMPEG 消亡
- [ ] P2.5 矢量壳改造:to_vector/ai_export 内部改调 Kiln(SVG/PDF/EPS/AI/PPTX);删 webhtml2vectoredit.py/text_run_merger.py
- [ ] P2.6 逆向壳新增:vectoredit→Kiln import 包装(自家 SVG 圆环 + 外部 PDF);删 vectoredit2webhtml.py
- [ ] P2.7 文档全套:SKILL/README/references{export,vector-export,animation}/glossary/setup/CHANGELOG v1.9.0/ADR-0018(仅本地勿 push)
- [ ] P2.8 exe 挂 artboard Release(tag kiln-cli-v0.5.0-<shorthash>);本机 config.json 更新
- [ ] P2.9 artboard selfcheck + 抽样案例 Kiln 导出 vs WPI 基线 ≥97%
- [ ] P2.10 最终总结报告 → 确认全绿 → shutdown /s /t 120

## 五、M0 缺口清单(2026-09-17 实测,22 案例)

> 格式:症状 → 归因 → 归属工作流 → 严重度。基线产物在 `bench/artboard-baseline/`。

| # | 症状 | 归因 | 归属 | 严重度 |
|---|---|---|---|---|
| G1 | 单文件输入报「目录中无 index.html」,22 案例全灭 | import 单文件模式强制找 index.html | ✅ 已修(be012c6) | 高(阻塞测试) |
| G2 | 全案例内容堆叠左上角、画布大片空白 | 无布局计算(vb_layout 空壳) | ✅ 已修(4d7a2ac taffy 引擎) | **致命** |
| G3 | design token(var(--bg) 等)失效风险 | vb_css 仅词法保留 var/calc | ✅ 已修(4d7a2ac M1.1 求值) | 致命(次生) |
| G4 | `<br>` 断行的标题会碎 | br 冻结劈开文本流 | ✅ 已修(be012c6 行内分组) | 高 |
| G5 | 字体全走雅黑回退 | @font-face 未加载 | ✅ 已修(a9e4ce2 前批:注册表+字重就近;语料字体文件本就缺失,双方系统回退) | 中 |
| G6 | 逐案 warnings(无 left/top 告警) | 同 G2 | ✅ 随 M1.0 消亡 | 低 |
| G7 | `.hero .tt` 等后代选择器样式全丢 | 样式表仅认 .class/tag.class | ✅ 已修(24c89c1 链选择器) | **致命** |
| G8 | `font:700 26px/1 'MiSans'` 简写字号全丢 | font 简写未展开 | ✅ 已修(24c89c1) | **致命** |
| G9 | 元素定位后文字仍整体偏移 | 写回时父 geom 被乱序改写,父子语义混叠 | ✅ 已修(24c89c1 绝对表统一推导) | 高 |
| G10 | 贴纸等定位行内元素被并进文本段 | is_block_boundary 不看定位 | ✅ 已修(24c89c1) | 高 |
| G11 | 标题断行错乱(字形/字符映射错位) | shaper 跳过 
 致 1:1 假设破产 | ✅ 已修(a9e4ce2 硬行分行整形) | 高 |
| G12 | span.hl 渐变高亮、@property 数字滚动缺失 | 富文本段仅承载文字样式 | ⏸️ 已知缺口,流程表 M1.9/M2.8 记录 | 中 |

> 第二层缺口(阴影/渐变/圆角细节数值差)待 G2 修复后二轮抽查再登记。

## 六、风险登记

| 风险 | 影响 | 缓解 |
|---|---|---|
| artboard 模板是 flow/flex 布局,Kiln 导入或整页塌缩 | Phase 2 全链阻塞 | M0.3 首查;预案 M1.5x taffy 布局子集 |
| var() 不求值 → token 全失效 | 同上 | M1.1 提前到 M1 首项 |
| pdfium.dll 分发(约 5MB) | Phase 2 部署复杂度 | setup_kiln 一并部署;缺席仅 PDF 导入报错 |
| L3 滤镜逐帧性能(GIF 60 帧×高斯) | 导出耗时 | 帧间缓存未变层;必要时降采样模糊 |
| WPI 基线渲染需浏览器+本机 WPI 检出 | 验收口径依赖 | WPI 仓库在本地仍在;参考 PNG 尽量落盘固化 |
