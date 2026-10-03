//! 命令信封(R0 最小实体;22 篇 §3.1「命令分发(run_command 适配)」的地基)。
//!
//! # 本批边界(重要,防误读)
//!
//! 只落**信封类型**:稳定命令 id + 请求/回执的端无关线格式。
//! `run_command` 的**分发体原样留在旧宿主**(`vb_app::app::dispatch*`
//! 的巨型 match,含全部命令语义),搬家是 R1 交付(22 篇 §4 R1 画布
//! 换血批);本模块不认识任何具体命令,也不持有任何应用状态。
//!
//! # id 双层注记
//!
//! - [`CommandId`]:6 位 `[a-z0-9]` **稳定 id**(语法同
//!   `vb_common::StableId`,机器侧跨改名/重排不变);
//! - `commands.yaml` 的点分目录 id(`file.new`,人类可读,菜单/快捷键/
//!   CLI 引用)是另一层。**目录 id ↔ CommandId 的映射表及其门禁随 R1
//!   命令单源批次落地**,本批只锁稳定 id 语法。

use crate::selection::ModifierState;
use serde::{Deserialize, Serialize};
use std::fmt;

/// 命令稳定 id:恰 6 个 `[a-z0-9]` 字符(小写字母或数字)。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CommandId(String);

/// [`CommandId::parse`] 的拒绝原因(携带原串,便于上报定位)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandIdError {
    got: String,
    reason: &'static str,
}

impl fmt::Display for CommandIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "非法命令稳定 id {:?}:{}", self.got, self.reason)
    }
}

impl std::error::Error for CommandIdError {}

impl CommandId {
    /// 稳定 id 定长(语法同 `vb_common::StableId` 的生成端)。
    pub const LEN: usize = 6;

    /// 校验并构造:恰 6 位、每字节为小写字母或数字。
    pub fn parse(s: &str) -> Result<Self, CommandIdError> {
        let err = |reason: &'static str| CommandIdError {
            got: s.to_string(),
            reason,
        };
        if s.len() != Self::LEN {
            return Err(err("须恰 6 字节"));
        }
        if !s
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        {
            return Err(err("只允许 [a-z0-9]"));
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CommandId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 命令请求信封(端无关):GUI 快捷键 / 菜单 / CLI / MCP 发同一形状。
///
/// 字段 = 稳定 id + 手势修饰键快照(旧宿主 `run_command(id, shift, alt)`
/// 参数的投影,复用 [`ModifierState`],多选/约束语义由执行方裁决)。
/// 数值输入等**命令参数载荷**不在此批定形 —— 随 R1 分发搬家一并收口。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRequest {
    pub id: CommandId,
    pub modifiers: ModifierState,
}

impl CommandRequest {
    pub fn new(id: CommandId) -> Self {
        Self {
            id,
            modifiers: ModifierState::default(),
        }
    }

    pub fn with_modifiers(mut self, modifiers: ModifierState) -> Self {
        self.modifiers = modifiers;
        self
    }
}

/// 命令回执信封(端无关):执行方对一次 [`CommandRequest`] 的最小答复。
///
/// 与旧宿主 `run_command` 的口径一致:命中即执行(是否入 undo 栈由具体
/// 命令决定,信封不表达);未命中/前置不满足(如无选中对象、"至少保留
/// 一块画板")回人类可读 `reason`,状态栏/toast 直出。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandOutcome {
    /// 已执行。
    Applied,
    /// 未执行:`reason` 为人类可读说明。
    NotRun { reason: String },
}

impl CommandOutcome {
    /// 是否真的执行了(统计/门禁用)。
    pub fn is_applied(&self) -> bool {
        matches!(self, CommandOutcome::Applied)
    }

    /// 构造"未执行"回执。
    pub fn not_run(reason: impl Into<String>) -> Self {
        Self::NotRun {
            reason: reason.into(),
        }
    }
}

impl CommandId {
    /// 目录 id 构造(点分空间,如 `file.new`;不做 6 位稳定 id 校验)。
    ///
    /// R0 边界:稳定 id ↔ 目录 id 的两层映射属 R1(见模块文档);本构造
    /// 让 R0 的旧宿主桥接能以目录 id 走 [`Dispatcher`],映射落地后收紧。
    pub fn from_catalog(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// 命令分发边界(ADR-0048 §3):宿主实现它来接受命令信封。
///
/// R0 只立边界 + 旧宿主桥接(vb_app 提供);新宿主在 R1 有了自己的会话
/// 后经同一 trait 接入。命中/未命中的回执口径见 [`CommandOutcome`]。
pub trait Dispatcher {
    fn dispatch(&mut self, req: CommandRequest) -> CommandOutcome;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_six_lowercase_alnum() {
        for s in ["abc123", "a0b1c2", "zzzzz9", "000000"] {
            let id = CommandId::parse(s).unwrap_or_else(|e| panic!("{s}: {e}"));
            assert_eq!(id.as_str(), s);
            assert_eq!(id.to_string(), s);
        }
    }

    #[test]
    fn parse_rejects_wrong_shape() {
        // 长度不对
        assert!(CommandId::parse("").is_err());
        assert!(CommandId::parse("abc12").is_err());
        assert!(CommandId::parse("abc1234").is_err());
        // 字符不对:大写 / 连字符 / 点 / 下划线 / 空白
        assert!(CommandId::parse("ABC123").is_err());
        assert!(CommandId::parse("ab-c12").is_err());
        assert!(CommandId::parse("abc.12").is_err());
        assert!(CommandId::parse("ab_c12").is_err());
        assert!(CommandId::parse("ab c12").is_err());
        // 多字节:字节数恰 6 的两个汉字也不行(逐字节校验)
        assert!(CommandId::parse("命令").is_err());
    }

    #[test]
    fn parse_error_reports_input_and_reason() {
        let e = CommandId::parse("file.new").expect_err("点分目录 id 不是稳定 id");
        assert_eq!(e.to_string(), "非法命令稳定 id \"file.new\":须恰 6 字节");
    }

    #[test]
    fn id_is_hashable_and_orderable() {
        use std::collections::BTreeSet;
        let mut set = BTreeSet::new();
        set.insert(CommandId::parse("abc123").unwrap());
        set.insert(CommandId::parse("abc123").unwrap());
        set.insert(CommandId::parse("zzzzzz").unwrap());
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn request_carries_id_and_modifiers() {
        let req = CommandRequest::new(CommandId::parse("abc123").unwrap()).with_modifiers(
            ModifierState {
                shift: true,
                ..Default::default()
            },
        );
        assert_eq!(req.id.as_str(), "abc123");
        assert!(req.modifiers.shift);
        assert!(req.modifiers.is_additive());
        // 缺省修饰键 = 无修饰
        assert!(!CommandRequest::new(CommandId::parse("zzzzzz").unwrap())
            .modifiers
            .is_additive());
    }

    #[test]
    fn outcome_helpers_and_serde_roundtrip() {
        assert!(CommandOutcome::Applied.is_applied());
        let nr = CommandOutcome::not_run("至少保留一块画板");
        assert!(!nr.is_applied());
        assert_eq!(
            nr,
            CommandOutcome::NotRun {
                reason: "至少保留一块画板".into()
            }
        );
        // 线格式可(反)序列化 —— 三端同源的前提
        for (v, json) in [
            (CommandOutcome::Applied, r#""Applied""#),
            (
                CommandOutcome::not_run("无选中对象"),
                r#"{"NotRun":{"reason":"无选中对象"}}"#,
            ),
        ] {
            assert_eq!(serde_json::to_string(&v).unwrap(), json);
            let back: CommandOutcome = serde_json::from_str(json).unwrap();
            assert_eq!(back, v);
        }
        let req = CommandRequest::new(CommandId::parse("a1b2c3").unwrap());
        let json = serde_json::to_string(&req).unwrap();
        let back: CommandRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, req);
    }
}
