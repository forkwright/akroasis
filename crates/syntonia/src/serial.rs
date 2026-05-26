//! Serial port abstraction for radio communication.
//!
//! Defines a [`SerialPort`] trait that abstracts over real hardware and test
//! mocks. The real implementation wraps `serialport::TTYPort`; test code uses
//! [`MockSerialPort`] with scripted byte sequences.

use std::io;
use std::time::Duration;

/// Abstraction over a serial port connection.
///
/// Implementors must be `Send` so the port can be owned by async tasks.
pub trait SerialPort: Send {
    /// Write the entire buffer to the port.
    ///
    /// # Errors
    /// Returns `io::Error` on write failure or timeout.
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()>;

    /// Read available bytes INTO `buf`, returning how many were read.
    ///
    /// # Errors
    /// Returns `io::Error` on read failure or timeout.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>; // kanon:ignore RUST/indexing-slicing -- trait method parameter &mut [u8], not indexing

    /// Read exactly `buf.len()` bytes, blocking until complete or timeout.
    ///
    /// # Errors
    /// Returns `io::Error` if not enough bytes arrive before timeout.
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()>; // kanon:ignore RUST/indexing-slicing -- trait method parameter &mut [u8], not indexing

    /// Set the read timeout for subsequent operations.
    ///
    /// # Errors
    /// Returns `io::Error` if the timeout cannot be SET.
    fn set_timeout(&mut self, duration: Duration) -> io::Result<()>;

    /// Flush the output buffer.
    ///
    /// # Errors
    /// Returns `io::Error` on flush failure.
    fn flush(&mut self) -> io::Result<()>;
}

/// Wrapper around a real `serialport::TTYPort` (or platform equivalent).
///
/// Only available with the `hardware-serial` feature (requires `libudev-dev`
/// on Linux).
#[cfg(feature = "hardware-serial")] // kanon:ignore RUST/feature-gate-check -- declared in syntonia/Cargo.toml [features]
pub struct HardwareSerialPort {
    inner: Box<dyn serialport::SerialPort>,
}

#[cfg(feature = "hardware-serial")] // kanon:ignore RUST/feature-gate-check -- declared in syntonia/Cargo.toml [features]
impl HardwareSerialPort {
    /// Open a serial port at the given path with the specified baud rate.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if the port cannot be opened at the requested baud
    /// rate (e.g. device missing, busy, or permission denied).
    pub fn open(path: &str, baud_rate: u32) -> io::Result<Self> {
        let port = serialport::new(path, baud_rate)
            .data_bits(serialport::DataBits::Eight)
            .parity(serialport::Parity::None)
            .stop_bits(serialport::StopBits::One)
            .flow_control(serialport::FlowControl::None)
            .timeout(Duration::from_millis(1500))
            .open()
            .map_err(io::Error::other)?;
        Ok(Self { inner: port })
    }
}

#[cfg(feature = "hardware-serial")] // kanon:ignore RUST/feature-gate-check -- declared in syntonia/Cargo.toml [features]
#[rustfmt::skip]
impl SerialPort for HardwareSerialPort { // kanon:ignore ARCHITECTURE/trait-impl-colocation -- SerialPort trait exists for testability; HardwareSerialPort is the production path
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> { io::Write::write_all(&mut self.inner, buf) }
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> { io::Read::read(&mut self.inner, buf) } // kanon:ignore RUST/indexing-slicing -- trait impl parameter &mut [u8]
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> { io::Read::read_exact(&mut self.inner, buf) } // kanon:ignore RUST/indexing-slicing -- trait impl parameter &mut [u8]

    fn set_timeout(&mut self, duration: Duration) -> io::Result<()> {
        self.inner.set_timeout(duration).map_err(io::Error::other)
    }

    fn flush(&mut self) -> io::Result<()> {
        io::Write::flush(&mut self.inner)
    }
}

#[cfg(test)]
pub mod mock {
    //! Mock serial port for testing without hardware.

    use std::collections::VecDeque;
    use std::io;
    use std::time::Duration;

    use super::SerialPort;

    /// A mock serial port that replays scripted responses.
    ///
    /// Test code pushes expected RX data via [`enqueue_response`]. Writes are
    /// captured in [`written`] for assertion.
    pub struct MockSerialPort {
        /// Bytes written by the protocol under test.
        pub written: Vec<u8>,
        /// Scripted response bytes the mock will return on reads.
        rx_queue: VecDeque<u8>,
        /// If SET, the next read will return this error.
        pending_error: Option<io::ErrorKind>,
        timeout: Duration,
    }

    impl Default for MockSerialPort {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MockSerialPort {
        /// Create an empty mock with a default 1.5 s timeout.
        pub fn new() -> Self {
            Self {
                written: Vec::new(),
                rx_queue: VecDeque::new(),
                pending_error: None,
                timeout: Duration::from_millis(1500),
            }
        }

        /// Queue response bytes that will be returned by subsequent reads.
        #[cfg_attr(
            not(feature = "hardware-serial"),
            expect(dead_code, reason = "used only by hardware-serial protocol tests")
        )]
        pub fn enqueue_response(&mut self, data: &[u8]) {
            self.rx_queue.extend(data);
        }

        /// Make the next read return an error of the given kind.
        #[cfg_attr(
            not(feature = "hardware-serial"),
            expect(dead_code, reason = "used only by hardware-serial protocol tests")
        )]
        pub fn inject_error(&mut self, kind: io::ErrorKind) {
            self.pending_error = Some(kind);
        }
    }

    impl SerialPort for MockSerialPort {
        fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
            self.written.extend_from_slice(buf);
            Ok(())
        }

        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if let Some(kind) = self.pending_error.take() {
                return Err(io::Error::new(kind, "mock injected error"));
            }
            if self.rx_queue.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "mock: no data available",
                ));
            }
            let n = buf.len().min(self.rx_queue.len());
            for byte in buf.iter_mut().take(n) {
                #[expect(
                    clippy::unwrap_used,
                    reason = "mock test scaffold; we just checked rx_queue.len() >= n immediately above"
                )]
                {
                    *byte = self.rx_queue.pop_front().unwrap();
                }
            }
            Ok(n)
        }

        fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
            if let Some(kind) = self.pending_error.take() {
                return Err(io::Error::new(kind, "mock injected error"));
            }
            if self.rx_queue.len() < buf.len() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "mock: insufficient data for read_exact",
                ));
            }
            for byte in buf.iter_mut() {
                #[expect(
                    clippy::unwrap_used,
                    reason = "mock test scaffold; we just checked rx_queue.len() >= buf.len() immediately above"
                )]
                {
                    *byte = self.rx_queue.pop_front().unwrap();
                }
            }
            Ok(())
        }

        fn set_timeout(&mut self, duration: Duration) -> io::Result<()> {
            self.timeout = duration;
            Ok(())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}
