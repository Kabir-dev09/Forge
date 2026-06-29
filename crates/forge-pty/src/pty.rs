use forge_core::config_registry::ShellConfig;
use forge_core::geometry::Size;
use forge_core::{ForgeError, Result};
use nix::fcntl::{fcntl, FcntlArg, OFlag};
use nix::pty::{openpty, Winsize};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::{close, dup2, execvpe, fork, setsid, ForkResult};
use std::ffi::CString;
use std::os::unix::io::{AsRawFd, OwnedFd};

pub fn size_to_winsize(size: Size, cell_w: u16, cell_h: u16) -> Winsize {
    Winsize {
        ws_col: (size.width as u16 / cell_w).max(1),
        ws_row: (size.height as u16 / cell_h).max(1),
        ws_xpixel: size.width as u16,
        ws_ypixel: size.height as u16,
    }
}

pub struct Pty {
    pub master_fd: OwnedFd,
    pub child_pid: nix::unistd::Pid,
    pub size: Winsize,
}

impl Pty {
    pub fn spawn(shell: &ShellConfig, winsize: Winsize) -> Result<Self> {
        let program_cstr = CString::new(shell.program.clone())
            .map_err(|e| ForgeError::Pty(format!("Invalid program string: {}", e)))?;

        let mut args = Vec::new();

        args.push(program_cstr.clone());

        for arg in &shell.args {
            args.push(
                CString::new(arg.clone())
                    .map_err(|e| ForgeError::Pty(format!("Invalid arg: {}", e)))?,
            );
        }

        let mut env_map = std::collections::HashMap::new();
        for (k, v) in std::env::vars() {
            env_map.insert(k, v);
        }
        env_map.insert("TERM".to_string(), "xterm-256color".to_string());
        env_map.insert("COLORTERM".to_string(), "truecolor".to_string());
        env_map.insert("LANG".to_string(), "en_US.UTF-8".to_string());
        for (k, v) in &shell.env {
            env_map.insert(k.clone(), v.clone());
        }

        let mut envs = Vec::new();
        for (k, v) in env_map {
            let entry = format!("{}={}", k, v);
            envs.push(
                CString::new(entry).map_err(|e| ForgeError::Pty(format!("Invalid env: {}", e)))?,
            );
        }

        let pty_res =
            openpty(None, None).map_err(|e| ForgeError::Pty(format!("openpty failed: {}", e)))?;

        unsafe {
            nix::libc::ioctl(
                pty_res.master.as_raw_fd(),
                nix::libc::TIOCSWINSZ,
                &winsize as *const _,
            );
        }

        match unsafe { fork() } {
            Ok(ForkResult::Parent { child, .. }) => {
                drop(pty_res.slave);

                let flags = fcntl(pty_res.master.as_raw_fd(), FcntlArg::F_GETFL)
                    .map_err(|e| ForgeError::Pty(format!("fcntl GETFL failed: {}", e)))?;
                let mut oflags = OFlag::from_bits_truncate(flags);
                oflags.insert(OFlag::O_NONBLOCK);
                fcntl(pty_res.master.as_raw_fd(), FcntlArg::F_SETFL(oflags))
                    .map_err(|e| ForgeError::Pty(format!("fcntl SETFL failed: {}", e)))?;

                Ok(Pty {
                    master_fd: pty_res.master,
                    child_pid: child,
                    size: winsize,
                })
            }
            Ok(ForkResult::Child) => {
                drop(pty_res.master);

                let slave_fd = pty_res.slave.as_raw_fd();

                if setsid().is_err() {
                    unsafe {
                        nix::libc::_exit(1);
                    }
                }

                // Acquire the controlling terminal. Without this, job control fails.
                // bash is resilient to this, but zsh and fish will immediately crash or exit.
                unsafe {
                    nix::libc::ioctl(slave_fd, nix::libc::TIOCSCTTY, 0);
                }

                if dup2(slave_fd, 0).is_err() {
                    unsafe {
                        nix::libc::_exit(1);
                    }
                }
                if dup2(slave_fd, 1).is_err() {
                    unsafe {
                        nix::libc::_exit(1);
                    }
                }
                if dup2(slave_fd, 2).is_err() {
                    unsafe {
                        nix::libc::_exit(1);
                    }
                }

                if slave_fd > 2 {
                    let _ = close(slave_fd);
                }

                let _ = execvpe(&program_cstr, &args, &envs);
                unsafe {
                    nix::libc::_exit(1);
                }
            }
            Err(e) => Err(ForgeError::Pty(format!("fork failed: {}", e))),
        }
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize> {
        match nix::unistd::read(self.master_fd.as_raw_fd(), buf) {
            Ok(n) => Ok(n),
            Err(nix::errno::Errno::EAGAIN) => Ok(0),
            Err(nix::errno::Errno::EIO) => Err(ForgeError::Pty("Shell exited".to_string())),
            Err(e) => Err(ForgeError::Pty(e.to_string())),
        }
    }

    pub fn write_all(&self, data: &[u8]) -> Result<()> {
        let mut written = 0;
        while written < data.len() {
            match nix::unistd::write(&self.master_fd, &data[written..]) {
                Ok(n) if n > 0 => written += n,
                Ok(_) => return Err(ForgeError::Pty("Write returned 0".to_string())),
                Err(nix::errno::Errno::EAGAIN) => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(nix::errno::Errno::EIO) => {
                    return Err(ForgeError::Pty("Shell exited".to_string()))
                }
                Err(e) => return Err(ForgeError::Pty(e.to_string())),
            }
        }
        Ok(())
    }

    pub fn resize(&mut self, cols: u16, rows: u16, xpixel: u16, ypixel: u16) -> Result<()> {
        let new_size = Winsize {
            ws_col: cols,
            ws_row: rows,
            ws_xpixel: xpixel,
            ws_ypixel: ypixel,
        };
        unsafe {
            nix::libc::ioctl(
                self.master_fd.as_raw_fd(),
                nix::libc::TIOCSWINSZ,
                &new_size as *const Winsize,
            );
        }

        let _ = nix::sys::signal::kill(self.child_pid, nix::sys::signal::Signal::SIGWINCH);

        self.size = new_size;
        Ok(())
    }

    pub fn try_wait(&self) -> Option<i32> {
        match waitpid(self.child_pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(_, code)) => Some(code),
            Ok(WaitStatus::Signaled(_, _, _)) => Some(-1),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_core::config_registry::ShellConfig;

    #[test]
    fn spawn_echo_and_read() {
        let mut shell = ShellConfig::default();
        shell.program = "/bin/sh".to_string();
        shell.args = vec!["-c".to_string(), "echo hello; exit 0".to_string()];
        let winsize = Winsize {
            ws_col: 80,
            ws_row: 24,
            ws_xpixel: 800,
            ws_ypixel: 480,
        };
        let pty = Pty::spawn(&shell, winsize).expect("PTY spawn failed");

        let mut buf = vec![0u8; 1024];
        let mut total = String::new();
        for _ in 0..100 {
            match pty.read(&mut buf) {
                Ok(0) => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Ok(n) => {
                    total.push_str(&String::from_utf8_lossy(&buf[..n]));
                }
                Err(_) => break,
            }
        }
        assert!(
            total.contains("hello"),
            "Expected 'hello' in output, got: {:?}",
            total
        );
    }
}
