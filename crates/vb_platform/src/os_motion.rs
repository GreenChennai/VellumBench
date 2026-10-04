//! 系统动效偏好探测(S5 清单 ④,§8.10 reduced-motion)的 OS 落点。
//!
//! Windows 读 `SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION)`(「设置 →
//! 辅助功能 → 视觉效果 → 动画效果」,即「减少动态效果」的反面)。FFI 走
//! **手写 `extern` 块**:user32 是系统库恒在,不引 winapi/windows-sys
//! (零新依赖纪律)。
//!
//! 这是**系统级**设置,与宿主框架(egui/gpui)无关 —— egui/gpui 两
//! backend 各自的 [`MotionPreferenceProbe`] 实现都落在 [`OsMotionProbe`],
//! 差别只在归属命名空间与构造入参;非 Windows 平台暂无探测口,诚实
//! 返回 `true`(允许动画;见 trait 文档的 fail-open 口径)。

use crate::traits::MotionPreferenceProbe;

/// Windows:`SPI_GETCLIENTAREAANIMATION`(与 windows-rs 0.61 同值;
/// `pvParam = *mut BOOL`,`BOOL = i32`)。
#[cfg(windows)]
const SPI_GETCLIENTAREAANIMATION: u32 = 0x1042;

#[cfg(windows)]
type WinBool = i32;

#[cfg(windows)]
#[link(name = "user32")]
extern "system" {
    fn SystemParametersInfoW(
        uiaction: u32,
        uiparam: u32,
        pvparam: *mut core::ffi::c_void,
        fwinini: u32,
    ) -> WinBool;
}

/// OS 动效偏好探测(Windows 实读系统偏好;其余平台 fail-open)。
///
/// 不缓存:启动只调一次,`SystemParametersInfoW` 本身 O(1),缓存反而
/// 让「运行中改系统设置」永远生效不了 —— 得不偿失。
#[derive(Debug, Default, Clone, Copy)]
pub struct OsMotionProbe;

impl OsMotionProbe {
    pub fn new() -> Self {
        Self
    }
}

impl MotionPreferenceProbe for OsMotionProbe {
    fn animations_enabled(&self) -> bool {
        #[cfg(windows)]
        {
            let mut enabled: WinBool = 1; // 预置「允许」:探测失败保持默认
            let ok = unsafe {
                SystemParametersInfoW(
                    SPI_GETCLIENTAREAANIMATION,
                    0,
                    (std::ptr::from_mut(&mut enabled)).cast::<core::ffi::c_void>(),
                    0,
                )
            };
            // ok == 0 → SPI 失败,保持预置 true(fail-open,不替用户关动效)
            ok == 0 || enabled != 0
        }
        #[cfg(not(windows))]
        {
            // 非 Windows 暂无跨 crate 依赖之外的探测口:诚实 fail-open。
            // macOS NSWorkspace.accessibilityDisplayShouldReduceMotion /
            // Linux XSETTINGS Net/EnableAnimations 落地下批(届时按
            // `#[cfg]` 分支补,trait 面不变)。
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 冒烟:真实调用不 panic、返回值是合法布尔(Windows 上读的是本机
    /// 系统偏好,值随机器不同,断言只钉「可调用 + fail-open 方向」)。
    #[test]
    fn os_probe_is_callable_and_fail_open() {
        let probe = OsMotionProbe::new();
        // 唯一可静态断言的:非 Windows 恒 true(fail-open 的规格面)
        #[cfg(not(windows))]
        assert!(probe.animations_enabled());
        #[cfg(windows)]
        {
            let enabled = probe.animations_enabled();
            let _ = enabled; // 值随机器;调用成功不 panic 即本测的目的
        }
    }
}
