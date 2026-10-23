//! **Children that die with this process** - however it ends.
//!
//! A server the editor starts (its Servers panel, its local test agent) is a
//! child process holding a port. `kill_on_drop` stops it when the editor
//! shuts down cleanly; a crash or a kill of the editor runs no destructor, so
//! the server lived on, holding its port, and the next Start failed on it
//! until somebody ended it by hand (LIMITS P3).
//!
//! On Windows every child is put in one **job object** with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, held for the life of this process:
//! when the process ends - any way at all - the system closes the handle and
//! ends every process in the job. Elsewhere it does nothing: on Linux the
//! agent runs under systemd, which ends its whole control group when it
//! stops, and a crashed editor there still leaves its children (LIMITS P3).

/// Put `child` in this process's job, so it ends when this process does.
/// Returns whether it was.
pub fn tie(child: &std::process::Child) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        imp::job().is_some_and(|job| job.adopt(child.as_raw_handle()))
    }
    #[cfg(not(windows))]
    {
        let _ = child;
        false
    }
}

/// [`tie`] for a tokio child.
pub fn tie_tokio(child: &tokio::process::Child) -> bool {
    #[cfg(windows)]
    {
        child.raw_handle().is_some_and(|h| imp::job().is_some_and(|job| job.adopt(h)))
    }
    #[cfg(not(windows))]
    {
        let _ = child;
        false
    }
}

#[cfg(windows)]
pub(crate) mod imp {
    use std::sync::OnceLock;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
    };

    /// A job whose processes end when its last handle closes.
    pub struct Job(HANDLE);

    // A job handle is a kernel object handle, usable from any thread.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        pub fn new() -> Option<Job> {
            // SAFETY: plain Win32 calls on handles this owns; the info struct
            // is zeroed and then filled as the API documents.
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if handle.is_null() {
                    return None;
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let set = SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if set == 0 {
                    CloseHandle(handle);
                    return None;
                }
                Some(Job(handle))
            }
        }

        pub fn adopt(&self, process: std::os::windows::io::RawHandle) -> bool {
            // SAFETY: `process` is a live process handle the caller holds.
            unsafe { AssignProcessToJobObject(self.0, process as HANDLE) != 0 }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: the handle is ours and closed once.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    /// This process's job: made the first time a child is tied, and never
    /// closed but by the process ending.
    pub fn job() -> Option<&'static Job> {
        static JOB: OnceLock<Option<Job>> = OnceLock::new();
        JOB.get_or_init(|| {
            let job = Job::new();
            if job.is_none() {
                log::warn!("[fleet] no job object: a server started here outlives a crash of this process");
            }
            job
        })
        .as_ref()
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::imp::Job;

    /// The job's last handle closing - which is what this process ending
    /// does - ends the child.
    #[test]
    fn a_child_in_the_job_ends_when_the_job_closes() {
        use std::os::windows::io::AsRawHandle;
        let mut child = std::process::Command::new("ping")
            .args(["-n", "60", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let job = Job::new().expect("a job object");
        assert!(job.adopt(child.as_raw_handle()));
        assert!(child.try_wait().unwrap().is_none(), "running");
        drop(job);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while child.try_wait().unwrap().is_none() {
            assert!(std::time::Instant::now() < deadline, "the child outlived its job");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}
