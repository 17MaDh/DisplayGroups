//! `AudioBackend` over Core Audio (MMDevice API).
//!
//! Setting the default endpoint uses `IPolicyConfig`, an undocumented interface that
//! the Windows Sound settings use and that tools such as SoundSwitch rely on. It is
//! called through its vtable so a missing or changed interface fails cleanly instead
//! of crashing.

use std::ffi::c_void;

use dg_engine::{AudioBackend, AudioEndpoint, EngineError};
use windows::core::{IUnknown, Interface, GUID, HRESULT, HSTRING, PCWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    STGM_READ,
};

const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870a_f99c_171d_4f9e_af0d_e63d_f40c_2bc9);
const IID_POLICY_CONFIG: GUID = GUID::from_u128(0xf867_9f50_850a_41cf_9c72_430f_2902_90c8);
/// `IPolicyConfig::SetDefaultEndpoint` is the 11th method after IUnknown's three.
const SET_DEFAULT_ENDPOINT_SLOT: usize = 13;

type SetDefaultEndpointFn =
    unsafe extern "system" fn(this: *mut c_void, id: PCWSTR, role: u32) -> HRESULT;

#[derive(Debug, Default)]
pub struct WinAudio;

impl WinAudio {
    pub fn new() -> Self {
        Self
    }
}

fn com_init() {
    // S_FALSE / RPC_E_CHANGED_MODE just mean COM is already initialised on this thread.
    // SAFETY: plain COM initialisation for the calling thread.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
}

fn err(op: &'static str) -> impl Fn(windows::core::Error) -> EngineError {
    move |e| EngineError::Win32 {
        op,
        code: e.code().0,
    }
}

fn enumerator() -> Result<IMMDeviceEnumerator, EngineError> {
    com_init();
    // SAFETY: standard COM activation.
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
        .map_err(err("MMDeviceEnumerator"))
}

fn device_id(device: &IMMDevice) -> Result<String, EngineError> {
    // SAFETY: GetId returns a CoTaskMem string that we free after copying.
    unsafe {
        let p = device.GetId().map_err(err("IMMDevice::GetId"))?;
        let id = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const c_void));
        Ok(id)
    }
}

fn friendly_name(device: &IMMDevice) -> Option<String> {
    // SAFETY: read-only property access.
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let value = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        Some(value.to_string())
    }
}

impl AudioBackend for WinAudio {
    fn render_endpoints(&self) -> Result<Vec<AudioEndpoint>, EngineError> {
        let e = enumerator()?;
        // SAFETY: COM calls on valid interfaces.
        unsafe {
            let list = e
                .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
                .map_err(err("EnumAudioEndpoints"))?;
            let count = list
                .GetCount()
                .map_err(err("IMMDeviceCollection::GetCount"))?;
            let mut out = Vec::new();
            for i in 0..count {
                let device = list.Item(i).map_err(err("IMMDeviceCollection::Item"))?;
                out.push(AudioEndpoint {
                    id: device_id(&device)?,
                    name: friendly_name(&device).unwrap_or_default(),
                });
            }
            Ok(out)
        }
    }

    fn default_endpoint(&self) -> Result<Option<String>, EngineError> {
        let e = enumerator()?;
        // SAFETY: COM call on a valid interface.
        match unsafe { e.GetDefaultAudioEndpoint(eRender, eConsole) } {
            Ok(device) => device_id(&device).map(Some),
            Err(_) => Ok(None),
        }
    }

    fn set_default_endpoint(&self, id: &str) -> Result<(), EngineError> {
        com_init();
        let wid = HSTRING::from(id);
        // SAFETY: the vtable slot is only called after QueryInterface for IPolicyConfig
        // succeeded; `policy` keeps the interface alive for the duration.
        unsafe {
            let unknown: IUnknown = CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL)
                .map_err(err("PolicyConfig"))?;
            let mut raw: *mut c_void = std::ptr::null_mut();
            unknown
                .query(&IID_POLICY_CONFIG, &mut raw)
                .ok()
                .map_err(err("IPolicyConfig"))?;
            let policy = IUnknown::from_raw(raw);
            let vtable = *(policy.as_raw() as *const *const usize);
            let set_default: SetDefaultEndpointFn =
                std::mem::transmute(*vtable.add(SET_DEFAULT_ENDPOINT_SLOT));
            // eConsole, eMultimedia, eCommunications
            for role in 0..3 {
                set_default(policy.as_raw(), PCWSTR(wid.as_ptr()), role)
                    .ok()
                    .map_err(err("IPolicyConfig::SetDefaultEndpoint"))?;
            }
        }
        Ok(())
    }

    fn set_volume(&self, id: &str, percent: u8) -> Result<(), EngineError> {
        let e = enumerator()?;
        let wid = HSTRING::from(id);
        // SAFETY: COM calls on valid interfaces.
        unsafe {
            let device = e
                .GetDevice(&wid)
                .map_err(err("IMMDeviceEnumerator::GetDevice"))?;
            let volume: IAudioEndpointVolume = device
                .Activate(CLSCTX_ALL, None)
                .map_err(err("IAudioEndpointVolume"))?;
            volume
                .SetMasterVolumeLevelScalar(f32::from(percent.min(100)) / 100.0, std::ptr::null())
                .map_err(err("SetMasterVolumeLevelScalar"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_playback_endpoints_read_only() {
        // Read-only; a machine without audio devices simply returns an empty list.
        let list = WinAudio::new().render_endpoints().unwrap();
        assert!(list.iter().all(|e| !e.id.is_empty()));
    }
}
