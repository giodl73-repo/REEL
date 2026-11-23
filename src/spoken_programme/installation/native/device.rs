//! Explicit WASAPI endpoint observation, only inside the owned probe/worker.
//! No default device, input stream, capture, gain or system settings mutation.
use super::*;
use cpal::traits::{DeviceTrait, HostTrait};
use windows::{
    Win32::{
        Devices::{DeviceAndDriverInstallation::*, Properties::*},
        System::SystemInformation::{
            GetNativeSystemInfo, OSVERSIONINFOEXW, OSVERSIONINFOW, PROCESSOR_ARCHITECTURE_AMD64,
            SYSTEM_INFO,
        },
    },
    core::PCWSTR,
};

pub(super) fn os_build() -> Result<u32> {
    let mut system = SYSTEM_INFO::default();
    unsafe { GetNativeSystemInfo(&mut system) };
    ensure!(
        cfg!(target_arch = "x86_64")
            && unsafe { system.Anonymous.Anonymous.wProcessorArchitecture }
                == PROCESSOR_ARCHITECTURE_AMD64,
        "installation requires native Windows x86-64"
    );
    let mut info = OSVERSIONINFOEXW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOEXW>() as u32,
        ..Default::default()
    };
    // RtlGetVersion is not affected by application-manifest version emulation.
    let status = unsafe {
        windows::Wdk::System::SystemServices::RtlGetVersion(
            (&mut info as *mut OSVERSIONINFOEXW).cast::<OSVERSIONINFOW>(),
        )
    };
    ensure!(
        status.0 >= 0
            && info.dwMajorVersion == 10
            && info.dwBuildNumber >= 22000
            && info.wProductType == 1, // VER_NT_WORKSTATION; exclude Windows Server.
        "installation requires observed Windows 11"
    );
    Ok(info.dwBuildNumber)
}
fn driver_version(id: &cpal::DeviceId) -> Result<String> {
    let instance = format!("SWD\\MMDEVAPI\\{}", id.id());
    ensure!(
        instance.len() <= 1100,
        "installation endpoint instance profile"
    );
    let wide: Vec<u16> = instance.encode_utf16().chain(Some(0)).collect();
    let mut devinst = 0;
    // The endpoint software device's actual DriverVersion property, not CPAL's
    // friendly interface/driver label. No parent-device fallback is inferred.
    ensure!(
        unsafe {
            CM_Locate_DevNodeW(
                &mut devinst,
                PCWSTR(wide.as_ptr()),
                CM_LOCATE_DEVNODE_NORMAL,
            )
        } == CR_SUCCESS,
        "installation endpoint driver instance unavailable"
    );
    let mut data = [0u16; 128];
    let mut bytes = std::mem::size_of_val(&data) as u32;
    let mut kind = DEVPROPTYPE::default();
    let status = unsafe {
        CM_Get_DevNode_PropertyW(
            devinst,
            &DEVPKEY_Device_DriverVersion,
            &mut kind,
            Some(data.as_mut_ptr().cast()),
            &mut bytes,
            0,
        )
    };
    ensure!(
        status == CR_SUCCESS
            && kind == DEVPROP_TYPE_STRING
            && bytes >= 2
            && bytes as usize <= std::mem::size_of_val(&data)
            && bytes % 2 == 0,
        "installation endpoint driver version unavailable"
    );
    let slice = &data[..bytes as usize / 2];
    ensure!(
        slice.last() == Some(&0) && !slice[..slice.len() - 1].contains(&0),
        "installation endpoint driver version shape"
    );
    let version = String::from_utf16(&slice[..slice.len() - 1])?;
    ensure!(
        !version.trim().is_empty()
            && version.len() <= 128
            && !version.chars().any(char::is_control),
        "installation endpoint driver version profile"
    );
    Ok(version)
}
pub(super) fn observe(id_text: &str) -> Result<(cpal::Device, EndpointObservation)> {
    ensure!(
        explicit_endpoint_id(id_text)
            && id_text.len() <= 1024
            && !id_text.chars().any(char::is_control),
        "installation endpoint ID profile"
    );
    let id: cpal::DeviceId = id_text.parse()?;
    ensure!(
        id.host() == cpal::HostId::Wasapi,
        "installation requires explicit WASAPI ID"
    );
    let build = os_build()?;
    let host = cpal::host_from_id(cpal::HostId::Wasapi)?;
    let mut found = None;
    let mut count = 0;
    for device in host.devices()?.take(65) {
        count += 1;
        ensure!(count <= 64, "installation endpoint enumeration budget");
        if device.id()? == id {
            ensure!(
                found.is_none() && device.supports_output(),
                "installation endpoint ambiguous or not output"
            );
            found = Some(device);
        }
    }
    let device = found.ok_or_else(|| anyhow::anyhow!("installation bound endpoint unavailable"))?;
    let driver = driver_version(&id)?;
    let mix = device.default_output_config()?;
    ensure!(
        mix.sample_rate() == 48000
            && mix.channels() == 2
            && mix.sample_format() == cpal::SampleFormat::F32,
        "installation endpoint native mix must be 48k stereo float32"
    );
    let mut configurations = Vec::new();
    let mut supports = false;
    for config in device.supported_output_configs()?.take(65) {
        ensure!(
            configurations.len() < 64,
            "installation endpoint format budget"
        );
        let (min, max) = match config.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => (*min, *max),
            cpal::SupportedBufferSize::Unknown => (0, 0),
        };
        if config.channels() == 2
            && config.sample_format() == cpal::SampleFormat::F32
            && config.contains_rate(48000)
            && (max == 0 || (min <= 480 && max >= 480))
        {
            supports = true
        }
        configurations.push((
            config.channels(),
            format!("{:?}", config.sample_format()),
            config.min_sample_rate(),
            config.max_sample_rate(),
            min,
            max,
        ));
    }
    ensure!(supports, "installation exact native stream unsupported");
    configurations.sort();
    let hash = crate::production::sha256_bytes(&serde_json::to_vec(&serde_json::json!({
        "id":id_text,"os_build":build,"endpoint_driver_version":driver,
        "mix":[mix.sample_rate(),u32::from(mix.channels())],"format":"F32","configurations":configurations
    }))?);
    let binding = EndpointBinding {
        profile: PROFILE.into(),
        device_id: id_text.into(),
        os_build: build,
        driver_version: driver,
        configuration_sha256: hash,
    };
    binding.validate()?;
    Ok((
        device,
        EndpointObservation {
            binding,
            sample_rate: 48000,
            channels: 2,
            format: "f32".into(),
            requested_buffer_frames: 480,
            effective_buffer_checked: false,
            device_playback_checked: false,
        },
    ))
}
