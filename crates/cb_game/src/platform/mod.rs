//! Platform-specific behaviour (ADR-0037). This module is the only place with
//! `cfg(target_os)`; everything else is a pure function of an injected [`Os`] so it is tested
//! on every OS.

pub mod modifiers;
pub mod open;
pub mod paths;

/// A supported operating system.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Os {
    /// Windows 10/11.
    Windows,
    /// Linux (X11 or Wayland).
    Linux,
    /// macOS on Apple Silicon.
    MacOs,
}

impl Os {
    /// The OS this binary was built for (other Unix-likes are treated as Linux).
    #[must_use]
    pub const fn current() -> Self {
        #[cfg(target_os = "windows")]
        {
            Os::Windows
        }
        #[cfg(target_os = "macos")]
        {
            Os::MacOs
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            Os::Linux
        }
    }

    /// Short name for logs and crash reports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Os::Windows => "windows",
            Os::Linux => "linux",
            Os::MacOs => "macos",
        }
    }
}
