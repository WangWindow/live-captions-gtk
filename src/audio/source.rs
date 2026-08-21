//! GStreamer/PulseAudio source resolution.

use std::cell::RefCell;
use std::rc::Rc;

use anyhow::{Context, Result, anyhow, bail};
use gstreamer as gst;
use libpulse_binding as pulse;
use pulse::callbacks::ListResult;
use pulse::context::{
    Context as PulseContext, FlagSet as PulseContextFlagSet, State as PulseState,
};
use pulse::mainloop::standard::{IterateResult, Mainloop};
use pulse::operation::{Operation, State as OperationState};

/// 音频源类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioSource {
    /// 物理麦克风输入。
    Microphone,
    /// 当前系统输出的 monitor 音频。
    SystemAudio,
}

/// Resolve a business-level source into a configured GStreamer source element.
pub fn resolve_gstreamer_source(source: AudioSource) -> Result<gst::Element> {
    gst::init().context("无法初始化 GStreamer")?;

    match source {
        AudioSource::Microphone => resolve_microphone(),
        AudioSource::SystemAudio => resolve_system_audio(),
    }
}

fn resolve_microphone() -> Result<gst::Element> {
    if gst::ElementFactory::find("pulsesrc").is_some() {
        return make_element("pulsesrc", None, None);
    }

    make_pipewire_source(false).context("无法创建麦克风 source；pulsesrc 和 pipewiresrc 均不可用")
}

fn resolve_system_audio() -> Result<gst::Element> {
    let mut pulse_error = None;
    if gst::ElementFactory::find("pulsesrc").is_some() {
        match default_pulse_monitor_source() {
            Ok(monitor_name) => return make_element("pulsesrc", Some(&monitor_name), None),
            Err(error) => pulse_error = Some(error),
        }
    }

    if gst::ElementFactory::find("pipewiresrc").is_some() {
        return make_pipewire_source(true).with_context(|| {
            format!(
                "无法创建 PipeWire 系统音频 source{}",
                pulse_error
                    .as_ref()
                    .map(|error| format!("；PulseAudio monitor 解析失败：{error}"))
                    .unwrap_or_default()
            )
        });
    }

    Err(pulse_error.unwrap_or_else(|| anyhow!("系统音频需要 pulsesrc 或 pipewiresrc plugin")))
}

fn make_element(
    factory: &str,
    device: Option<&str>,
    stream_properties: Option<gst::Structure>,
) -> Result<gst::Element> {
    let mut builder = gst::ElementFactory::make(factory).name("capture_source");
    if let Some(device) = device {
        builder = builder.property("device", device);
    }
    if let Some(stream_properties) = stream_properties {
        builder = builder.property("stream-properties", stream_properties);
    }
    builder
        .build()
        .with_context(|| format!("无法创建 GStreamer source element: {factory}"))
}

fn make_pipewire_source(system_audio: bool) -> Result<gst::Element> {
    let stream_properties = system_audio.then(|| {
        gst::Structure::builder("props")
            .field("stream.capture.sink", true)
            .build()
    });
    make_element("pipewiresrc", None, stream_properties)
}

fn default_pulse_monitor_source() -> Result<String> {
    let mut mainloop = Mainloop::new().context("无法创建 PulseAudio mainloop")?;
    let mut context =
        PulseContext::new(&mainloop, "live-captions-gtk").context("无法创建 PulseAudio context")?;
    context
        .connect(None, PulseContextFlagSet::NOAUTOSPAWN, None)
        .context("无法连接 PulseAudio server")?;
    let result = (|| -> Result<String> {
        wait_for_context_ready(&mut mainloop, &context)?;

        let default_sink = Rc::new(RefCell::new(None));
        let default_sink_callback = Rc::clone(&default_sink);
        let server_operation = context.introspect().get_server_info(move |info| {
            *default_sink_callback.borrow_mut() =
                info.default_sink_name.as_ref().map(|name| name.to_string());
        });
        wait_for_operation(&mut mainloop, &server_operation)?;
        drop(server_operation);

        let sink_name = default_sink
            .borrow_mut()
            .take()
            .context("PulseAudio 没有 default sink")?;
        let monitor_name = Rc::new(RefCell::new(None));
        let monitor_callback = Rc::clone(&monitor_name);
        let sink_operation =
            context
                .introspect()
                .get_sink_info_by_name(&sink_name, move |result| {
                    if let ListResult::Item(info) = result {
                        *monitor_callback.borrow_mut() = info
                            .monitor_source_name
                            .as_ref()
                            .map(|name| name.to_string());
                    }
                });
        wait_for_operation(&mut mainloop, &sink_operation)?;
        drop(sink_operation);

        monitor_name
            .borrow_mut()
            .take()
            .context("PulseAudio default sink 没有 monitor source")
    })();
    context.disconnect();
    result
}

fn wait_for_context_ready(mainloop: &mut Mainloop, context: &PulseContext) -> Result<()> {
    loop {
        match context.get_state() {
            PulseState::Ready => return Ok(()),
            PulseState::Failed | PulseState::Terminated => {
                bail!("PulseAudio context 状态异常: {:?}", context.get_state())
            }
            _ => match mainloop.iterate(true) {
                IterateResult::Success(_) => {}
                IterateResult::Err(error) => bail!("PulseAudio mainloop 失败: {error:?}"),
                IterateResult::Quit(retval) => {
                    bail!("PulseAudio mainloop 提前退出: {retval:?}")
                }
            },
        }
    }
}

fn wait_for_operation<T: ?Sized>(mainloop: &mut Mainloop, operation: &Operation<T>) -> Result<()> {
    loop {
        match operation.get_state() {
            OperationState::Done => return Ok(()),
            OperationState::Cancelled => bail!("PulseAudio introspection operation 被取消"),
            OperationState::Running => match mainloop.iterate(true) {
                IterateResult::Success(_) => {}
                IterateResult::Err(error) => bail!("PulseAudio mainloop 失败: {error:?}"),
                IterateResult::Quit(retval) => {
                    bail!("PulseAudio mainloop 提前退出: {retval:?}")
                }
            },
        }
    }
}
