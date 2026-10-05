//! Windows process trees (ARCHITECTURE §5.3): every pane's process is put in
//! a Job Object with `KILL_ON_JOB_CLOSE`. Closing the pane terminates the job,
//! so children it started go too, and if Tandem itself dies the OS closes the
//! handle and ends the tree, so a crash leaves no orphans on Windows.

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

pub struct Job(HANDLE);

// SAFETY: a job handle is a kernel object reference; the calls made on it
// here are thread-safe.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    /// A kill-on-close job holding `pid`. `None` if the OS refuses (the
    /// caller then falls back to killing the root process).
    pub fn for_process(pid: u32) -> Option<Job> {
        // SAFETY: plain Win32 calls with owned handles, each checked; the
        // limit struct is zero-initialised as the API expects.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            let assigned = !process.is_null() && AssignProcessToJobObject(job, process) != 0;
            if !process.is_null() {
                CloseHandle(process);
            }
            if ok == 0 || !assigned {
                CloseHandle(job);
                return None;
            }
            Some(Job(job))
        }
    }

    /// Ends every process in the job.
    pub fn terminate(&self) {
        // SAFETY: the handle is owned and open until drop.
        unsafe {
            TerminateJobObject(self.0, 1);
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: closing the handle we own exactly once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
