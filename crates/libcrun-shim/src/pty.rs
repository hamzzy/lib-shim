//! PTY (Pseudo-Terminal) Support
//!
//! This module provides PTY handling for interactive container exec sessions.

use crate::error::{Result, ShimError};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};

/// PTY master/slave pair
pub struct Pty {
    master: std::fs::File,
    slave: std::fs::File,
    original_termios: Option<libc::termios>,
}

impl Pty {
    /// Create a new PTY pair
    #[cfg(unix)]
    pub fn new() -> Result<Self> {
        use std::ptr;

        let mut master_fd: libc::c_int = 0;
        let mut slave_fd: libc::c_int = 0;

        let ret = unsafe {
            libc::openpty(
                &mut master_fd,
                &mut slave_fd,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };

        if ret != 0 {
            return Err(ShimError::runtime("Failed to open PTY"));
        }

        let master = unsafe { std::fs::File::from_raw_fd(master_fd) };
        let slave = unsafe { std::fs::File::from_raw_fd(slave_fd) };

        Ok(Self {
            master,
            slave,
            original_termios: None,
        })
    }

    #[cfg(not(unix))]
    pub fn new() -> Result<Self> {
        Err(ShimError::runtime("PTY not supported on this platform"))
    }

    /// Get the master file descriptor
    pub fn master_fd(&self) -> RawFd {
        self.master.as_raw_fd()
    }

    /// Get the slave file descriptor
    pub fn slave_fd(&self) -> RawFd {
        self.slave.as_raw_fd()
    }

    /// Get mutable reference to master
    pub fn master(&mut self) -> &mut std::fs::File {
        &mut self.master
    }

    /// Get mutable reference to slave
    pub fn slave(&mut self) -> &mut std::fs::File {
        &mut self.slave
    }

    /// Set terminal to raw mode for interactive use
    #[cfg(unix)]
    pub fn set_raw_mode(&mut self) -> Result<()> {
        let stdin_fd = std::io::stdin().as_raw_fd();

        // Save original termios
        let mut original: libc::termios = unsafe { std::mem::zeroed() };
        let ret = unsafe { libc::tcgetattr(stdin_fd, &mut original) };
        if ret != 0 {
            return Err(ShimError::runtime("Failed to get terminal attributes"));
        }

        self.original_termios = Some(original);

        // Set raw mode
        let mut raw = original;
        raw.c_lflag &= !(libc::ECHO | libc::ICANON | libc::ISIG | libc::IEXTEN);
        raw.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
        raw.c_oflag &= !libc::OPOST;
        raw.c_cflag |= libc::CS8;
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;

        let ret = unsafe { libc::tcsetattr(stdin_fd, libc::TCSANOW, &raw) };
        if ret != 0 {
            return Err(ShimError::runtime("Failed to set raw mode"));
        }

        Ok(())
    }

    #[cfg(not(unix))]
    pub fn set_raw_mode(&mut self) -> Result<()> {
        Err(ShimError::runtime("Raw mode not supported on this platform"))
    }

    /// Restore terminal to original mode
    #[cfg(unix)]
    pub fn restore_mode(&mut self) -> Result<()> {
        if let Some(ref original) = self.original_termios {
            let stdin_fd = std::io::stdin().as_raw_fd();
            let ret = unsafe { libc::tcsetattr(stdin_fd, libc::TCSANOW, original) };
            if ret != 0 {
                return Err(ShimError::runtime("Failed to restore terminal"));
            }
        }
        Ok(())
    }

    #[cfg(not(unix))]
    pub fn restore_mode(&mut self) -> Result<()> {
        Ok(())
    }

    /// Resize the PTY window
    #[cfg(unix)]
    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        let ws = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        let ret = unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &ws) };

        if ret == -1 {
            return Err(ShimError::runtime("Failed to resize PTY"));
        }

        Ok(())
    }

    #[cfg(not(unix))]
    pub fn resize(&self, _rows: u16, _cols: u16) -> Result<()> {
        Ok(())
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        let _ = self.restore_mode();
    }
}

/// Interactive session for container exec
pub struct InteractiveSession {
    pty: Pty,
}

impl InteractiveSession {
    /// Create a new interactive session
    pub fn new() -> Result<Self> {
        let pty = Pty::new()?;
        Ok(Self { pty })
    }

    /// Get the PTY
    pub fn pty(&self) -> &Pty {
        &self.pty
    }

    /// Get mutable PTY
    pub fn pty_mut(&mut self) -> &mut Pty {
        &mut self.pty
    }

    /// Get the slave FD for the container process
    pub fn slave_fd(&self) -> RawFd {
        self.pty.slave_fd()
    }

    /// Set raw mode
    pub fn set_raw_mode(&mut self) -> Result<()> {
        self.pty.set_raw_mode()
    }

    /// Restore terminal mode
    pub fn restore_mode(&mut self) -> Result<()> {
        self.pty.restore_mode()
    }
}

/// Get current terminal size
#[cfg(unix)]
pub fn get_terminal_size() -> Option<(u16, u16)> {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let ret = unsafe { libc::ioctl(std::io::stdout().as_raw_fd(), libc::TIOCGWINSZ, &mut ws) };

    if ret == 0 {
        Some((ws.ws_row, ws.ws_col))
    } else {
        None
    }
}

#[cfg(not(unix))]
pub fn get_terminal_size() -> Option<(u16, u16)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn test_pty_creation() {
        let pty = Pty::new();
        assert!(pty.is_ok());
    }

    #[test]
    fn test_terminal_size() {
        // This might not work in all test environments
        let _size = get_terminal_size();
    }
}
