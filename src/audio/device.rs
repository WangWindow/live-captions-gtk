//! 音频设备探测
//!
//! 按用途分两大类：
//! - **输入源设备**（Microphone）—— 物理麦克风
//! - **输出源设备**（SystemAudio）—— 系统音频输出的 monitor 源
//!
//! 后端优先级：PipeWire → PulseAudio → ALSA
//!
//! - **PipeWire**：系统音频直接用默认输出设备（sink）创建捕获流，
//!   PipeWire 自动处理 `STREAM_CAPTURE_SINK`，无需查找 monitor source。
//! - **PulseAudio**：获取默认输出设备名，拼接 `.monitor` 精确匹配 monitor source。
//! - **ALSA**：无 monitor 概念，仅支持麦克风。

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait};

/// 音频源类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioSource {
    /// 物理麦克风输入
    Microphone,
    /// 系统音频输出（捕获当前正在播放的声音）
    SystemAudio,
}

/// 根据 `AudioSource` 解析出对应的 `cpal::Device`。
///
/// 自动选择最优后端：PipeWire → PulseAudio → ALSA
pub fn resolve(source: AudioSource) -> Result<cpal::Device> {
    if cpal::available_hosts().contains(&cpal::HostId::PipeWire) {
        if let Ok(host) = cpal::host_from_id(cpal::HostId::PipeWire) {
            match source {
                AudioSource::Microphone => {
                    if let Some(dev) = host.default_input_device() {
                        return Ok(dev);
                    }
                }
                AudioSource::SystemAudio => {
                    // PipeWire: 获取默认 sink，直接在上面建捕获流
                    if let Some(dev) = host.default_output_device() {
                        return Ok(dev);
                    }
                }
            }
        }
    }

    // PulseAudio
    if cpal::available_hosts().contains(&cpal::HostId::PulseAudio) {
        if let Ok(host) = cpal::host_from_id(cpal::HostId::PulseAudio) {
            match source {
                AudioSource::Microphone => {
                    if let Some(dev) = host.default_input_device() {
                        return Ok(dev);
                    }
                }
                AudioSource::SystemAudio => {
                    if let Ok(dev) = find_pa_monitor_by_default_sink(&host) {
                        return Ok(dev);
                    }
                    if let Ok(dev) = find_pa_any_monitor(&host) {
                        return Ok(dev);
                    }
                }
            }
        }
    }

    // 回退到默认主机（ALSA）
    let host = cpal::default_host();
    match source {
        AudioSource::Microphone => host.default_input_device().context("未找到麦克风设备"),
        AudioSource::SystemAudio => {
            anyhow::bail!(
                "系统音频捕获需要 PipeWire 或 PulseAudio，\n\
                 请确认已安装 pipewire-pulse 或 pulseaudio。"
            )
        }
    }
}

// ---------------------------------------------------------------------------
//  PulseAudio 辅助：通过默认 sink 名查找 monitor source
// ---------------------------------------------------------------------------

/// PulseAudio: 获取默认输出设备名 → 拼接 `.monitor` → 精确匹配输入设备
fn find_pa_monitor_by_default_sink(host: &cpal::Host) -> Result<cpal::Device> {
    let output = host
        .default_output_device()
        .context("PulseAudio 无默认输出设备")?;

    let sink_id = output.id().context("无法获取输出设备 ID")?;
    let sink_name = sink_id.id();
    let desc = output.description().ok();
    if let Some(d) = &desc {
        eprintln!("[设备] PulseAudio 默认输出: {} (ID: {sink_name})", d.name());
    }

    let monitor_name = format!("{sink_name}.monitor");
    eprintln!("[设备] PulseAudio 目标 Monitor ID: {monitor_name}");

    let devices = host.input_devices().context("无法枚举输入设备")?;

    for dev in devices {
        if let Ok(dev_id) = dev.id() {
            let name = dev_id.id();
            if name == monitor_name {
                let desc = dev.description().ok();
                let label = desc.as_ref().map(|d| d.name()).unwrap_or(name);
                eprintln!("[设备] PulseAudio 精确匹配到 Monitor: {label}");
                return Ok(dev);
            }
        }
    }

    // 回退：模糊匹配
    find_pa_any_monitor(host)
}

/// PulseAudio: 遍历所有输入设备，按 `.monitor` 后缀模糊匹配
fn find_pa_any_monitor(host: &cpal::Host) -> Result<cpal::Device> {
    let devices = host.input_devices().context("无法枚举输入设备")?;

    for dev in devices {
        if let Ok(dev_id) = dev.id() {
            let name = dev_id.id();
            if name.ends_with(".monitor") {
                let desc = dev.description().ok();
                let label = desc.as_ref().map(|d| d.name()).unwrap_or(name);
                eprintln!("[设备] PulseAudio 模糊匹配到 Monitor: {label}");
                return Ok(dev);
            }
        }
    }

    anyhow::bail!("PulseAudio 下未找到任何 monitor 设备")
}
