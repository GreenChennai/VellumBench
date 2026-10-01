/* zcode-workflow
name: fix-ui
description: 只跑 UI 质量闭环:静态规则审计(ui-taste-checklist + web-design-guidelines)→ 启动应用真人式点验 → 按发现的问题逐个修复 → 复检,直到 UI 门通过。
whenToUse: 用户说"界面很丑/帮我修 UI/检查界面质量"、/fix-ui 命令;不需要新功能开发,只修界面。
args:
  scope:
    type: string
    required: false
    default: ""
    description: 限定范围,如某页面/组件目录;留空表示全项目
  maxRounds:
    type: number
    required: false
    default: 2
    description: 修复-复检最多循环轮数
*/

const scope = typeof args.scope === "string" && args.scope ? String(args.scope) : "整个项目";
let maxRounds = 2;
if (typeof args.maxRounds === "number" && args.maxRounds > 0) maxRounds = args.maxRounds;

interface UiFinding {
  /** 问题位置:file:line 或页面/组件名 */
  where: string;
  /** 一句话说清什么问题,引用违反的规则 */
  what: string;
  /** 建议修法 */
  fix: string;
  /** 优先级:P0 破损不可用 / P1 明显难看或不可及 / P2 打磨项 */
  priority: "P0" | "P1" | "P2";
}

const memoryNote =
  "如果本会话可用 hindsight 记忆 MCP 工具,开工先 recall 该项目 UI 相关历史,收工 retain 本轮结论;没有就跳过。";

phase("审计界面并列出问题清单");
const auditor = agent("UI 审计员", {
  system:
    "你只找问题不改代码。触发 ui-taste-checklist 与 web-design-guidelines(及项目栈对应的 rust-ui/python-ui)技能,读相关源码与样式做静态审计;再启动应用(读 README/AGENTS.md 找命令)用浏览器工具实测核心路径并截图对照。纯后端/CLI 项目返回空清单并说明无 UI。" +
    memoryNote,
});
let findings = await auditor.ask<UiFinding[]>(
  `审计范围:${scope}。按 P0/P1/P2 输出问题清单(位置 + 违反的规则 + 建议修法)。宁可多报 P2,不许漏 P0/P1。`,
);
log(`发现 ${findings.length} 个 UI 问题(P0/P1 优先修)`);
for (const f of findings) report(f);

if (findings.length === 0) {
  await artifact.markdown("report", `# UI 复检结论\n\n范围:${scope}。未发现 P0/P1/P2 问题,UI 门通过。`, { title: "UI 修复报告", primary: true });
  return { conclusion: `范围"${scope}"的界面未发现问题,UI 门通过。`, findings: [], verified: ["静态规则审计 + 启动应用浏览器实测"], notCovered: ["真实用户验收"] };
}

let fixed = 0;
for (let round = 1; round <= maxRounds; round++) {
  phase("按清单修复 UI 问题并复检");
  const fixer = agent(`UI 修复师-r${round}`, {
    system:
      "你是 UI 实现工程师,按清单逐条修复,保持既有设计系统(design/MASTER.md)与代码风格;修 P0/P1 必修,P2 尽量。遵守 rust-ui/python-ui/ui-ux-pro-max 技能规范。" + memoryNote,
  });
  const todo = findings.filter((f) => f.priority !== "P2" || round === maxRounds);
  await fixer.ask<string>(
    `修复以下 UI 问题(范围:${scope}):\n${todo.map((f) => `- [${f.priority}] ${f.where}: ${f.what} → ${f.fix}`).join("\n")}\n逐条修完并提交。返回每条的修复说明。`,
  );
  const checker = agent(`UI 复检员-r${round}`, {
    system: "你是独立复检员,没参与修复。只验证清单上的问题是否真的解决,并检查修复没引入新问题。",
  });
  const remaining = await checker.ask<UiFinding[]>(
    `复查这批 UI 问题是否已解决:\n${findings.map((f) => `- [${f.priority}] ${f.where}: ${f.what}`).join("\n")}\n返回仍未解决或新引入的问题清单(同样格式);全解决返回空数组。`,
  );
  findings = remaining;
  if (findings.length === 0) {
    fixed = round;
    log(`第 ${round} 轮全部修复,复检通过`);
    break;
  }
  log(`第 ${round} 轮后剩余 ${findings.length} 个问题`);
}

await artifact.markdown(
  "report",
  [
    `# UI 修复报告(${scope})`,
    "",
    `- 初始发现:${fixed ? "已全部修复" : "部分修复"}`,
    `- 循环轮数:${fixed || maxRounds}`,
    `- 剩余问题:${findings.length === 0 ? "无" : findings.map((f) => `[${f.priority}] ${f.where} ${f.what}`).join(";")}`,
  ].join("\n"),
  { title: "UI 修复报告", primary: true },
);

return {
  conclusion:
    findings.length === 0
      ? `范围"${scope}"的 UI 问题已全部修复并复检通过。`
      : `范围"${scope}"的 UI 问题修复了大部分,${findings.length} 个未解决项见 findings,建议人工确认。`,
  findings: findings.map((f) => ({
    where: f.where,
    what: `[${f.priority}] ${f.what}`,
    evidence: `UI 复检员复核;原始审计建议:${f.fix}`,
    status: "verified" as const,
    severity: f.priority === "P0" ? ("high" as const) : ("medium" as const),
  })),
  verified: ["静态规则审计(ui-taste-checklist + web-design-guidelines)", "启动应用浏览器实测", "每轮独立复检"],
  notCovered: ["跨浏览器兼容性", "真实用户验收"],
};
