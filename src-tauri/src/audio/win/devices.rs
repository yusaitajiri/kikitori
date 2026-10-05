//! Capture device list (FR-13).

use wasapi::{DeviceEnumerator, Direction, Role};

use crate::audio::source::Device;

/// Runs `f` on a fresh thread with COM initialized for the multi-threaded apartment, so
/// command threads never need COM.
pub fn run_in_mta<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::spawn(move || {
        let _ = wasapi::initialize_mta();
        let out = f();
        wasapi::deinitialize();
        out
    })
    .join()
    .expect("COM helper thread panicked")
}

pub fn list_mic_devices() -> anyhow::Result<Vec<Device>> {
    run_in_mta(|| -> anyhow::Result<Vec<Device>> {
        let enumerator = DeviceEnumerator::new()?;
        let default_id = enumerator
            .get_default_device_for_role(&Direction::Capture, &Role::Communications)
            .and_then(|d| d.get_id())
            .ok();
        let collection = enumerator.get_device_collection(&Direction::Capture)?;
        let mut out = Vec::new();
        for i in 0..collection.get_nbr_devices()? {
            let device = collection.get_device_at_index(i)?;
            let id = device.get_id()?;
            let name = device.get_friendlyname().unwrap_or_else(|_| id.clone());
            out.push(Device { is_default: default_id.as_deref() == Some(id.as_str()), id, name });
        }
        out.sort_by(|a, b| b.is_default.cmp(&a.is_default).then(a.name.cmp(&b.name)));
        Ok(out)
    })
}

/// Friendly name of a capture device, or of the default communications device.
pub fn mic_device_name(device_id: Option<String>) -> Option<String> {
    run_in_mta(move || {
        let enumerator = DeviceEnumerator::new().ok()?;
        let device = match device_id {
            Some(id) => enumerator.get_device(&id).ok()?,
            None => enumerator.get_default_device_for_role(&Direction::Capture, &Role::Communications).ok()?,
        };
        device.get_friendlyname().ok()
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn listing_devices_does_not_fail() {
        // A CI runner may have no capture devices; the call itself must still succeed.
        let devices = super::list_mic_devices().unwrap();
        assert!(devices.iter().filter(|d| d.is_default).count() <= 1);
    }
}
