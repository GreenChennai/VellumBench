# ADR-0043: 动效时间轴落盘为 CSS @keyframes,预览与导出复用同一求值

- 状态:已接受(阶段 5-9 / 05-9;兑付迭代附录 ADR-VB-L11,主文档 W12)
- 背景:导出期已能解析 `@keyframes` 并支持逐帧采样;若编辑器预览另写一套
  求值,预览与导出必然分叉 —— 「所见即所得」对动画失效。
- 决策:
  1. 时间轴关键帧(时刻 / 值 / 缓动)落盘为每对象 `@keyframes vb-anim-<sid>`
     + 对应 `animation` 声明,写进 canonical HTML 的 raw_css 区 ——
     动画即 CSS,浏览器与任意工具都能读;
  2. **预览与导出复用同一求值路径**:播放时按真实 dt 推进播放头,画布经
     `anim::apply_frame_state` 呈现;GIF/MP4 导出逐帧走同一状态机
     (无浏览器时静态求值兜底并告警);
  3. 关键帧增删 / 循环 / 清空均为命令(可撤销);时间轴面板状态不持久化
     (会话态)。
- 落地:`crates/vb_doc/src/commands.rs`(`anim_keyframes_name` / SetAnim
  命令族)、`crates/vb_app/src/app/timeline.rs`(面板 + 播放节拍)、
  `crates/vb_export`(导出期解析与逐帧采样)、台账 09-I。
- 取舍:受 CSS 动画表达力限制(曲线变速等不做);换来"所见即所得 +
  导出可复现 + 文档仍是纯 HTML/CSS"。
- 被否决替代:(a) 自定义动画格式(脱离 HTML,摧毁单一真相);
  (b) 预览自研求值(与导出分叉)。
