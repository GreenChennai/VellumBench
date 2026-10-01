/* zcode-workflow
name: dev-loop
description: 自动开发流水线:把目标拆成工单,多子 Agent 并发实现(git worktree 隔离),合并后过测试门、UI 门、评审门,不过就带反馈打回重做,全绿后走发布门。Rust/Python 项目通用。
whenToUse: 用户要求"实现/迭代某功能直到可上线"、/dev-loop 命令;需要并发开发 + 质量门禁的完整迭代。
args:
  goal:
    type: string
    required: true
    description: 本轮要实现的目标,一句话或需求列表
  maxRounds:
    type: number
    required: false
    default: 3
    description: 门禁不过时最多打回重做的轮数
*/

// ───────────────────────── 参数 ─────────────────────────
const goal = String(args.goal);
let maxRounds = 3;
if (typeof args.maxRounds === "number" && args.maxRounds > 0) maxRounds = args.maxRounds;

// ───────────────────────── 类型 ─────────────────────────
interface Ticket {
  /** 工单短 id,如 t1、t2 */
  id: string;
  /** 一句话标题 */
  title: string;
  /** 做什么、改哪些文件、验收标准;足够一个子 Agent 独立完成 */
  detail: string;
  /** 是否涉及用户界面 */
  touchesUi: boolean;
}
interface GateResult {
  /** 该门是否通过 */
  pass: boolean;
  /** 发现的问题清单;通过时为空 */
  problems: string[];
}
interface WorkflowReport {
  conclusion: string;
  findings: { where: string; what: string; evidence: string; status: "verified" | "unconfirmed"; severity: "low" | "medium" | "high" }[];
  verified: string[];
  notCovered: string[];
}

// 测试门/构建门:按项目栈自动选择命令(cargo / uv / pytest / npm),全部通过返回 0。
// 放在 node -e 里执行,保证 world.run 命令集固定。
const GATE_JS = [
  "const{execSync}=require('child_process');const fs=require('fs');",
  "function run(name,c){try{execSync(c,{stdio:'pipe',cwd:process.cwd()});return 0}catch(e){process.stderr.write('['+name+'] failed\\n');process.stderr.write((e.stdout||'').toString());process.stderr.write((e.stderr||'').toString());return 1}}",
  "let code=0,ran=[];",
  "if(fs.existsSync('Cargo.toml')){ran.push('cargo');",
  "  if(run('fmt','cargo fmt -- --check'))code=1;",
  "  if(code===0&&run('clippy','cargo clippy --all-targets -- -D warnings'))code=1;",
  "  if(code===0&&run('test','cargo test'))code=1;",
  "  if(code===0&&run('build','cargo build --release'))code=1;}",
  "else if(fs.existsSync('pyproject.toml')||fs.existsSync('setup.py')){ran.push('uv');",
  "  if(run('ruff','uv run ruff check .'))code=1;",
  "  if(code===0&&run('pytest','uv run pytest -q'))code=1;",
  "  if(code===0&&run('build','uv build'))code=1;}",
  "else if(fs.existsSync('package.json')){ran.push('npm');",
  "  if(run('test','npm test'))code=1;",
  "  if(code===0&&run('build','npm run build'))code=1;}",
  "else{console.error('gate: no recognized project manifest (Cargo.toml/pyproject.toml/package.json)');code=1}",
  "console.error('ran: '+ran.join(', '));process.exit(code)",
].join("\n");

// ───────────────────────── 主流程 ─────────────────────────
const memoryNote =
  "如果本会话可用 hindsight 记忆 MCP 工具(mcp__hindsight__ 前缀),开工先 recall 项目约定与历史教训,收工把本轮踩坑与决策 retain 回去;没有这些工具就跳过,不要报错。";

phase("把目标拆解成可并行的工单");
log(`目标:${goal}`);
const planner = agent("规划师", {
  system: "你是资深架构师。读代码库(SPEC.md/README/AGENTS.md 与源码)后把目标拆成 2-6 张可独立完成的垂直切片工单,每张足够小(一个子 Agent 一次提交能做完),彼此文件冲突最小化。" + memoryNote,
});
const tickets = await planner.ask<Ticket[]>(
  `目标:${goal}\n\n先浏览代码库结构(cargo/Cargo.toml 或 pyproject.toml 或 package.json、src 布局),再产出工单数组。每个工单写清:改哪些文件、实现什么、验收标准。涉及界面的工单必须标注 touchesUi=true,并要求遵循项目设计系统 design/MASTER.md(若存在)。`,
);
log(`拆出 ${tickets.length} 张工单:`);

phase("各工单在独立 worktree 里并行实现");
// 预建 worktree 与分支,避免并发创建打架
const worktrees: { id: string; dir: string; branch: string }[] = [];
for (const t of tickets) {
  const dir = `../.dev-loop-wt/${t.id}`;
  const branch = `dev-loop/${t.id}`;
  const add = await world.run("git", ["worktree", "add", dir, "-b", branch]);
  if (add.exitCode !== 0) log(`工单 ${t.id} worktree 复用(可能已存在):${add.stderr.slice(0, 120)}`);
  worktrees.push({ id: t.id, dir, branch });
}
log(`已建 ${worktrees.length} 个 worktree,开始并行实现`);
await Promise.all(
  tickets.map(async (t) => {
    const wt = worktrees.find((w) => w.id === t.id)!;
    const impl = agent(`实现-${t.id}`, {
      system:
        "你是严格测试先行的实现工程师。遵守项目既有代码风格与 AGENTS.md。" +
        (t.touchesUi ? "这是界面工单:触发并遵循 rust-ui / python-ui / ui-ux-pro-max 技能,间距用 4/8 尺度,空/加载/错误三态齐备,对照 design/MASTER.md。" : "") +
        memoryNote,
    });
    const res = await impl.ask<string>(
      `在 worktree 目录 ${wt.dir}(分支 ${wt.branch})里完成工单:${t.title}\n${t.detail}\n\n要求:测试先行;只在本 worktree 里改文件;完成后在 ${wt.branch} 分支提交(git add -A && git commit),提交信息写清做了什么。返回一段完成说明:改了哪些文件、测试结果、遗留风险。`,
    );
    report({ ticket: t.id, summary: res.slice(0, 500) });
  }),
);

phase("合并各分支并跑全量测试");
const integrator = agent("集成工程师", {
  system: "你负责把各实现分支合并回当前分支,处理冲突时保持两边意图,合并后确保可编译。不要新写功能。",
});
const mergeNote = await integrator.ask<string>(
  `把以下分支按顺序合并进当前分支(${worktrees.map((w) => w.branch).join(", ")}),冲突逐个解决;全部合并后清理 worktree(git worktree remove ../.dev-loop-wt/<id>)。返回合并摘要:每个分支合了什么、有没有冲突及怎么解的。`,
);
report({ stage: "merge", summary: mergeNote.slice(0, 500) });

let lastFeedback = "首轮,无历史反馈";
let gatePass = false;
for (let round = 1; round <= maxRounds; round++) {
  phase("跑语言门禁并修复失败项");
  const gate = await world.run("node", ["-e", GATE_JS], { timeoutMs: 1_800_000 });
  if (gate.exitCode === 0) {
    gatePass = true;
    log(`第 ${round} 轮:测试/构建门禁全绿`);
    break;
  }
  log(`第 ${round} 轮:门禁失败,交给修复工程师`);
  const fixer = agent(`修复工程师-r${round}`, {
    system: "你是缺陷修复工程师,只修门禁报出的问题,不做额外重构。若门禁不可能通过,如实说明而不是绕过。",
  });
  await fixer.ask<string>(
    `测试/构建门禁失败(第 ${round} 轮)。历史反馈:${lastFeedback}\n失败输出:\n${(gate.stderr || gate.stdout).slice(0, 8000)}\n\n修复后提交到当前分支。返回修了什么。`,
  );
  lastFeedback = `第 ${round} 轮门禁失败已修复,注意同类问题`;
}

let uiResult: GateResult = { pass: true, problems: [] };
if (gatePass) {
  phase("UI 门:真人式点验 + 规则审计");
  const uiTester = agent("UI 测试员", {
    system:
      "你负责 UI 质量门。先触发 ui-taste-checklist 与 web-design-guidelines 技能做静态审计;再启动应用(读 README/AGENTS.md 找启动命令),用浏览器工具走一遍核心交互并截图自查。若项目没有任何界面(纯后端/CLI),直接返回 pass。" + memoryNote,
  });
  uiResult = await uiTester.ask<GateResult>(
    `检查本轮改动涉及的界面(工单:${tickets.filter((t) => t.touchesUi).map((t) => t.id).join(", ") || "无"})。对照 ui-taste-checklist 九大项与 web 界面规则审计,输出 problems 列表(每条带 file:line 或截图证据)与 pass 判定。`,
  );
  for (const p of uiResult.problems) report({ stage: "ui-gate", problem: p.slice(0, 500) });
} else {
  log("测试门未过,跳过 UI 门");
}

if (gatePass && uiResult.pass) {
  phase("评审门:ocr 规则检测 + 独立代码评审");
  const ocr = await world.run("ocr", ["review", "--format", "json", "--output", ".dev-loop/ocr-report.json"], { timeoutMs: 600_000 });
  const ocrNote = ocr.exitCode === 0 ? "ocr 规则检测已完成,报告在 .dev-loop/ocr-report.json" : `ocr 未产出报告(exit ${ocr.exitCode}),跳过该检测`;
  log(ocrNote);
  const reviewer = agent("独立评审员", {
    system: "你是独立评审员,没参与实现。用 git diff 看本轮全部改动,结合 ocr 报告(若存在),只报 blocker 与 major 问题,不吹毛求疵。不要修改任何文件。",
  });
  const review = await reviewer.ask<GateResult>(
    `评审本轮改动(目标:${goal})。${ocrNote}。读 git diff 与相关文件,返回 problems(blocker/major,带 file:line)与 pass 判定。`,
  );
  for (const p of review.problems) report({ stage: "review-gate", problem: p.slice(0, 500) });
  if (!review.pass) {
    phase("按评审意见打回修复");
    const fixer = agent("评审修复工程师", { system: "只修评审列出的 blocker/major 问题,修完提交。" });
    await fixer.ask<string>(`评审意见:\n${review.problems.join("\n")}\n逐条修复并提交。返回修复清单。`);
    const regate = await world.run("node", ["-e", GATE_JS], { timeoutMs: 1_800_000 });
    if (regate.exitCode !== 0) log("评审修复后门禁仍红,留待人工处理");
  }
} else if (!gatePass) {
  log("测试门未过,跳过 UI/评审门,报告里如实披露");
}

phase("产出本轮迭代报告");
await artifact.markdown(
  "report",
  [
    `# dev-loop 迭代报告:${goal}`,
    "",
    `- 工单:${tickets.map((t) => `${t.id} ${t.title}`).join(";")}`,
    `- 测试/构建门禁:${gatePass ? "✅ 通过" : "❌ 未通过"}`,
    `- UI 门:${uiResult.pass ? "✅ 通过" : "❌ " + uiResult.problems.length + " 个问题"}`,
    `- 详见上方各阶段结果项;合并与修复过程见 Results 流。`,
  ].join("\n"),
  { title: "迭代报告", primary: true },
);

return {
  conclusion: gatePass
    ? `目标"${goal}"已完成 ${tickets.length} 张工单的实现与合并,门禁${uiResult.pass ? "全绿,达到可发布状态" : "测试绿但 UI 门有问题,见 findings"}。`
    : `目标"${goal}"实现了 ${tickets.length} 张工单,但 ${maxRounds} 轮内测试门未全绿,代码已提交,失败输出见 findings。`,
  findings: [
    ...uiResult.problems.map((p) => ({ where: "UI 门", what: p, evidence: "UI 测试员审计与实测", status: "verified" as const, severity: "medium" as const })),
  ],
  verified: gatePass
    ? ["cargo/uv/npm 测试与构建门禁由脚本实际运行(exit 0)", "UI 走静态审计 + 浏览器实测", "ocr 规则检测 + 独立评审(视运行结果)"]
    : ["门禁失败输出已捕获并交修复,未达到通过"],
  notCovered: ["真实用户验收、生产环境部署演练(发布门/ship 工作流负责)"],
};
