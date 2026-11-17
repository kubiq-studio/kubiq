//! A cell's process and everything it starts, as one group: started together, interrupted
//! together, killed together — on every OS the host runs on. Behind the `process` feature, the
//! one part of this crate that does I/O; the notebook's kernel manager and the runbook's runner
//! both start their children through it.
//!
//! - **Unix**: the child leads a new process group (`process_group(0)`); interrupt is SIGINT to
//!   the group, terminate SIGTERM, kill SIGKILL.
//! - **Windows**: the child is put in a Job object that kills every process in it when the job is
//!   closed or terminated — a tree kill with no orphan (`taskkill /T` races a child that has just
//!   forked). There is no SIGINT: a Jupyter kernel is interrupted through the inheritable event
//!   whose handle it reads from `JPY_INTERRUPT_EVENT` ([`InterruptEvent`]); anything else gets
//!   `CTRL_BREAK_EVENT` to its own process group (`CREATE_NEW_PROCESS_GROUP`).

use std::io;
use std::process::{Child, Command};

/// The group a [`spawn`] started. Dropping it does not kill anything: call [`Group::kill_tree`].
pub struct Group {
    pid: u32,
    #[cfg(windows)]
    job: win::Job,
}

/// Start `cmd` as the leader of a group of its own.
pub fn spawn(cmd: &mut Command) -> io::Result<(Child, Group)> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
        let child = cmd.spawn()?;
        let pid = child.id();
        Ok((child, Group { pid }))
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
        let child = cmd.spawn()?;
        let job = win::Job::new()?;
        job.assign(&child)?;
        let pid = child.id();
        Ok((child, Group { pid, job }))
    }
}

impl Group {
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Ctrl-C to the group: SIGINT on Unix, `CTRL_BREAK_EVENT` on Windows.
    pub fn interrupt(&self) -> io::Result<()> {
        #[cfg(unix)]
        let result = unix::signal(self.pid, unix::SIGINT);
        #[cfg(windows)]
        let result = win::ctrl_break(self.pid);
        result
    }

    /// Ask the group to end: SIGTERM on Unix; on Windows there is no polite form, so the tree goes.
    pub fn terminate(&self) -> io::Result<()> {
        #[cfg(unix)]
        let result = unix::signal(self.pid, unix::SIGTERM);
        #[cfg(windows)]
        let result = self.job.terminate();
        result
    }

    /// Kill the leader and everything it started.
    pub fn kill_tree(&self) -> io::Result<()> {
        #[cfg(unix)]
        let result = unix::signal(self.pid, unix::SIGKILL);
        #[cfg(windows)]
        let result = self.job.terminate();
        result
    }
}

#[cfg(unix)]
mod unix {
    pub const SIGINT: i32 = libc::SIGINT;
    pub const SIGTERM: i32 = libc::SIGTERM;
    pub const SIGKILL: i32 = libc::SIGKILL;

    pub fn signal(pid: u32, sig: i32) -> std::io::Result<()> {
        // SAFETY: `kill` reads its two integers; a negative pid names the process group the child
        // leads (spawned with `process_group(0)`).
        let sent = unsafe { libc::kill(-(pid as i32), sig) };
        if sent == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

/// The event a Jupyter kernel on Windows waits on for an interrupt: created inheritable, its
/// handle's number handed to the kernel in `JPY_INTERRUPT_EVENT` (`ipykernel`'s
/// `ParentPollerWindows`). Elsewhere it is nothing, and interrupt is a signal.
pub struct InterruptEvent {
    #[cfg(windows)]
    handle: win::Handle,
}

impl InterruptEvent {
    /// A new event; `None` where the platform has none (Unix) or creating one failed.
    pub fn new() -> Option<InterruptEvent> {
        #[cfg(windows)]
        let result = win::Handle::event().map(|handle| InterruptEvent { handle });
        #[cfg(not(windows))]
        let result = None;
        result
    }

    /// The variables the kernel's environment carries: `JPY_INTERRUPT_EVENT` and
    /// `JPY_PARENT_PID`.
    pub fn env(&self) -> Vec<(String, String)> {
        #[cfg(windows)]
        let result = vec![
            ("JPY_INTERRUPT_EVENT".into(), (self.handle.0 as usize).to_string()),
            ("JPY_PARENT_PID".into(), std::process::id().to_string()),
        ];
        #[cfg(not(windows))]
        let result = Vec::new();
        result
    }

    /// Set the event: the kernel raises `KeyboardInterrupt` in the running cell.
    pub fn fire(&self) -> io::Result<()> {
        #[cfg(windows)]
        let result = self.handle.set();
        #[cfg(not(windows))]
        let result = Err(io::Error::other("no interrupt event on this platform"));
        result
    }
}

#[cfg(windows)]
mod win {
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent};

    /// An owned kernel handle, closed on drop.
    pub struct Handle(pub HANDLE);

    // SAFETY: a kernel object handle is a process-wide value usable from any thread.
    unsafe impl Send for Handle {}
    unsafe impl Sync for Handle {}

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the handle was returned by a Create* call and is closed once, here.
            unsafe { CloseHandle(self.0) };
        }
    }

    fn check(ok: i32) -> io::Result<()> {
        if ok != 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
    }

    impl Handle {
        /// An inheritable, auto-reset, unsignalled event.
        pub fn event() -> Option<Handle> {
            let attrs = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: std::ptr::null_mut(),
                bInheritHandle: 1,
            };
            // SAFETY: `attrs` outlives the call; a null name makes an anonymous event.
            let h = unsafe { CreateEventW(&attrs, 0, 0, std::ptr::null()) };
            (!h.is_null()).then_some(Handle(h))
        }

        pub fn set(&self) -> io::Result<()> {
            // SAFETY: an event handle this struct owns.
            check(unsafe { SetEvent(self.0) })
        }
    }

    /// A Job object that kills its processes when terminated or closed.
    pub struct Job(Handle);

    impl Job {
        pub fn new() -> io::Result<Job> {
            // SAFETY: no attributes, no name.
            let h = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if h.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = Job(Handle(h));
            // SAFETY: a plain-old-data struct, zero is its documented empty value.
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: `info` is the struct the class names, its size passed alongside.
            check(unsafe {
                SetInformationJobObject(
                    job.0.0,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            })?;
            Ok(job)
        }

        pub fn assign(&self, child: &Child) -> io::Result<()> {
            // SAFETY: the child's process handle is valid while `child` is alive.
            check(unsafe { AssignProcessToJobObject(self.0.0, child.as_raw_handle() as HANDLE) })
        }

        pub fn terminate(&self) -> io::Result<()> {
            // SAFETY: a job handle this struct owns.
            check(unsafe { TerminateJobObject(self.0.0, 1) })
        }
    }

    pub fn ctrl_break(pid: u32) -> io::Result<()> {
        // SAFETY: plain integers; the group id is the leader's pid (`CREATE_NEW_PROCESS_GROUP`).
        check(unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) })
    }
}
