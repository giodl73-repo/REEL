//! Windows process creation with atomic assignment to a kill-on-close Job Object.
use super::*;
use std::{
    ffi::{OsStr, OsString},
    os::windows::{ffi::OsStrExt, io::AsRawHandle},
    ptr,
    time::Instant,
};
use windows_sys::Win32::{
    Foundation::*,
    System::{JobObjects::*, Threading::*},
};

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

// Microsoft CRT argument quoting: double backslashes only before quotes/end.
fn quote(value: &str) -> String {
    let mut result = String::from("\"");
    let mut slashes = 0;
    for c in value.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        if c == '"' {
            result.push_str(&"\\".repeat(slashes * 2 + 1));
        } else {
            result.push_str(&"\\".repeat(slashes));
        }
        slashes = 0;
        result.push(c);
    }
    result.push_str(&"\\".repeat(slashes * 2));
    result.push('"');
    result
}

struct Handles {
    job: HANDLE,
    process: HANDLE,
    thread: HANDLE,
}
impl Handles {
    fn stop(&mut self) -> Result<()> {
        unsafe {
            if self.job.is_null() {
                return Ok(());
            }
            if TerminateJobObject(self.job, 1) == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            loop {
                let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
                if QueryInformationJobObject(
                    self.job,
                    JobObjectBasicAccountingInformation,
                    (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                    std::mem::size_of_val(&accounting) as u32,
                    ptr::null_mut(),
                ) == 0
                {
                    return Err(std::io::Error::last_os_error().into());
                }
                if accounting.ActiveProcesses == 0 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            CloseHandle(self.job);
            self.job = ptr::null_mut();
        }
        Ok(())
    }
}
impl Drop for Handles {
    fn drop(&mut self) {
        // Closing the last job handle kills all descendants before admission ends.
        let _ = self.stop();
        unsafe {
            if !self.job.is_null() {
                CloseHandle(self.job);
            }
            if !self.process.is_null() {
                WaitForSingleObject(self.process, INFINITE);
                CloseHandle(self.process);
            }
            if !self.thread.is_null() {
                CloseHandle(self.thread);
            }
        }
    }
}
struct Attributes(LPPROC_THREAD_ATTRIBUTE_LIST);
impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.0);
        }
    }
}

pub fn run(
    program: &Path,
    args: &[String],
    workspace: &Path,
    log: &Path,
    timeout: Duration,
    check: impl Fn() -> Result<()>,
) -> Result<()> {
    let stdout = OpenOptions::new().write(true).create_new(true).open(log)?;
    let stdin = File::open("NUL")?;
    execute(
        program, args, workspace, &stdin, &stdout, timeout, None, check,
    )
    .map_err(|error| format!("{error}; inspect {}", log.display()).into())
}

/// OS-enforced aggregate committed-memory/process bounds for one owned child
/// tree. This is not capacity admission, a disk quota or a network sandbox.
#[derive(Clone, Copy)]
pub struct QuietLimits {
    pub memory_bytes: usize,
    pub processes: u32,
}
impl QuietLimits {
    fn validate(&self) -> Result<()> {
        if !(16 * 1024 * 1024..=16_u64 * 1024 * 1024 * 1024).contains(&(self.memory_bytes as u64))
            || !(1..=256).contains(&self.processes)
        {
            return Err("Invalid contained child limits".into());
        }
        Ok(())
    }
}
/// Internal trusted execution primitive. Caller supplies held program/source
/// guards, an admitted existing workspace and cancellation/authority checks.
/// Atomically joins a kill-on-close job; suppresses stdout/stderr to NUL, without
/// creating a potentially unbounded log. All owned descendants stop before return.
/// No cache, workspace, reservation, source claim or publication is created here.
pub fn run_quiet(
    program: &Path,
    args: &[String],
    workspace: &Path,
    timeout: Duration,
    limits: QuietLimits,
    check: impl Fn() -> Result<()>,
) -> Result<()> {
    limits.validate()?;
    if !program.is_absolute()
        || !workspace.is_absolute()
        || !workspace.is_dir()
        || timeout.is_zero()
        || timeout > Duration::from_secs(MAX_STEP_TIMEOUT_SECONDS)
    {
        return Err("Invalid contained child paths or deadline".into());
    }
    check()?;
    let stdin = File::open("NUL")?;
    let stdout = OpenOptions::new().write(true).open("NUL")?;
    execute(
        program,
        args,
        workspace,
        &stdin,
        &stdout,
        timeout,
        Some(limits),
        check,
    )
}

#[allow(clippy::too_many_arguments)]
fn execute(
    program: &Path,
    args: &[String],
    workspace: &Path,
    stdin: &File,
    stdout: &File,
    timeout: Duration,
    bounded: Option<QuietLimits>,
    check: impl Fn() -> Result<()>,
) -> Result<()> {
    let program = wide(program.as_os_str());
    let cwd = wide(workspace.as_os_str());
    let line = std::iter::once(quote(&String::from_utf16_lossy(
        &program[..program.len() - 1],
    )))
    .chain(args.iter().map(|s| quote(s)))
    .collect::<Vec<_>>()
    .join(" ");
    let mut line = wide(OsStr::new(&line));
    let mut variables: BTreeMap<OsString, OsString> = std::env::vars_os()
        .filter(|(k, _)| {
            let key = k.to_string_lossy();
            !key.eq_ignore_ascii_case("TEMP")
                && !key.eq_ignore_ascii_case("TMP")
                && !key.eq_ignore_ascii_case("PATH")
        })
        .collect();
    variables.insert("TEMP".into(), workspace.as_os_str().to_owned());
    variables.insert("TMP".into(), workspace.as_os_str().to_owned());
    let mut search = vec![workspace.join("tools")];
    if let Some(path) = std::env::var_os("PATH") {
        search.extend(std::env::split_paths(&path));
    }
    variables.insert("PATH".into(), std::env::join_paths(search)?);
    let mut environment = Vec::new();
    for (key, value) in variables {
        environment.extend(key.encode_wide());
        environment.push('=' as u16);
        environment.extend(value.encode_wide());
        environment.push(0);
    }
    environment.push(0);
    // All API pointers refer to live aligned buffers. The job attribute binds
    // the process at creation, avoiding a running or suspended orphan window.
    unsafe {
        let job = CreateJobObjectW(ptr::null(), ptr::null());
        if job.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut handles = Handles {
            job,
            process: ptr::null_mut(),
            thread: ptr::null_mut(),
        };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if let Some(bounded) = bounded {
            limits.BasicLimitInformation.LimitFlags |=
                JOB_OBJECT_LIMIT_JOB_MEMORY | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
            limits.JobMemoryLimit = bounded.memory_bytes;
            limits.BasicLimitInformation.ActiveProcessLimit = bounded.processes;
        }
        if SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&limits) as u32,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut size = 0;
        InitializeProcThreadAttributeList(ptr::null_mut(), 2, 0, &mut size);
        let mut buffer = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
        let list = buffer.as_mut_ptr().cast();
        if InitializeProcThreadAttributeList(list, 2, 0, &mut size) == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let _attributes = Attributes(list);
        if UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
            (&job as *const HANDLE).cast(),
            std::mem::size_of::<HANDLE>(),
            ptr::null_mut(),
            ptr::null(),
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        for handle in [stdout.as_raw_handle(), stdin.as_raw_handle()] {
            if SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        let inherited = [stdout.as_raw_handle(), stdin.as_raw_handle()];
        if UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            inherited.as_ptr().cast(),
            std::mem::size_of_val(&inherited),
            ptr::null_mut(),
            ptr::null(),
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = std::mem::size_of_val(&startup) as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = stdin.as_raw_handle();
        startup.StartupInfo.hStdOutput = stdout.as_raw_handle();
        startup.StartupInfo.hStdError = stdout.as_raw_handle();
        startup.lpAttributeList = list;
        let mut process = PROCESS_INFORMATION::default();
        if CreateProcessW(
            program.as_ptr(),
            line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            1,
            CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
            environment.as_ptr().cast(),
            cwd.as_ptr(),
            &startup.StartupInfo,
            &mut process,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        handles.process = process.hProcess;
        handles.thread = process.hThread;
        let started = Instant::now();
        loop {
            match WaitForSingleObject(handles.process, 250) {
                WAIT_OBJECT_0 => break,
                WAIT_TIMEOUT => (),
                _ => return Err(std::io::Error::last_os_error().into()),
            }
            if started.elapsed() > timeout {
                return Err("Job step timed out; process group terminated".into());
            }
            check()?;
        }
        let mut code = 0;
        if GetExitCodeProcess(handles.process, &mut code) == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if code != 0 {
            return Err(format!("Job step exited {code}").into());
        }
        handles.stop()?;
    }
    check()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_arguments_preserve_quotes_and_trailing_backslashes() {
        assert_eq!(quote("plain"), "\"plain\"");
        assert_eq!(quote("a b\\"), "\"a b\\\\\"");
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
    }
}
