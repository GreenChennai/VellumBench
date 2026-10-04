//! 插件进程沙箱最小版(PLG-01 / PLG-08;ADR-0052)。
//!
//! **裁定(ADR-0052)**:Windows **Job Object** 最小沙箱 ——
//! - `KILL_ON_JOB_CLOSE`:句柄随 [`JobGuard`] Drop 关闭,job 内全部进程
//!   (含插件派生的**孙进程**)被内核终止 —— 宿主崩溃/退出不再留孤儿;
//! - `PROCESS_MEMORY` 限额([`PLUGIN_MEMORY_LIMIT_BYTES`]):失控分配在
//!   插件进程内失败,不拖垮宿主;
//! - `ACTIVE_PROCESS` 限额([`PLUGIN_MAX_PROCESSES`]):防 fork 炸弹。
//!
//! 子进程创建后立即挂入 job(孙进程自动继承 job)。assign 失败 → 降级:
//! 进程照常运行,但降级原因经 [`PluginProcess::sandbox_note`] 如实上报
//! (RB-06:降级必须可观测;残余风险清单在 ADR-0052)。
//!
//! 被否决:WASM/WASI 路线(重估不做,理由在 ADR-0052)、低完整性令牌
//! (AppContainer 依赖打包/ACL 布局,超出本批最小版,列为后续)。
//!
//! 零新依赖:Win32 API 以 `extern "system"` 手工声明(仅 5 个函数 +
//! 3 个结构体),维持插件宿主「零新外部依赖」纪律(ADR-VB-L12)。

/// 插件进程树的内存上限(字节;1 GiB —— 原生插件做图像/几何运算的
/// 合理上限;超出即分配失败,不影响宿主)。
pub const PLUGIN_MEMORY_LIMIT_BYTES: usize = 1024 * 1024 * 1024;
/// 插件进程树的进程数上限(防 fork 炸弹)。
pub const PLUGIN_MAX_PROCESSES: u32 = 32;

/// Job 句柄守卫:存活期 = 插件进程生命周期;Drop 关闭句柄 →
/// `KILL_ON_JOB_CLOSE` 终止 job 内全部进程。
#[derive(Debug)]
pub struct JobGuard {
    #[cfg(windows)]
    handle: *mut std::ffi::c_void,
}

// SAFETY:句柄只在本结构的方法/Drop 中使用,不跨线程解引用;
// PluginProcess 经 Arc 在宿主线程与握手线程间共享,需要 Send+Sync。
unsafe impl Send for JobGuard {}
unsafe impl Sync for JobGuard {}

/// 把子进程挂入专用 Job Object。失败 → Err(调用方降级并记录)。
#[cfg(windows)]
pub fn attach(child: &std::process::Child) -> Result<JobGuard, String> {
    use std::os::windows::io::AsRawHandle;

    const JOB_OBJECT_LIMIT_ACTIVE_PROCESS: u32 = 0x0000_0008;
    const JOB_OBJECT_LIMIT_PROCESS_MEMORY: u32 = 0x0000_0100;
    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
    const JOBOBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: i32 = 9; // JobObjectExtendedLimitInformation

    #[repr(C)]
    #[derive(Default)]
    struct IoCounters {
        read_operation_count: u64,
        write_operation_count: u64,
        other_operation_count: u64,
        read_transfer_count: u64,
        write_transfer_count: u64,
        other_transfer_count: u64,
    }

    #[repr(C)]
    struct BasicLimitInformation {
        per_process_user_time_limit: i64,
        per_job_user_time_limit: i64,
        limit_flags: u32,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    struct ExtendedLimitInformation {
        basic: BasicLimitInformation,
        io_info: IoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateJobObjectW(
            lp_job_attributes: *mut std::ffi::c_void,
            lp_name: *const u16,
        ) -> *mut std::ffi::c_void;
        fn SetInformationJobObject(
            h_job: *mut std::ffi::c_void,
            job_object_information_class: i32,
            lp_job_object_information: *mut std::ffi::c_void,
            cb_job_object_information_length: u32,
        ) -> i32;
        fn AssignProcessToJobObject(
            h_job: *mut std::ffi::c_void,
            h_process: *mut std::ffi::c_void,
        ) -> i32;
        fn CloseHandle(h_object: *mut std::ffi::c_void) -> i32;
    }

    // SAFETY:CreateJobObjectW 无副作用,失败返回空句柄。
    let handle = unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
    if handle.is_null() {
        return Err("CreateJobObjectW 失败".into());
    }
    let mut info = ExtendedLimitInformation {
        basic: BasicLimitInformation {
            per_process_user_time_limit: 0,
            per_job_user_time_limit: 0,
            limit_flags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                | JOB_OBJECT_LIMIT_PROCESS_MEMORY
                | JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
            minimum_working_set_size: 0,
            maximum_working_set_size: 0,
            active_process_limit: PLUGIN_MAX_PROCESSES,
            affinity: 0,
            priority_class: 0,
            scheduling_class: 0,
        },
        io_info: IoCounters::default(),
        process_memory_limit: PLUGIN_MEMORY_LIMIT_BYTES,
        job_memory_limit: PLUGIN_MEMORY_LIMIT_BYTES,
        peak_process_memory_used: 0,
        peak_job_memory_used: 0,
    };
    // SAFETY:info 是完整初始化的 ExtendedLimitInformation,类别与长度匹配。
    let ok = unsafe {
        SetInformationJobObject(
            handle,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
            &mut info as *mut _ as *mut std::ffi::c_void,
            std::mem::size_of::<ExtendedLimitInformation>() as u32,
        )
    };
    if ok == 0 {
        // SAFETY:handle 是刚创建的合法句柄。
        unsafe { CloseHandle(handle) };
        return Err("SetInformationJobObject 失败(限额未设置)".into());
    }
    // SAFETY:child 存活期内句柄有效;assign 后子进程(及其子孙)受 job 管辖。
    let ok =
        unsafe { AssignProcessToJobObject(handle, child.as_raw_handle() as *mut std::ffi::c_void) };
    if ok == 0 {
        // SAFETY:同上。
        unsafe { CloseHandle(handle) };
        return Err("AssignProcessToJobObject 失败(宿主可能已处于不兼容的 job 内)".into());
    }
    Ok(JobGuard { handle })
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            #[link(name = "kernel32")]
            extern "system" {
                fn CloseHandle(h_object: *mut std::ffi::c_void) -> i32;
            }
            // SAFETY:句柄在本结构存活期内不被他处使用/复制。
            unsafe { CloseHandle(self.handle) };
            // KILL_ON_JOB_CLOSE:job 内全部进程(含孙进程)被内核终止
        }
    }
}

/// 非 Windows 平台:本批最小版不提供进程树沙箱,恒降级(原因如实上报,
/// 宿主记入插件日志环;RB-06)。
#[cfg(not(windows))]
pub fn attach(_child: &std::process::Child) -> Result<JobGuard, String> {
    Err("当前平台暂无 Job Object 沙箱(Windows 专属)".into())
}
