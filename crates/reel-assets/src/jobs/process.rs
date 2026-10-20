//! Windows process creation with atomic assignment to a kill-on-close Job Object.
use super::*;
use std::{
    ffi::{OsStr, OsString},
    os::windows::{ffi::OsStrExt, io::AsRawHandle},
    ptr,
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
impl Drop for Handles {
    fn drop(&mut self) {
        // Closing the last job handle kills all descendants before admission ends.
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
        InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut size);
        let mut buffer = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
        let list = buffer.as_mut_ptr().cast();
        if InitializeProcThreadAttributeList(list, 1, 0, &mut size) == 0 {
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
            return Err(format!("Job step exited {code}; inspect {}", log.display()).into());
        }
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
