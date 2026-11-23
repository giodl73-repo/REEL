//! Explicit blocking console primitives. Use only on owned I/O threads, never
//! the audio callback or <=20 ms controller. No endpoint or playback is opened.
use super::{input::InputSender, parent::LayoutObservation, *};
use std::{
    fs::File,
    os::windows::io::{AsHandle, AsRawHandle},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};
use windows::Win32::{
    Foundation::*,
    System::{Console::*, IO::CancelSynchronousIo, Performance::QueryPerformanceCounter},
};

fn handle(file: &File) -> HANDLE {
    HANDLE(file.as_raw_handle())
}
struct Environment {
    input: File,
    output: File,
    input_mode: CONSOLE_MODE,
    output_mode: CONSOLE_MODE,
    input_cp: u32,
    output_cp: u32,
}
impl Drop for Environment {
    fn drop(&mut self) {
        unsafe {
            let _ = SetConsoleMode(handle(&self.input), self.input_mode);
            let _ = SetConsoleMode(handle(&self.output), self.output_mode);
            let _ = SetConsoleCP(self.input_cp);
            let _ = SetConsoleOutputCP(self.output_cp);
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleRead {
    Queued,
    Invalid,
    Eof,
}
/// Holds explicit attached-console handles and restores process console settings
/// on actual destruction. Opening/rendering/reading/destruction may block.
pub struct ConsoleSurface {
    environment: Arc<Environment>,
    reader_started: bool,
    left: i16,
    top: i16,
    expected: Option<Vec<u16>>,
    pending: [u16; 64],
    offset: usize,
    length: usize,
    received_counter: i64,
}
impl ConsoleSurface {
    /// Start the sole cooked reader after the transcript has been drawn.
    /// The renderer retains its own surface and may update the status row
    /// while the reader blocks. The caller owns the input queue and must join
    /// this reader off the controller before releasing console ownership.
    pub fn start_reader(&mut self, mut input: InputSender) -> Result<ConsoleReader> {
        ensure!(
            self.expected.is_some() && !self.reader_started && self.offset == self.length,
            "installation console reader unavailable"
        );
        let mut surface = Self {
            environment: self.environment.clone(),
            reader_started: false,
            left: self.left,
            top: self.top,
            expected: None,
            pending: [0; 64],
            offset: 0,
            length: 0,
            received_counter: 0,
        };
        let stop = Arc::new(AtomicBool::new(false));
        let requested = stop.clone();
        let thread = std::thread::Builder::new()
            .name("reel-console-input".into())
            .spawn(move || {
                while !requested.load(Ordering::Acquire) {
                    let result = surface.read(&mut input);
                    if requested.load(Ordering::Acquire) {
                        break;
                    }
                    if result? == ConsoleRead::Eof {
                        return Ok(Some(surface.received_counter()));
                    }
                }
                Ok(None)
            })?;
        self.reader_started = true;
        Ok(ConsoleReader {
            stop,
            thread: Some(thread),
        })
    }
    /// Original QPC of the last native read, including EOF. Never resample it
    /// when converting an input termination into the owner's Quit request.
    pub fn received_counter(&self) -> i64 {
        self.received_counter
    }
    pub fn open() -> Result<Self> {
        // These names refer to this process's actual attached console, not a
        // redirected stdio pipe/file. No console is allocated or substituted.
        let input = File::options().read(true).write(true).open("CONIN$")?;
        let output = File::options().read(true).write(true).open("CONOUT$")?;
        let (mut input_mode, mut output_mode) = (CONSOLE_MODE::default(), CONSOLE_MODE::default());
        unsafe {
            GetConsoleMode(handle(&input), &mut input_mode)?;
            GetConsoleMode(handle(&output), &mut output_mode)?;
        }
        let environment = Environment {
            input,
            output,
            input_mode,
            output_mode,
            input_cp: unsafe { GetConsoleCP() },
            output_cp: unsafe { GetConsoleOutputCP() },
        };
        ensure!(
            environment.input_cp != 0 && environment.output_cp != 0,
            "installation console code pages unavailable"
        );
        unsafe {
            SetConsoleCP(65001)?;
            SetConsoleOutputCP(65001)?;
            SetConsoleMode(
                handle(&environment.input),
                (input_mode | ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT)
                    & !ENABLE_VIRTUAL_TERMINAL_INPUT,
            )?;
            SetConsoleMode(
                handle(&environment.output),
                output_mode | ENABLE_PROCESSED_OUTPUT,
            )?;
        }
        let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
        unsafe {
            GetConsoleScreenBufferInfo(handle(&environment.output), &mut info)?;
        }
        ensure!(
            info.srWindow.Right - info.srWindow.Left + 1 >= 80
                && info.srWindow.Bottom - info.srWindow.Top + 1 >= 30,
            "installation console requires 80 columns and 30 rows"
        );
        Ok(Self {
            environment: Arc::new(environment),
            reader_started: false,
            left: info.srWindow.Left,
            top: info.srWindow.Top,
            expected: None,
            pending: [0; 64],
            offset: 0,
            length: 0,
            received_counter: 0,
        })
    }
    pub fn layout(&self) -> Result<LayoutObservation> {
        let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
        let (mut input_mode, mut output_mode) = (CONSOLE_MODE::default(), CONSOLE_MODE::default());
        unsafe {
            GetConsoleScreenBufferInfo(handle(&self.environment.output), &mut info)?;
            GetConsoleMode(handle(&self.environment.input), &mut input_mode)?;
            GetConsoleMode(handle(&self.environment.output), &mut output_mode)?;
        }
        let required = ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT;
        ensure!(
            input_mode & required == required
                && input_mode & ENABLE_VIRTUAL_TERMINAL_INPUT == CONSOLE_MODE(0)
                && output_mode & ENABLE_PROCESSED_OUTPUT != CONSOLE_MODE(0),
            "installation console mode changed"
        );
        let layout = LayoutObservation {
            columns: (info.srWindow.Right - info.srWindow.Left + 1) as u16,
            rows: (info.srWindow.Bottom - info.srWindow.Top + 1) as u16,
            input_code_page: unsafe { GetConsoleCP() },
            output_code_page: unsafe { GetConsoleOutputCP() },
        };
        ensure!(
            layout.columns >= 80
                && layout.rows >= 30
                && layout.input_code_page == 65001
                && layout.output_code_page == 65001
                && info.srWindow.Left == self.left
                && info.srWindow.Top == self.top,
            "installation console layout changed"
        );
        Ok(layout)
    }
    fn write_cells(&self, top: i16, rows: i16, values: &[u16]) -> Result<()> {
        self.layout()?;
        let cells: Vec<CHAR_INFO> = values
            .iter()
            .map(|value| CHAR_INFO {
                Char: CHAR_INFO_0 {
                    UnicodeChar: *value,
                },
                Attributes: 7,
            })
            .collect();
        let mut rect = SMALL_RECT {
            Left: self.left,
            Top: top,
            Right: self.left + 79,
            Bottom: top + rows - 1,
        };
        unsafe {
            WriteConsoleOutputW(
                handle(&self.environment.output),
                cells.as_ptr(),
                COORD { X: 80, Y: rows },
                COORD { X: 0, Y: 0 },
                &mut rect,
            )?;
        }
        ensure!(
            rect.Left == self.left
                && rect.Top == top
                && rect.Right == self.left + 79
                && rect.Bottom == top + rows - 1,
            "installation console write clipped"
        );
        Ok(())
    }
    pub fn transcript(&mut self, original: &str) -> Result<LayoutObservation> {
        let text = super::super::text(original.as_bytes().to_vec())?;
        let mut values = vec![b' ' as u16; 80 * 20];
        for (row, line) in text.lines().enumerate() {
            for (column, value) in line.encode_utf16().enumerate() {
                values[row * 80 + column] = value;
            }
        }
        // Reserve rows 1..20 for text, row 21 for status. Input uses a fixed
        // lower area: classic cooked echo does not honor VT scrolling margins.
        self.write_cells(self.top, 20, &values)?;
        self.expected = Some(values);
        self.verify_transcript()?;
        self.layout()
    }
    pub fn verify_transcript(&self) -> Result<()> {
        self.layout()?;
        let expected = self
            .expected
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("installation transcript not displayed"))?;
        let mut cells = vec![CHAR_INFO::default(); 80 * 20];
        let mut rect = SMALL_RECT {
            Left: self.left,
            Top: self.top,
            Right: self.left + 79,
            Bottom: self.top + 19,
        };
        unsafe {
            ReadConsoleOutputW(
                handle(&self.environment.output),
                cells.as_mut_ptr(),
                COORD { X: 80, Y: 20 },
                COORD { X: 0, Y: 0 },
                &mut rect,
            )?;
        }
        ensure!(
            rect.Left == self.left
                && rect.Top == self.top
                && rect.Right == self.left + 79
                && rect.Bottom == self.top + 19,
            "installation console read clipped"
        );
        ensure!(
            cells
                .iter()
                .zip(expected)
                .all(|(cell, value)| unsafe { cell.Char.UnicodeChar == *value }),
            "installation transcript displaced by console input"
        );
        Ok(())
    }
    pub fn status(&self, status: &str) -> Result<()> {
        ensure!(
            status.len() <= 80 && status.bytes().all(|c| (32..=126).contains(&c)),
            "installation plain console status profile"
        );
        self.verify_transcript()?;
        let mut values = vec![b' ' as u16; 80];
        for (slot, value) in values.iter_mut().zip(status.encode_utf16()) {
            *slot = value;
        }
        self.write_cells(self.top + 20, 1, &values)
    }
    /// Blocking standard cooked read. Every returned chunk is stamped before
    /// parsing; queued lines keep that receipt, including multiple pasted lines.
    pub fn read(&mut self, input: &mut InputSender) -> Result<ConsoleRead> {
        ensure!(
            !self.reader_started,
            "installation console reader already owns input"
        );
        self.write_cells(self.top + 21, 9, &vec![b' ' as u16; 80 * 9])?;
        unsafe {
            SetConsoleCursorPosition(
                handle(&self.environment.output),
                COORD {
                    X: self.left,
                    Y: self.top + 21,
                },
            )?;
        }
        let mut line = [0u8; 32];
        let (mut length, mut invalid, mut eof) = (0, false, false);
        loop {
            if self.offset == self.length {
                let mut count = 0;
                unsafe {
                    ReadConsoleW(
                        handle(&self.environment.input),
                        self.pending.as_mut_ptr().cast(),
                        64,
                        &mut count,
                        None,
                    )?;
                    QueryPerformanceCounter(&mut self.received_counter)?;
                }
                ensure!(
                    self.received_counter > 0 && count <= 64,
                    "installation console read context"
                );
                if count == 0 {
                    return Ok(ConsoleRead::Eof);
                }
                self.offset = 0;
                self.length = count as usize;
            }
            let value = self.pending[self.offset];
            self.pending[self.offset] = 0;
            self.offset += 1;
            if value == 10 {
                if invalid {
                    return Ok(ConsoleRead::Invalid);
                }
                if eof {
                    return Ok(ConsoleRead::Eof);
                }
                return match input
                    .line(std::str::from_utf8(&line[..length])?, self.received_counter)
                {
                    Ok(()) => Ok(ConsoleRead::Queued),
                    Err(super::input::InputError::InvalidLine) => Ok(ConsoleRead::Invalid),
                    Err(error) => Err(error.into()),
                };
            }
            if value == 13 {
                continue;
            }
            if value == 26 && length == 0 && !invalid {
                eof = true;
                continue;
            }
            if eof || value > 127 || length == 32 {
                invalid = true;
            } else {
                line[length] = value as u8;
                length += 1;
            }
        }
    }
}

pub struct ConsoleReader {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<Option<i64>>>>,
}
impl ConsoleReader {
    pub fn finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }
    /// Cancel synchronous I/O and join the real thread. Repeat cancellation
    /// because the first request can race entry into the native read.
    /// Returns the native EOF receipt counter if the reader reached EOF.
    pub fn shutdown(&mut self) -> Result<Option<i64>> {
        self.stop.store(true, Ordering::Release);
        let Some(thread) = self.thread.take() else {
            return Ok(None);
        };
        while !thread.is_finished() {
            unsafe {
                let _ = CancelSynchronousIo(HANDLE(thread.as_handle().as_raw_handle()));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        thread
            .join()
            .map_err(|_| anyhow::anyhow!("installation console reader panicked"))?
    }
}
impl Drop for ConsoleReader {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::super::input::CommandPool;
    use super::*;
    use anyhow::Context as _;
    use reel_assets::jobs::{
        BorrowedPseudoConsole, QuietEvent, QuietLimits, run_pseudoconsole_observed,
    };
    use std::os::windows::io::FromRawHandle;
    use std::{
        io::{Read, Write},
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicU32, Ordering},
        },
        thread::JoinHandle,
    };
    use windows::Win32::System::Pipes::CreatePipe;
    struct Pseudo(HPCON);
    impl Pseudo {
        fn borrowed(&self) -> BorrowedPseudoConsole<'_> {
            unsafe { BorrowedPseudoConsole::from_raw(self.0.0).unwrap() }
        }
    }
    impl Drop for Pseudo {
        fn drop(&mut self) {
            unsafe {
                ClosePseudoConsole(self.0);
            }
        }
    }
    struct Capture {
        console: Option<Pseudo>,
        input: Option<File>,
        reader: Option<JoinHandle<(Vec<u8>, bool)>>,
    }
    fn pipe() -> (File, File) {
        let (mut read, mut write) = (HANDLE::default(), HANDLE::default());
        unsafe {
            CreatePipe(&mut read, &mut write, None, 0).unwrap();
            (
                File::from_raw_handle(read.0),
                File::from_raw_handle(write.0),
            )
        }
    }
    impl Capture {
        fn new() -> Self {
            let (input, writer) = pipe();
            let (mut reader, output) = pipe();
            let console = unsafe {
                CreatePseudoConsole(COORD { X: 80, Y: 30 }, handle(&input), handle(&output), 0)
                    .unwrap()
            };
            drop(input);
            drop(output);
            let reader = std::thread::spawn(move || {
                let mut bytes = Vec::new();
                let mut overflow = false;
                let mut buffer = [0; 4096];
                loop {
                    let count = match reader.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => count,
                    };
                    if bytes.len() + count <= 256 * 1024 {
                        bytes.extend_from_slice(&buffer[..count]);
                    } else {
                        overflow = true;
                    } // Keep draining through actual close.
                }
                (bytes, overflow)
            });
            Self {
                console: Some(Pseudo(console)),
                input: Some(writer),
                reader: Some(reader),
            }
        }
        fn finish(&mut self) -> (Vec<u8>, bool) {
            drop(self.input.take());
            drop(self.console.take());
            self.reader.take().unwrap().join().unwrap()
        }
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            drop(self.input.take());
            drop(self.console.take());
            if let Some(reader) = self.reader.take() {
                let _ = reader.join();
            }
        }
    }
    #[test]
    #[ignore = "owned headless console child; explicitly launched by parent"]
    fn console_fixture() {
        let original_input = File::options()
            .read(true)
            .write(true)
            .open("CONIN$")
            .unwrap();
        let original_output = File::options()
            .read(true)
            .write(true)
            .open("CONOUT$")
            .unwrap();
        let (mut original_input_mode, mut original_output_mode) =
            (CONSOLE_MODE::default(), CONSOLE_MODE::default());
        unsafe {
            GetConsoleMode(handle(&original_input), &mut original_input_mode).unwrap();
            GetConsoleMode(handle(&original_output), &mut original_output_mode).unwrap();
            original_input_mode |= ENABLE_LINE_INPUT
                | ENABLE_ECHO_INPUT
                | ENABLE_PROCESSED_INPUT
                | ENABLE_VIRTUAL_TERMINAL_INPUT;
            original_output_mode &= !ENABLE_PROCESSED_OUTPUT;
            SetConsoleMode(handle(&original_input), original_input_mode).unwrap();
            SetConsoleMode(handle(&original_output), original_output_mode).unwrap();
            SetConsoleCP(1252).unwrap();
            SetConsoleOutputCP(1252).unwrap();
        }
        let original_pages = unsafe { (GetConsoleCP(), GetConsoleOutputCP()) };
        let mut surface = ConsoleSurface::open().unwrap();
        let text = (0..20)
            .map(|row| format!("Línea {row:02}: señal de ingeniería; año, acción, pingüino.\n"))
            .collect::<String>();
        let layout = surface.transcript(&text).unwrap();
        assert!(
            layout.columns >= 80
                && layout.rows >= 30
                && layout.input_code_page == 65001
                && layout.output_code_page == 65001
        );
        surface.status("Engineering console fixture").unwrap();
        let pool = CommandPool::new();
        let (mut input, mut commands) = pool.channel();
        std::fs::write(
            "console-ready.marker",
            b"actual console cells and cooked modes checked",
        )
        .unwrap();
        for index in 0..40 {
            assert_eq!(
                surface
                    .read(&mut input)
                    .with_context(|| format!("echo line {index}"))
                    .unwrap(),
                ConsoleRead::Queued
            );
            let command = commands.receive().unwrap();
            assert_eq!(command.command(), Command::Status);
            assert!(command.received_counter() > 0);
            drop(command);
            assert_eq!(pool.occupied(), 0);
            surface
                .status("Status after ordinary echoed line")
                .with_context(|| format!("after echo line {index}"))
                .unwrap();
        }
        for _ in 0..3 {
            assert_eq!(surface.read(&mut input).unwrap(), ConsoleRead::Invalid);
            assert_eq!(pool.occupied(), 0);
            assert!(commands.receive().is_none());
            surface.verify_transcript().unwrap();
        }
        assert_eq!(surface.read(&mut input).unwrap(), ConsoleRead::Eof);
        assert!(surface.received_counter() > 0);
        assert_eq!(pool.occupied(), 0);
        surface.verify_transcript().unwrap();
        let (input, mut commands) = pool.channel();
        let mut reader = surface.start_reader(input).unwrap();
        std::thread::sleep(Duration::from_millis(30));
        assert!(!reader.finished());
        surface
            .status("Renderer stays responsive during cooked input")
            .unwrap();
        assert!(reader.shutdown().unwrap().is_none());
        assert!(reader.finished() && commands.receive().is_none());
        std::fs::write(
            "console-resize-ready.marker",
            b"complete read and unchanged source cells",
        )
        .unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while !std::path::Path::new("console-resized.marker").is_file() {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(surface.layout().is_err());
        drop(surface);
        let (mut input_mode, mut output_mode) = (CONSOLE_MODE::default(), CONSOLE_MODE::default());
        unsafe {
            GetConsoleMode(handle(&original_input), &mut input_mode).unwrap();
            GetConsoleMode(handle(&original_output), &mut output_mode).unwrap();
        }
        assert_eq!(
            (input_mode.0, output_mode.0),
            (original_input_mode.0, original_output_mode.0)
        );
        assert_eq!(
            unsafe { (GetConsoleCP(), GetConsoleOutputCP()) },
            original_pages
        );
        std::fs::write(
            "console-complete.marker",
            b"forty echoed lines preserved exact original cells",
        )
        .unwrap();
    }
    #[test]
    fn owned_console_keeps_spanish_transcript_through_repeated_cooked_echo() {
        let root = tempfile::tempdir().unwrap();
        let path = std::env::current_exe().unwrap();
        let identity = crate::media_runtime::ByteIdentity::observe_file(&path).unwrap();
        let binding = crate::media_runtime::BoundTool::bind(&crate::scene_delivery::FileRef {
            path,
            sha256: identity.sha256,
            bytes: identity.bytes,
        })
        .unwrap();
        let mut capture = Capture::new();
        let console = capture.console.as_ref().unwrap().borrowed();
        let pid = AtomicU32::new(0);
        let cancel = Arc::new(AtomicBool::new(false));
        struct Cancel(Arc<AtomicBool>);
        impl Drop for Cancel {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        std::thread::scope(|scope| {
            let (program, directory, actual_pid) = (binding.path(), root.path(), &pid);
            let stopped = cancel.clone();
            let job = scope.spawn(move || {
                run_pseudoconsole_observed(
                    program,
                    &[
                        "--ignored".into(),
                        "--exact".into(),
                        "spoken_programme::installation::native::console::tests::console_fixture"
                            .into(),
                        "--test-threads=1".into(),
                    ],
                    directory,
                    Duration::from_secs(15),
                    QuietLimits {
                        memory_bytes: 256 * 1024 * 1024,
                        processes: 1,
                    },
                    console,
                    |event| {
                        if stopped.load(Ordering::Acquire) {
                            return Err("controlled console cancelled".into());
                        }
                        if let QuietEvent::Process(info) = event {
                            actual_pid.store(info.process_id, Ordering::Release);
                        }
                        Ok(())
                    },
                )
                .map_err(|error| error.to_string())
            });
            let _cancel = Cancel(cancel.clone());
            let until = Instant::now() + Duration::from_secs(10);
            while !root.path().join("console-ready.marker").is_file() {
                assert!(!job.is_finished(), "console child exited before admission");
                assert!(Instant::now() < until, "console admission deadline");
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(pid.load(Ordering::Acquire) > 0);
            capture
                .input
                .as_mut()
                .unwrap()
                .write_all(
                    format!(
                        "{}bogus\rpausé\r{}\r\x1a\r",
                        "status\r".repeat(40),
                        "s".repeat(65)
                    )
                    .as_bytes(),
                )
                .unwrap();
            while !root.path().join("console-resize-ready.marker").is_file() {
                assert!(
                    !job.is_finished(),
                    "console child exited before resize check"
                );
                assert!(Instant::now() < until, "console resize handshake deadline");
                std::thread::sleep(Duration::from_millis(2));
            }
            unsafe {
                ResizePseudoConsole(capture.console.as_ref().unwrap().0, COORD { X: 79, Y: 29 })
                    .unwrap();
            }
            std::fs::write(
                root.path().join("console-resized.marker"),
                b"actual pseudoconsole resized below admitted layout",
            )
            .unwrap();
            let result = job.join().unwrap();
            assert!(result.is_ok(), "{result:?}");
            assert!(root.path().join("console-complete.marker").is_file());
        });
        let (bytes, overflow) = capture.finish();
        assert!(!overflow);
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("señal") && text.contains("pingüino"));
    }
}
