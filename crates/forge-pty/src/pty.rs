use forge_core::config_registry::ShellConfig;
use forge_core::geometry::Size;
use forge_core::{ForgeError, Result};
use nix::fcntl::{fcntl, FcntlArg, OFlag};
use nix::pty::{openpty, Winsize};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::{close, dup2, execvpe, fork, setsid, ForkResult};
use std::ffi::CString;
use std::os::unix::io::{AsRawFd, OwnedFd};

fn ensure_integration_scripts() -> std::io::Result<std::path::PathBuf> {
    let mut dir =
        std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()));
    dir.push(".local");
    dir.push("share");
    dir.push("forge");
    dir.push("shell-integration");

    std::fs::create_dir_all(&dir)?;

    let bash_script = include_str!("integration/bash.sh");
    let zsh_script = include_str!("integration/zsh.sh");
    let fish_script = include_str!("integration/fish.fish");
    let nu_script = include_str!("integration/nu.nu");

    std::fs::write(dir.join("bash.sh"), bash_script)?;
    std::fs::write(dir.join("zsh.sh"), zsh_script)?;
    std::fs::write(dir.join("fish.fish"), fish_script)?;
    std::fs::write(dir.join("nu.nu"), nu_script)?;

    Ok(dir)
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtyReadResult {
    Data(usize),
    WouldBlock,
    Eof,
}

impl Pty {
    pub fn spawn(shell: &ShellConfig, winsize: Winsize) -> Result<Self> {
        let program_cstr = CString::new(shell.program.clone())
            .map_err(|e| ForgeError::Pty(format!("Invalid program string: {}", e)))?;

        let mut args = Vec::new();

        args.push(program_cstr.clone());

        let mut base_args = Vec::new();
        for arg in &shell.args {
            base_args.push(
                CString::new(arg.clone())
                    .map_err(|e| ForgeError::Pty(format!("Invalid arg: {}", e)))?,
            );
        }

        let mut env_map = std::collections::HashMap::new();

        if let Ok(dir) = ensure_integration_scripts() {
            let prog = shell.program.as_str();
            if prog.ends_with("bash") {
                let init_path = dir.join("bash-init.sh");
                let init_script = format!(
                    "if [ -f ~/.bashrc ]; then source ~/.bashrc; fi\nsource {}",
                    dir.join("bash.sh").display()
                );
                std::fs::write(&init_path, init_script).ok();

                args.push(CString::new("--rcfile").unwrap());
                args.push(CString::new(init_path.to_string_lossy().to_string()).unwrap());
            } else if prog.ends_with("zsh") {
                let init_path = dir.join(".zshrc");
                let init_script = format!(
                    "ZDOTDIR=\"${{OLD_ZDOTDIR:-$HOME}}\"\nif [ -f \"$ZDOTDIR/.zshrc\" ]; then\n    source \"$ZDOTDIR/.zshrc\"\nfi\nsource \"{}\"\n",
                    dir.join("zsh.sh").display()
                );
                std::fs::write(&init_path, init_script).ok();
                env_map.insert(
                    "OLD_ZDOTDIR".to_string(),
                    std::env::var("ZDOTDIR").unwrap_or_else(|_| "".to_string()),
                );
                env_map.insert("ZDOTDIR".to_string(), dir.to_string_lossy().to_string());
            } else if prog.ends_with("fish") {
                args.push(CString::new("--init-command").unwrap());
                args.push(
                    CString::new(format!("source {}", dir.join("fish.fish").display())).unwrap(),
                );
            } else if prog.ends_with("nu") {
                args.push(CString::new("-e").unwrap());
                args.push(CString::new(format!("source {}", dir.join("nu.nu").display())).unwrap());
            }
        }

        args.extend(base_args);

        for (k, v) in std::env::vars() {
            env_map.insert(k, v);
        }
        env_map.insert("TERM".to_string(), "xterm-256color".to_string());
        env_map.insert("COLORTERM".to_string(), "truecolor".to_string());
        env_map.insert("LANG".to_string(), "en_US.UTF-8".to_string());
        for (k, v) in &shell.parsed_env {
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

    pub fn read_nonblocking(&self, buf: &mut [u8]) -> Result<PtyReadResult> {
        match nix::unistd::read(self.master_fd.as_raw_fd(), buf) {
            Ok(0) => Ok(PtyReadResult::Eof),
            Ok(n) => Ok(PtyReadResult::Data(n)),
            Err(nix::errno::Errno::EAGAIN) => Ok(PtyReadResult::WouldBlock),
            Err(nix::errno::Errno::EIO) => Ok(PtyReadResult::Eof),
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
                    let pfd = nix::poll::PollFd::new(std::os::fd::AsFd::as_fd(&self.master_fd), nix::poll::PollFlags::POLLOUT);
                    let _ = nix::poll::poll(&mut [pfd], 1000_u16);
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
