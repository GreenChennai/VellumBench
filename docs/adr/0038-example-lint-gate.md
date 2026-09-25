# ADR-0038: 新增示例体检门禁(check_examples)

- 状态:已接受(阶段 3 / 03-2;兑付迭代附录 ADR-VB-L06,主文档 W9)
- 背景:实测 P0-④ `examples/landing` 卡片类名不匹配 → 覆叠 + 右侧空白 1/3,
  长期无人发现 —— 随仓库示例是"产品的脸",却没有机器体检。
- 决策:
  1. `tools/check_examples.py` 对每个随仓库示例断言:tree 可读、画板数 /
     元素数 / 文字数达基线、无元素覆叠、无"空白 1/3"漂位;
  2. 基线入库固化(`tools/baselines/`),示例有意改动时经评审重建基线;
  3. 接入 ci.ps1 门禁 9(轻量档也跑 —— 走 vellum-cli 无头通道)。
- 落地:`tools/check_examples.py`、`tools/baselines/`、`tools/ci.ps1`(门禁 9)。
- 取舍:示例改一次要同步基线;换来示例可信、防"打开即像软件坏了"。
- 被否决替代:手工检查示例(P0-④ 证明不可靠)。
