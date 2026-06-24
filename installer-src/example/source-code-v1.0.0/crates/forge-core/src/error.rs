use thiserror::Error;

#[derive(Error, Debug)]
pub enum ForgeError {
    #[error("Wayland error: {0}")]
    Wayland(String),
    #[error("Vulkan error: {0}")]
    Vulkan(String),
    #[error("PTY error: {0}")]
    Pty(String),
    #[error("Config error: {0}")]
    Config(String),
    #[error("Plugin error: {0}")]
    Plugin(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Other error: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, ForgeError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        assert_eq!(ForgeError::Wayland("test".into()).to_string(), "Wayland error: test");
        assert_eq!(ForgeError::Vulkan("test".into()).to_string(), "Vulkan error: test");
        assert_eq!(ForgeError::Pty("test".into()).to_string(), "PTY error: test");
        assert_eq!(ForgeError::Config("test".into()).to_string(), "Config error: test");
        assert_eq!(ForgeError::Plugin("test".into()).to_string(), "Plugin error: test");
        assert_eq!(ForgeError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "test")).to_string(), "IO error: test");
        assert_eq!(ForgeError::Other("test".into()).to_string(), "Other error: test");
    }
}
