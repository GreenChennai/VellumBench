/* zcode-workflow
name: ship
description: 发布门:跑全量语言门禁(cargo/uv/npm)→ ocr 规则检测 + 独立评审 → 按 RELEASE.md 冒烟 → 构建产物并打语义化版本 tag,产出发布报告。不达标不放行。
whenToUse: 用户说"发布/上线/打个版本"、/ship 命令;dev-loop 全绿后的最后一道关。
args:
  version:
    type: string
    required: false
    default: ""
    description: 要发布的版本号(如 v1.2.0);留空则按 semver 规则自动 bump
  skipReview:
    type: boolean
    required: false
    default: false
    description: 跳过评审门(仅限紧急修复)
*/

let version = typeof args.version === "string" && args.version ? String(args.version) : "";
const skipReview = args.skipReview === true;

interface ShipCheck {
  /** 检查项名称 */
  name: string;
  /** 是否通过 */
  pass: boolean;
  /** 证据:命令与输出摘要,或评审结论 */
  evidence: string;
}

const GATE_JS = [
  "const{execSync}=require('child_process');const fs=require('fs');",
  "function run(name,c){try{execSync(c,{stdio:'pipe',cwd:process.cwd()});return 0}catch(e){process.stderr.write('['+name+'] failed\\n');process.stderr.write((e.stdout||'').toString());process.stderr.write((e.stderr||'').toString());return 1}}",
  "let code=0,ran=[];",
  "if(fs.existsSync('Cargo.toml')){ran.push('cargo');",
  "  if(run('test','cargo test'))code=1;",
  "  if(code===0&&run('build','cargo build --release'))code=1;}",
  "else if(fs.existsSync('pyproject.toml')||fs.existsSync('setup.py')){ran.push('uv');",
  "  if(run('pytest','uv run pytest -q'))code=1;",
  "  if(code===0&&run('build','uv build'))code=1;}",
  "else if(fs.existsSync('package.json')){ran.push('npm');",
  "  if(run('test','npm test'))code=1;",
  "  if(code===0&&run('build','npm run build'))code=1;}",
  "else{console.error('no manifest');code=1}",
  "console.error('ran: '+ran.join(', '));process.exit(code)",
].join("\n");

const checks: ShipCheck[] = [];

phase("跑全量门禁(测试 + 构建)");
const gate = await world.run("node", ["-e", GATE_JS], { timeoutMs: 1_800_000 });
checks.push({ name: "语言门禁(测试/构建)", pass: gate.exitCode === 0, evidence: gate.exitCode === 0 ? "gate exit 0" : (gate.stderr || "").slice(0, 500) });

if (!skipReview) {
  phase("ocr 规则检测 + 独立评审");
  const ocr = await world.run("ocr", ["review", "--format", "json", "--output", ".dev-loop/ocr-ship.json"], { timeoutMs: 600_000 });
  const reviewer = agent("发布评审员", {
    system: "你是发布把关人。看 git diff(相对上一个 tag),结合 ocr 报告(若存在),只关心会上生产的问题:数据丢失、崩溃、安全、性能倒退。不改文件。",
  });
  const review = await reviewer.ask<ShipCheck>(
    `判断当前代码是否可发布。${ocr.exitCode === 0 ? "ocr 报告在 .dev-loop/ocr-ship.json" : "ocr 未产出报告,跳过"}。返回 {name:"评审", pass:bool, evidence:"结论与理由"}。`,
  );
  checks.push(review);
}

phase("冒烟验证");
const smoker = agent("冒烟测试员", {
  system: "你按 RELEASE.md(不存在则按 README 的运行说明)把软件以生产方式启动,走一遍核心用户路径并记录结果。发现问题如实报告,不为过门禁而降低标准。",
});
const smoke = await smoker.ask<ShipCheck>(`执行冒烟验证,返回 {name:"冒烟", pass:bool, evidence:"走了哪些路径、结果如何"}。`);
checks.push(smoke);

phase("打版本 tag 并出发布报告");
let tagged = "";
const allPass = checks.every((c) => c.pass);
if (allPass) {
  if (!version) {
    const last = await world.run("git", ["describe", "--tags", "--abbrev=0"]);
    const lastTag = last.exitCode === 0 ? last.stdout.trim() : "v0.0.0";
    const m = /^v(\d+)\.(\d+)\.(\d+)$/.exec(lastTag);
    const bump = m ? `${m[1]}.${Number(m[2]) + 1}.0` : "0.1.0";
    version = `v${bump}`;
  }
  const tag = await world.run("git", ["tag", "-a", version, "-m", `release ${version}`]);
  tagged = tag.exitCode === 0 ? version : `打 tag 失败:${tag.stderr.slice(0, 200)}`;
}

await artifact.markdown(
  "report",
  [
    `# 发布报告 ${version || "(未打版)"}`,
    "",
    ...checks.map((c) => `- ${c.pass ? "✅" : "❌"} ${c.name}:${c.evidence}`),
    `- tag:${tagged || "未打(门禁未全绿)"}`,
  ].join("\n"),
  { title: "发布报告", primary: true },
);

return {
  conclusion: allPass
    ? `全部门禁通过,已打版本 ${tagged},可以发布。`
    : `发布被拦下:${checks.filter((c) => !c.pass).map((c) => c.name).join("、")} 未过,详见 findings。`,
  findings: checks
    .filter((c) => !c.pass)
    .map((c) => ({ where: c.name, what: "发布门未通过", evidence: c.evidence, status: "verified" as const, severity: "high" as const })),
  verified: checks.map((c) => `${c.name}:${c.evidence.slice(0, 120)}`),
  notCovered: ["生产环境部署演练(部署目标由人工或后续步骤执行)"],
};
