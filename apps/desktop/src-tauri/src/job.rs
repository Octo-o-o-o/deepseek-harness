//! Windows Job Object wrapper. Not locally verified on this machine.

#![cfg(windows)]

use std::io;
use std::os::windows::io::AsRawHandle;
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
}

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
