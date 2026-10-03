//! 能力台账 —— **数据已下沉 [`vb_session::capabilities`]**(R0 骨架批次,
//! 22 篇 §4 R0 交付 4:会话态宿主无关化第一步)。
//!
//! 本模块保持 re-export:vb_app 既有引用(`crate::capabilities::*`)零改动,
//! 行为不变。门禁测试按依赖分家:
//! - 依赖 `shortcuts` 命令注册表的(台账引用的命令必须在**当前宿主**已注册)
//!   留在本文件;
//! - 纯数据不变量(编号唯一 / Partial 去向 / Dropped 纪律等)随数据迁入
//!   `vb_session::capabilities` 的测试模块。

pub use vb_session::capabilities::{CapStatus, Capability, CapabilityUi, CAPABILITIES};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcuts::is_implemented;

    /// 台账里列出的每个命令都必须是**已注册命令**(否则台账在说谎)。
    #[test]
    fn every_listed_command_is_implemented() {
        for c in CAPABILITIES {
            for id in c.commands {
                assert!(
                    is_implemented(id),
                    "能力台账 {} 引用了未注册命令 {id}",
                    c.id
                );
            }
        }
    }

    /// 05-1 三态收敛门禁②:所有 `Dropped` 项必须**UI 无入口可复现** ——
    /// 不列任何命令、命令注册表(`IMPLEMENTED_IDS`)里没有它的 id、
    /// 快捷键表里也没有。出现即视为"灰按钮/死入口"回归。
    #[test]
    fn dropped_items_have_no_command_entry() {
        for c in CAPABILITIES {
            if !matches!(c.status, CapStatus::Dropped(_)) {
                continue;
            }
            assert!(
                c.commands.is_empty(),
                "{} 是 Dropped 项,不得声明命令入口",
                c.id
            );
            for id in c.commands {
                assert!(
                    !is_implemented(id),
                    "{} 的 Dropped 能力出现在命令注册表:{id}",
                    c.id
                );
            }
        }
    }
}
