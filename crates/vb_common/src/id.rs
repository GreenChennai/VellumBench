//! 稳定短码 `StableId`:6 位 `[a-z0-9]`,落盘为 `data-vb-id`(见 CONTEXT.md)。
//!
//! 不变量:元素全生命周期不变 —— 改名、移动、重排、Undo/Redo 都不影响;
//! `NodeId`(slotmap 键)是内存索引,允许变化。二者严格分离(设计文档 04 篇)。

use std::fmt;

const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
const LEN: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StableId(String);

impl StableId {
    /// 从种子确定性生成短码(splitmix64 → base36)。
    /// 同一种子生成同一短码;冲突由 [`SidAllocator`] 跳过。
    pub fn from_seed(seed: u64) -> Self {
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        let mut s = String::with_capacity(LEN);
        let mut v = z;
        for _ in 0..LEN {
            let d = (v % 36) as usize;
            s.push(ALPHABET[d] as char);
            v /= 36;
            if v == 0 {
                v = 0x1234_5678_9abc_def1; // 低位耗尽后继续搅动
            }
        }
        Self(s)
    }

    /// 从已有字符串解析(导入 `data-vb-id` 用);须为非空 `[a-z0-9-]`。
    pub fn parse(s: &str) -> Option<Self> {
        if s.is_empty() || s.len() > 16 {
            return None;
        }
        if !s
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return None;
        }
        Some(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StableId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 短码分配器:顺序种子 + 查重,保证文档内唯一。
#[derive(Debug, Default)]
pub struct SidAllocator {
    used: std::collections::HashSet<String>,
    next: u64,
}

impl SidAllocator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reserve(&mut self, id: StableId) {
        self.used.insert(id.as_str().to_string());
    }

    pub fn contains(&self, id: &str) -> bool {
        self.used.contains(id)
    }

    /// 分配一个未占用的新短码。
    pub fn alloc(&mut self) -> StableId {
        loop {
            let id = StableId::from_seed(self.next);
            self.next += 1;
            if self.used.insert(id.as_str().to_string()) {
                return id;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_unique() {
        let mut alloc = SidAllocator::new();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..10_000 {
            let id = alloc.alloc();
            assert_eq!(id.as_str().len(), 6);
            assert!(id
                .as_str()
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
            assert!(seen.insert(id.as_str().to_string()), "collision: {id}");
        }
        assert_eq!(StableId::from_seed(0), StableId::from_seed(0));
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(StableId::parse("a7f3c2").is_some());
        assert!(StableId::parse("A7F3C2").is_none());
        assert!(StableId::parse("").is_none());
        assert!(StableId::parse("带中文").is_none());
    }
}
