//! CoreBluetooth BLE GATT transport for Logitech devices on macOS.
//!
//! Logitech Bolt-generation devices (MX Master 4, MX Master 3S, MX Mechanical, MX Keys S,
//! etc.) and Bluetooth devices whose IOHIDDevice handle is blocked by macOS Input
//! Monitoring TCC expose HID++ 2.0 on their vendor BLE GATT service
//! (`00010000-0000-1000-8000-011f2000046d`).

use crate::hidpp::{DIRECT_INDEX, LONG_LEN, REPORT_LONG};
use anyhow::{anyhow, bail, Result};
use std::ffi::{c_char, c_void, CStr, CString};
use std::time::{Duration, Instant};

extern "C" {
    fn fs_ble_connected_keys() -> *mut c_char;
    fn fs_ble_free_string(s: *mut c_char);
    fn fs_ble_open(uuid: *const c_char, name_out: *mut c_char, name_cap: usize, product_id_out: *mut u16) -> *mut c_void;
    fn fs_ble_write(handle: *mut c_void, data: *const u8, len: usize) -> bool;
    fn fs_ble_read(handle: *mut c_void, out_buf: *mut u8, cap: usize, timeout_ms: i32) -> i32;
    fn fs_ble_close(handle: *mut c_void);
}

/// How a HID++ long report is framed in the BLE GATT characteristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Framing {
    /// `[index, feature, fn|swid, params...]` (19 bytes)
    WithIndex,
    /// `[feature, fn|swid, params...]` (18 bytes)
    FeatureFirst,
    /// `[0x11, index, feature, fn|swid, params...]` (20 bytes)
    WithReportId,
}

impl Framing {
    fn drop_bytes(self) -> usize {
        match self {
            Framing::WithReportId => 0,
            Framing::WithIndex => 1,
            Framing::FeatureFirst => 2,
        }
    }
}

pub struct BleDevice {
    handle: *mut c_void,
    pub product: String,
    pub product_id: u16,
    pub key: String,
    framing: Option<Framing>,
}

unsafe impl Send for BleDevice {}

impl Drop for BleDevice {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { fs_ble_close(self.handle) };
            self.handle = std::ptr::null_mut();
        }
    }
}

impl BleDevice {
    pub fn open(key: &str) -> Option<Self> {
        let c_key = CString::new(key).ok()?;
        let mut name_buf = vec![0 as c_char; 256];
        let mut product_id: u16 = 0;
        let handle = unsafe { fs_ble_open(c_key.as_ptr(), name_buf.as_mut_ptr(), name_buf.len(), &mut product_id) };
        if handle.is_null() {
            return None;
        }
        let product = unsafe { CStr::from_ptr(name_buf.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let mut dev = BleDevice {
            handle,
            product,
            product_id,
            key: key.to_string(),
            framing: None,
        };
        if dev.detect_framing().is_none() {
            log::debug!("{}: did not answer HID++ ping over BLE GATT", dev.product);
            return None;
        }
        Some(dev)
    }

    fn frame(report: &[u8], f: Framing) -> Vec<u8> {
        let mut long = [0u8; LONG_LEN];
        let copy_len = report.len().min(LONG_LEN);
        long[..copy_len].copy_from_slice(&report[..copy_len]);
        long[0] = REPORT_LONG;
        long[f.drop_bytes()..LONG_LEN].to_vec()
    }

    fn unframe(raw: &[u8], f: Framing) -> Vec<u8> {
        let mut out = match f {
            Framing::WithReportId => raw.to_vec(),
            Framing::WithIndex => {
                let mut v = Vec::with_capacity(1 + raw.len());
                v.push(REPORT_LONG);
                v.extend_from_slice(raw);
                v
            }
            Framing::FeatureFirst => {
                let mut v = Vec::with_capacity(2 + raw.len());
                v.push(REPORT_LONG);
                v.push(DIRECT_INDEX);
                v.extend_from_slice(raw);
                v
            }
        };
        if out.len() >= 2 && matches!(out[1], 0x00 | 0x01) {
            out[1] = DIRECT_INDEX;
        }
        if !out.is_empty() && out[0] == REPORT_LONG && out.len() < LONG_LEN {
            out.resize(LONG_LEN, 0);
        }
        out
    }

    fn send_raw(&self, payload: &[u8]) -> Result<()> {
        let ok = unsafe { fs_ble_write(self.handle, payload.as_ptr(), payload.len()) };
        if !ok {
            bail!("{}: BLE GATT write failed", self.product);
        }
        Ok(())
    }

    fn read_raw(&self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        let mut buf = [0u8; 64];
        let ms = timeout.as_millis().max(1) as i32;
        let n = unsafe { fs_ble_read(self.handle, buf.as_mut_ptr(), buf.len(), ms) };
        if n < 0 {
            bail!("{} disconnected", self.product);
        }
        if n == 0 {
            return Ok(None);
        }
        Ok(Some(buf[..n as usize].to_vec()))
    }

    fn detect_framing(&mut self) -> Option<Framing> {
        for f in [Framing::WithIndex, Framing::FeatureFirst, Framing::WithReportId] {
            // Drain any stale notifications first.
            while let Ok(Some(_)) = self.read_raw(Duration::from_millis(5)) {}
            let mut ping = [0u8; LONG_LEN];
            ping[0] = REPORT_LONG;
            ping[1] = DIRECT_INDEX;
            ping[2] = 0x00; // Root feature
            ping[3] = 0x1F; // getProtocolVersion, swid 0xF
            ping[6] = 0x5A; // echo byte
            if let Err(e) = self.send_raw(&Self::frame(&ping, f)) {
                log::debug!("{}: BLE framing {f:?} write refused: {e}", self.product);
                continue;
            }
            let deadline = Instant::now() + Duration::from_millis(900);
            while Instant::now() < deadline {
                let Ok(Some(raw)) = self.read_raw(Duration::from_millis(80)) else { continue };
                let r = Self::unframe(&raw, f);
                if r.len() >= 7 && r[2] == 0x00 && r[3] == 0x1F && r[6] == 0x5A {
                    log::info!("{}: HID++ over Bluetooth LE active (framing {f:?})", self.product);
                    self.framing = Some(f);
                    return Some(f);
                }
            }
        }
        None
    }

    pub fn write(&mut self, report: &[u8]) -> Result<()> {
        let f = match self.framing {
            Some(f) => f,
            None => self.detect_framing().ok_or_else(|| anyhow!("{} does not answer HID++ over BLE", self.product))?,
        };
        self.send_raw(&Self::frame(report, f))
    }

    pub fn read(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        let Some(f) = self.framing else { return Ok(None) };
        let Some(raw) = self.read_raw(timeout)? else { return Ok(None) };
        Ok(Some(Self::unframe(&raw, f)))
    }
}

/// Returns the keys (`ble:<uuid>`) of currently connected candidate BLE peripherals.
pub fn endpoint_keys() -> Vec<String> {
    let ptr = unsafe { fs_ble_connected_keys() };
    if ptr.is_null() {
        return Vec::new();
    }
    let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
    unsafe { fs_ble_free_string(ptr) };
    s.lines().filter(|l| !l.is_empty()).map(String::from).collect()
}
