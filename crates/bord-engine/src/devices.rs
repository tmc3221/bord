use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait};

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub is_default_input: bool,
    pub is_default_output: bool,
}

pub fn list_devices() -> Result<Vec<DeviceInfo>> {
    let host = cpal::default_host();

    let default_in  = host.default_input_device().map(|d| d.name().unwrap_or_default());
    let default_out = host.default_output_device().map(|d| d.name().unwrap_or_default());

    let mut out = Vec::new();

    if let Ok(devices) = host.devices() {
        for dev in devices {
            let name = dev.name().unwrap_or_else(|_| "<unknown>".to_string());
            let is_def_in  = default_in.as_ref().map(|n| n == &name).unwrap_or(false);
            let is_def_out = default_out.as_ref().map(|n| n == &name).unwrap_or(false);
            out.push(DeviceInfo { name, is_default_input: is_def_in, is_default_output: is_def_out });
        }
    }
    Ok(out)
}

/// Pretty-print for CLI
pub fn print_devices() -> Result<()> {
    let list = list_devices()?;
    if list.is_empty() {
        println!("(no devices found)");
        return Ok(());
    }
    for (i, d) in list.iter().enumerate() {
        let mut marks = String::new();
        if d.is_default_input { marks.push_str("*I"); }
        if d.is_default_output { if !marks.is_empty() { marks.push(' ');} marks.push_str("*O"); }
        if !marks.is_empty() { print!("[{marks}] "); }
        println!("{:>2}  {}", i, d.name);
    }
    Ok(())
}

