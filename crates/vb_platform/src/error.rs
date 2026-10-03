//! 平台错误(两分类,ADR-0046「降级必须可观测」的平台版)。
//!
//! 宿主能力缺失(gpui 0.2.2 无运行期改标题)与调用失败(OS 层报错)必须
//! 可区分:前者是**结构性**缺失,调用方应换路或降级 UI;后者是瞬时故障,
//! 可重试或报给用户。禁止把不支持假装成功。

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlatformError {
    /// 当前宿主/内核不支持该能力(结构性缺失,非瞬时故障)。
    #[error("平台能力不支持:{op}({reason})")]
    Unsupported { op: &'static str, reason: String },

    /// 调用失败(OS 层错误;可重试或上报)。
    #[error("平台操作失败:{op}({reason})")]
    Failed { op: &'static str, reason: String },
}

impl PlatformError {
    pub fn unsupported(op: &'static str, reason: impl Into<String>) -> Self {
        PlatformError::Unsupported {
            op,
            reason: reason.into(),
        }
    }

    pub fn failed(op: &'static str, reason: impl Into<String>) -> Self {
        PlatformError::Failed {
            op,
            reason: reason.into(),
        }
    }

    /// 是否结构性不支持(区别于瞬时失败)。
    pub fn is_unsupported(&self) -> bool {
        matches!(self, PlatformError::Unsupported { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_and_failed_are_distinguishable() {
        let u = PlatformError::unsupported("set_title", "gpui 0.2.2 仅开窗期可定标题");
        let f = PlatformError::failed("set_text", "OS 拒绝访问剪贴板");
        assert!(u.is_unsupported());
        assert!(!f.is_unsupported());
        assert!(u.to_string().contains("不支持"));
        assert!(f.to_string().contains("失败"));
    }
}
