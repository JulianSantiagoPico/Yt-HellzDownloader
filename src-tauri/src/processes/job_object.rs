#[cfg(windows)]
use std::os::windows::io::RawHandle;
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    },
};

pub struct JobObjectHandle {
    #[cfg(windows)]
    handle: HANDLE,
}

// Safety: Job Object handles in Windows can be safely passed between threads.
unsafe impl Send for JobObjectHandle {}
unsafe impl Sync for JobObjectHandle {}

impl JobObjectHandle {
    pub fn new() -> Result<Self, String> {
        #[cfg(windows)]
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err("Fallo al crear el Job Object de Windows".into());
            }

            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

            let res = SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );

            if res == 0 {
                CloseHandle(handle);
                return Err(
                    "Fallo al configurar JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE en el Job Object"
                        .into(),
                );
            }

            Ok(Self { handle })
        }

        #[cfg(not(windows))]
        Ok(Self {})
    }

    #[cfg(windows)]
    pub fn assign_process(&self, process_handle: RawHandle) -> Result<(), String> {
        unsafe {
            let res = AssignProcessToJobObject(self.handle, process_handle as HANDLE);
            if res == 0 {
                return Err("Fallo al asignar el proceso al Job Object de Windows".into());
            }
            Ok(())
        }
    }

    #[cfg(not(windows))]
    pub fn assign_process(&self, _process_handle: RawHandle) -> Result<(), String> {
        Ok(())
    }

    pub fn terminate(&self, exit_code: u32) -> Result<(), String> {
        #[cfg(windows)]
        unsafe {
            if !self.handle.is_null() {
                let res = TerminateJobObject(self.handle, exit_code);
                if res == 0 {
                    return Err("Fallo al terminar los procesos del Job Object".into());
                }
            }
        }
        let _ = exit_code;
        Ok(())
    }
}

impl Drop for JobObjectHandle {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            if !self.handle.is_null() {
                CloseHandle(self.handle);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_object_creation() {
        let job = JobObjectHandle::new();
        assert!(job.is_ok());
    }

    #[test]
    #[cfg(windows)]
    fn test_job_object_terminates_child_processes() {
        use std::os::windows::io::AsRawHandle;
        use std::process::Command;

        let job = JobObjectHandle::new().expect("Failed to create Job Object");

        // Spawn a process that would otherwise outlive or keep running
        let mut child = Command::new("cmd.exe")
            .args(["/c", "ping -n 10 127.0.0.1 > nul"])
            .spawn()
            .expect("Failed to spawn test process");

        let assign_res = job.assign_process(child.as_raw_handle());
        assert!(assign_res.is_ok(), "Failed to assign process to Job Object");

        // Terminate the Job Object
        let term_res = job.terminate(42);
        assert!(term_res.is_ok(), "Failed to terminate Job Object");

        let status = child.wait().expect("Failed to wait on child");
        // Process must have been terminated
        assert!(!status.success());
    }
}
