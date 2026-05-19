use std::{fmt::Display, ops::Add};

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// A DMX address, indexed from 1.
///
/// We don't check that the value is valid at parse time, as this makes
/// deserializing into an untagged ParseDmxAddr fail with an obscure message.
/// This needs to be validated downstream.
#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Copy, Clone, Debug)]
pub struct DmxAddr(usize);

impl DmxAddr {
    /// Create a new DMX address from a 1-indexed value.
    pub fn new(addr: usize) -> Self {
        Self(addr)
    }

    /// Get the DMX buffer index of this address (indexed from 0).
    pub fn dmx_index(&self) -> usize {
        self.0 - 1
    }

    /// Ensure this address is in range.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=512).contains(&self.0),
            "invalid DMX address {}",
            self.0
        );
        Ok(())
    }
}

impl Display for DmxAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Add<usize> for DmxAddr {
    type Output = DmxAddr;
    fn add(self, rhs: usize) -> Self::Output {
        Self(self.0 + rhs)
    }
}

/// A data buffer for one DMX universe.
pub type DmxBuffer = [u8; 512];

/// A DMX universe: a port paired with its output buffer.
pub struct DmxUniverse {
    pub port: Box<dyn rust_dmx::DmxPort>,
    pub buffer: DmxBuffer,
}

impl DmxUniverse {
    /// Create a new universe with an offline port and zeroed buffer.
    pub fn offline() -> Self {
        Self {
            port: Box::new(rust_dmx::OfflineDmxPort),
            buffer: [0u8; 512],
        }
    }
}

/// Index into the DMX universes.
pub type UniverseIdx = usize;

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn dmx_addr_new() {
        let addr = DmxAddr::new(1);
        assert_eq!(addr.dmx_index(), 0);

        let addr = DmxAddr::new(512);
        assert_eq!(addr.dmx_index(), 511);
        addr.validate().unwrap();
    }

    #[test]
    fn dmx_addr_validate_boundaries() {
        DmxAddr::new(1).validate().unwrap();
        DmxAddr::new(512).validate().unwrap();
        DmxAddr::new(0).validate().unwrap_err();
        DmxAddr::new(513).validate().unwrap_err();
    }

    #[test]
    fn dmx_addr_add() {
        let addr = DmxAddr::new(10) + 3;
        assert_eq!(addr.dmx_index(), 12); // 13 - 1
    }
}

#[cfg(test)]
pub(crate) mod mock {
    use rust_dmx::{DmxPort, OpenError, SetFpsError, WriteError};
    use serde::{Deserialize, Serialize};
    use std::fmt;

    #[derive(Serialize, Deserialize)]
    pub struct MockDmxPort {
        pub open_should_fail: bool,
        pub opened: bool,
        /// If `Some`, the mock supports framerate control and reports this
        /// value from `get_framerate()`; `set_framerate` updates it. If `None`,
        /// the port behaves like a non-framerate-capable port (the trait
        /// defaults: `get_framerate` returns None, `set_framerate` errors).
        pub framerate: Option<u8>,
    }

    impl MockDmxPort {
        pub fn new() -> Self {
            Self {
                open_should_fail: false,
                opened: false,
                framerate: None,
            }
        }

        pub fn failing() -> Self {
            Self {
                open_should_fail: true,
                opened: false,
                framerate: None,
            }
        }

        pub fn with_framerate(fps: u8) -> Self {
            Self {
                open_should_fail: false,
                opened: false,
                framerate: Some(fps),
            }
        }
    }

    #[typetag::serde]
    impl DmxPort for MockDmxPort {
        fn open(&mut self) -> Result<(), OpenError> {
            if self.open_should_fail {
                Err(OpenError::NotConnected)
            } else {
                self.opened = true;
                Ok(())
            }
        }

        fn close(&mut self) {
            self.opened = false;
        }

        fn get_framerate(&self) -> Option<u8> {
            self.framerate
        }

        fn set_framerate(&mut self, fps: u8) -> Result<(), SetFpsError> {
            if self.framerate.is_none() {
                return Err(SetFpsError::Unsupported);
            }
            self.framerate = Some(fps);
            Ok(())
        }

        fn write(&mut self, _frame: &[u8]) -> Result<(), WriteError> {
            if !self.opened {
                return Err(WriteError::Disconnected);
            }
            Ok(())
        }
    }

    impl fmt::Display for MockDmxPort {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "mock")
        }
    }
}
