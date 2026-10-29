//! Bound explicit consumer stdin without detached blocking reads or credential values.
use anyhow::{bail, Result};
use std::time::Duration;

const INPUT_LIMIT: usize = 64 * 1024;
const INPUT_DEADLINE: Duration = Duration::from_secs(5);

pub(crate) fn read_stdin() -> Result<String> {
    use std::io::IsTerminal;
    if std::io::stdin().is_terminal() {
        bail!("consumer stdin cannot be read interactively; pipe or redirect the input, or use --input FILE");
    }
    #[cfg(unix)]
    {
        read_input_fd(libc::STDIN_FILENO, INPUT_LIMIT, INPUT_DEADLINE)
    }
    #[cfg(not(unix))]
    {
        bail!("consumer stdin capture is unavailable on this platform; use --input FILE");
    }
}

pub(crate) fn read_file(path: &std::path::Path) -> Result<String> {
    use anyhow::Context;
    if !path.is_file() {
        bail!("consumer --input must name a regular file or - for stdin");
    }
    let file = std::fs::File::open(path).context("consumer input file could not be opened")?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        read_input_fd(file.as_raw_fd(), INPUT_LIMIT, INPUT_DEADLINE)
    }
    #[cfg(not(unix))]
    {
        use std::io::Read;
        let mut bytes = Vec::new();
        file.take((INPUT_LIMIT + 1) as u64)
            .read_to_end(&mut bytes)
            .context("consumer input file could not be read")?;
        if bytes.len() > INPUT_LIMIT {
            bail!("consumer input exceeds 64 KiB");
        }
        String::from_utf8(bytes).context("consumer input must be UTF-8")
    }
}

#[cfg(unix)]
fn read_input_fd(fd: std::os::fd::RawFd, limit: usize, timeout: Duration) -> Result<String> {
    use anyhow::Context;
    use std::{
        io::Read,
        os::fd::{AsRawFd, FromRawFd},
        time::Instant,
    };

    // Duplicating stdin gives the guard ownership of a descriptor. Status flags
    // are shared with stdin, so restore them before the descriptor is closed.
    struct Input {
        file: std::fs::File,
        original_flags: libc::c_int,
    }
    impl Drop for Input {
        fn drop(&mut self) {
            unsafe {
                libc::fcntl(self.file.as_raw_fd(), libc::F_SETFL, self.original_flags);
            }
        }
    }
    let copied = unsafe { libc::dup(fd) };
    if copied < 0 {
        bail!("could not open consumer input");
    }
    let file = unsafe { std::fs::File::from_raw_fd(copied) };
    let original_flags = unsafe { libc::fcntl(copied, libc::F_GETFL) };
    if original_flags < 0 {
        bail!("could not inspect consumer input");
    }
    let mut input = Input {
        file,
        original_flags,
    };
    if unsafe { libc::fcntl(copied, libc::F_SETFL, original_flags | libc::O_NONBLOCK) } < 0 {
        bail!("could not configure bounded consumer input");
    }
    let deadline = Instant::now() + timeout;
    let mut bytes = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            bail!("consumer input timed out; close the pipe after sending the code");
        }
        let mut ready = libc::pollfd {
            fd: copied,
            events: libc::POLLIN,
            revents: 0,
        };
        let wait_ms = remaining.as_millis().clamp(1, i32::MAX as u128) as i32;
        let polled = unsafe { libc::poll(&mut ready, 1, wait_ms) };
        if polled < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            bail!("could not wait for consumer input");
        }
        if polled == 0 {
            continue;
        }
        let mut buffer = [0_u8; 4096];
        let capacity = (limit + 1 - bytes.len()).min(buffer.len());
        match input.file.read(&mut buffer[..capacity]) {
            Ok(0) => break,
            Ok(count) => {
                bytes.extend_from_slice(&buffer[..count]);
                if bytes.len() > limit {
                    bail!("consumer input exceeds 64 KiB");
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                continue
            }
            Err(_) => bail!("consumer input could not be read"),
        }
    }
    String::from_utf8(bytes).context("consumer input must be UTF-8")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        io::Write,
        os::{fd::AsRawFd, unix::net::UnixStream},
    };

    fn read_fixture(bytes: &[u8], limit: usize) -> Result<String> {
        let (input, mut writer) = UnixStream::pair().unwrap();
        writer.write_all(bytes).unwrap();
        drop(writer);
        read_input_fd(input.as_raw_fd(), limit, Duration::from_secs(1))
    }

    #[test]
    fn captures_source_until_eof_without_echoing_errors() {
        assert_eq!(
            read_fixture(b"print('synthetic')\n", 64).unwrap(),
            "print('synthetic')\n"
        );
        assert_eq!(read_fixture(b"", 64).unwrap(), "");
        assert!(read_fixture(&[0xff], 64)
            .unwrap_err()
            .to_string()
            .contains("UTF-8"));
        let error = read_fixture(b"SYNTHETIC_PRIVATE_SOURCE_MUST_NOT_BE_ECHOED", 8).unwrap_err();
        assert!(error.to_string().contains("64 KiB"));
        assert!(!format!("{error:#}").contains("SYNTHETIC_PRIVATE"));
    }

    #[test]
    fn open_pipe_times_out_and_restores_descriptor_flags() {
        let (input, _writer) = UnixStream::pair().unwrap();
        let fd = input.as_raw_fd();
        let before = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        let error = read_input_fd(fd, 64, Duration::from_millis(25)).unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFL) }, before);
    }
}
