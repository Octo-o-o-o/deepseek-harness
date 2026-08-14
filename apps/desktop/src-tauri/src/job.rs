//! Windows process FFI: Job Object wrapper, reader cancellation, and process
//! creation times. Not locally verified on this machine beyond `cargo test`.

#![cfg(windows)]

use std::io;
use std::os::windows::io::{AsRawHandle, RawHandle};
use std::process::Child;

/// Owned Job Object that kills its members when dropped.
pub struct JobObject {
    handle: *mut core::ffi::c_void,
}

unsafe impl Send for JobObject {}

const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: u32 = 9;

#[repr(C)]
struct IoCounters {
    read_op: u64,
    write_op: u64,
    other_op: u64,
    read_tx: u64,
    write_tx: u64,
    other_tx: u64,
}

#[repr(C)]
struct JobObjectBasicLimitInformation {
    per_process_user_time: i64,
    per_job_user_time: i64,
    limit_flags: u32,
    min_working_set: usize,
    max_working_set: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}

#[repr(C)]
struct JobObjectExtendedLimitInformation {
    basic: JobObjectBasicLimitInformation,
    io: IoCounters,
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory: usize,
    peak_job_memory: usize,
}

#[link(name = "kernel32")]
extern "system" {
    fn CreateJobObjectW(
        security: *mut core::ffi::c_void,
        name: *const u16,
    ) -> *mut core::ffi::c_void;
    fn SetInformationJobObject(
        job: *mut core::ffi::c_void,
        class: u32,
        info: *mut core::ffi::c_void,
        length: u32,
    ) -> i32;
    fn AssignProcessToJobObject(
        job: *mut core::ffi::c_void,
        process: *mut core::ffi::c_void,
    ) -> i32;
    fn TerminateJobObject(job: *mut core::ffi::c_void, exit_code: u32) -> i32;
    fn CloseHandle(handle: *mut core::ffi::c_void) -> i32;
    fn DuplicateHandle(
        source_process: *mut core::ffi::c_void,
        source: *mut core::ffi::c_void,
        target_process: *mut core::ffi::c_void,
        target: *mut *mut core::ffi::c_void,
        access: u32,
        inherit: i32,
        options: u32,
    ) -> i32;
    fn CancelIoEx(file: *mut core::ffi::c_void, overlapped: *mut core::ffi::c_void) -> i32;
    fn GetCurrentProcess() -> *mut core::ffi::c_void;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut core::ffi::c_void;
    fn GetProcessTimes(
        process: *mut core::ffi::c_void,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
}

/// 100-ns intervals since 1601-01-01, as the Win32 APIs exchange it.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct FileTime {
    low: u32,
    high: u32,
}

impl FileTime {
    fn to_u64(self) -> u64 {
        (u64::from(self.high) << 32) | u64::from(self.low)
    }
}

const DUPLICATE_SAME_ACCESS: u32 = 0x0000_0002;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x0000_1000;

impl JobObject {
    /// Create a job that kills assigned processes when the handle closes.
    ///
    /// # Returns
    /// An empty job ready for [`JobObject::assign`].
    pub fn create() -> io::Result<Self> {
        let handle = unsafe { CreateJobObjectW(core::ptr::null_mut(), core::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut info = JobObjectExtendedLimitInformation {
            basic: JobObjectBasicLimitInformation {
                per_process_user_time: 0,
                per_job_user_time: 0,
                limit_flags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                min_working_set: 0,
                max_working_set: 0,
                active_process_limit: 0,
                affinity: 0,
                priority_class: 0,
                scheduling_class: 0,
            },
            io: IoCounters {
                read_op: 0,
                write_op: 0,
                other_op: 0,
                read_tx: 0,
                write_tx: 0,
                other_tx: 0,
            },
            process_memory_limit: 0,
            job_memory_limit: 0,
            peak_process_memory: 0,
            peak_job_memory: 0,
        };
        let ok = unsafe {
            SetInformationJobObject(
                handle,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                (&mut info as *mut JobObjectExtendedLimitInformation).cast(),
                std::mem::size_of::<JobObjectExtendedLimitInformation>() as u32,
            )
        };
        if ok == 0 {
            unsafe {
                CloseHandle(handle);
            }
            return Err(io::Error::last_os_error());
        }
        Ok(Self { handle })
    }

    /// Assign `child` to this job so descendants are killed with it.
    ///
    /// # Parameters
    /// - `child`: the sidecar process just spawned.
    pub fn assign(&self, child: &Child) -> io::Result<()> {
        let ok = unsafe { AssignProcessToJobObject(self.handle, child.as_raw_handle().cast()) };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// Terminate every process in the job.
    pub fn terminate(&self) {
        unsafe {
            let _ = TerminateJobObject(self.handle, 1);
        }
    }
}

impl Drop for JobObject {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

/// Duplicate of a pipe read handle that can cancel the reader thread's
/// pending `ReadFile` from another thread. Windows pipes cannot be made
/// non-blocking, so `CancelIoEx` on a duplicated handle (same kernel file
/// object) is the standard way to unblock a shutdown-time reader.
pub struct CancelHandle {
    handle: *mut core::ffi::c_void,
}

unsafe impl Send for CancelHandle {}
unsafe impl Sync for CancelHandle {}

impl CancelHandle {
    /// Duplicate `source` so cancellation outlives the reader's own handle.
    ///
    /// # Returns
    /// A handle whose [`CancelHandle::cancel`] aborts pending reads, or
    /// `None` when duplication fails (readers then rely on EOF only).
    pub fn duplicate(source: RawHandle) -> Option<Self> {
        let mut target: *mut core::ffi::c_void = core::ptr::null_mut();
        let ok = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                source,
                GetCurrentProcess(),
                &mut target,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if ok == 0 || target.is_null() {
            None
        } else {
            Some(Self { handle: target })
        }
    }

    /// Abort every pending I/O on the underlying pipe. Idempotent.
    pub fn cancel(&self) {
        unsafe {
            let _ = CancelIoEx(self.handle, core::ptr::null_mut());
        }
    }
}

impl Drop for CancelHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

/// Creation time of an arbitrary process id, for pid-reuse-proof identity.
///
/// # Parameters
/// - `pid`: process to inspect.
///
/// # Returns
/// The creation `FILETIME` as one `u64`, or `None` when the process cannot
/// be opened (exited, elevated, another session). Callers treat `None` as
/// "identity unverifiable" and must not kill.
pub fn process_creation_time(pid: u32) -> Option<u64> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }
    let mut creation = FileTime::default();
    let mut exit = FileTime::default();
    let mut kernel = FileTime::default();
    let mut user = FileTime::default();
    let ok = unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) };
    unsafe {
        let _ = CloseHandle(process);
    }
    if ok == 0 {
        None
    } else {
        Some(creation.to_u64())
    }
}

/// Creation time of a live child we own.
///
/// # Parameters
/// - `child`: freshly spawned sidecar.
///
/// # Returns
/// Same encoding as [`process_creation_time`]; `None` on API failure.
pub fn child_creation_time(child: &Child) -> Option<u64> {
    let mut creation = FileTime::default();
    let mut exit = FileTime::default();
    let mut kernel = FileTime::default();
    let mut user = FileTime::default();
    let ok = unsafe {
        GetProcessTimes(
            child.as_raw_handle().cast(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    };
    if ok == 0 {
        None
    } else {
        Some(creation.to_u64())
    }
}
