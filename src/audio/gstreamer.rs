//! GStreamer audio capture boundary.
//!
//! The source resolver selects the source element; this module owns the
//! common pipeline tail, appsink queue, sample mapping, and lifecycle
//! semantics shared by all sources.

use std::time::Duration;

use anyhow::{anyhow, ensure, Context, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_audio as gst_audio;
use gstreamer_audio::audio_buffer::Readable;

use super::source::AudioSource;

const AUDIO_SINK_NAME: &str = "audio_sink";

/// The format handed to the ASR layer by every GStreamer capture pipeline.
pub fn standard_audio_caps() -> gst::Caps {
    gst::Caps::builder("audio/x-raw")
        .field("format", "F32LE")
        .field("layout", "interleaved")
        .field("channels", 1i32)
        .field("rate", 16_000i32)
        .build()
}

/// GStreamer-backed audio capture session.
pub struct GstreamerCapture {
    pipeline: gst::Pipeline,
    appsink: gst_app::AppSink,
    audio_info: gst_audio::AudioInfo,
    last_dropped_buffers: u64,
}

impl GstreamerCapture {
    /// Build and start a capture pipeline whose final element is a named
    /// `appsink` called `audio_sink`.
    ///
    /// The description must negotiate the standard caps returned by
    /// [`standard_audio_caps`]. This constructor remains useful for
    /// deterministic fake-source integration tests.
    pub fn from_pipeline_description(description: &str) -> Result<Self> {
        gst::init().context("无法初始化 GStreamer")?;

        let element = gst::parse::launch(description).context("无法解析 GStreamer 音频管线")?;
        let pipeline = element
            .downcast::<gst::Pipeline>()
            .map_err(|_| anyhow!("音频描述必须解析为 GStreamer pipeline"))?;
        Self::from_pipeline(pipeline)
    }

    /// Build a standard capture pipeline around a source element created by a
    /// GStreamer device/provider resolver.
    pub fn from_source_element(source: gst::Element) -> Result<Self> {
        gst::init().context("无法初始化 GStreamer")?;

        let convert = gst::ElementFactory::make("audioconvert")
            .name("audio_convert")
            .build()
            .context("缺少 GStreamer audioconvert plugin")?;
        let resample = gst::ElementFactory::make("audioresample")
            .name("audio_resample")
            .build()
            .context("缺少 GStreamer audioresample plugin")?;
        let capsfilter = gst::ElementFactory::make("capsfilter")
            .name("audio_caps")
            .property("caps", standard_audio_caps())
            .build()
            .context("缺少 GStreamer capsfilter plugin")?;
        let appsink = gst::ElementFactory::make("appsink")
            .name(AUDIO_SINK_NAME)
            .build()
            .context("缺少 GStreamer appsink plugin")?;

        let pipeline = gst::Pipeline::new();
        pipeline
            .add_many([&source, &convert, &resample, &capsfilter, &appsink])
            .context("无法将 GStreamer 音频元素加入 pipeline")?;
        gst::Element::link_many([&source, &convert, &resample, &capsfilter, &appsink])
            .context("无法链接 GStreamer 音频 source pipeline")?;

        Self::from_pipeline(pipeline)
    }

    /// Resolve the requested business source through GStreamer/PulseAudio
    /// providers and start the standard capture pipeline.
    pub fn start(source: AudioSource) -> Result<Self> {
        let source_element = super::source::resolve_gstreamer_source(source)?;
        Self::from_source_element(source_element)
    }

    fn from_pipeline(pipeline: gst::Pipeline) -> Result<Self> {
        let appsink = pipeline
            .by_name(AUDIO_SINK_NAME)
            .context("GStreamer pipeline 缺少名为 audio_sink 的 appsink")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| anyhow!("audio_sink 不是 appsink 元素"))?;

        appsink.set_max_buffers(8);
        appsink.set_leaky_type(gst_app::AppLeakyType::Downstream);
        appsink.set_property("emit-signals", false);

        let caps = standard_audio_caps();
        let audio_info =
            gst_audio::AudioInfo::from_caps(&caps).context("无法从标准音频 caps 创建 AudioInfo")?;
        let last_dropped_buffers = appsink.property::<u64>("dropped");

        if let Err(state_error) = pipeline.set_state(gst::State::Playing) {
            let detail = pipeline_error_detail(&pipeline);
            return Err(anyhow!(
                "无法启动 GStreamer 音频管线（状态错误：{state_error:?}；{detail}）"
            ));
        }

        Ok(Self {
            pipeline,
            appsink,
            audio_info,
            last_dropped_buffers,
        })
    }

    /// Pull one negotiated GStreamer sample without blocking longer than the
    /// caller-provided timeout.
    pub fn try_pull_sample(&self, timeout: Duration) -> Result<Option<gst::Sample>> {
        let timeout =
            gst::ClockTime::from_nseconds(timeout.as_nanos().try_into().unwrap_or(u64::MAX));
        Ok(self.appsink.try_pull_sample(timeout))
    }

    /// The negotiated audio contract used to map samples at the ASR boundary.
    pub fn audio_info(&self) -> &gst_audio::AudioInfo {
        &self.audio_info
    }

    /// Map a GStreamer sample into the f32 slice expected by Sherpa-ONNX.
    ///
    /// The sample remains a standard `gst::Sample` until this boundary. The
    /// only application-owned copy is the final `Vec<f32>` returned here.
    pub fn samples_from_sample(
        sample: &gst::Sample,
        audio_info: &gst_audio::AudioInfo,
    ) -> Result<Vec<f32>> {
        let caps = sample.caps().context("GStreamer sample 缺少音频 caps")?;
        let sample_info = gst_audio::AudioInfo::from_caps(caps)
            .context("无法从 GStreamer sample caps 创建 AudioInfo")?;
        ensure!(
            sample_info == *audio_info,
            "GStreamer sample caps 与期望音频格式不一致: {sample_info:?} != {audio_info:?}"
        );
        ensure!(
            audio_info.format() == gst_audio::AudioFormat::F32le
                && audio_info.layout() == gst_audio::AudioLayout::Interleaved
                && audio_info.channels() == 1
                && audio_info.rate() == 16_000,
            "GStreamer sample 不是标准 F32LE mono 16kHz 音频: {audio_info:?}"
        );

        let buffer = sample
            .buffer_owned()
            .context("GStreamer sample 缺少音频 buffer")?;
        let audio_buffer = gst_audio::AudioBuffer::<Readable>::from_buffer_readable(
            buffer, audio_info,
        )
        .map_err(|buffer| anyhow!("无法读取 GStreamer 音频 buffer（{} bytes）", buffer.size()))?;
        let plane = audio_buffer
            .plane_data(0)
            .context("无法读取 GStreamer 音频 plane")?;
        let mut chunks = plane.chunks_exact(std::mem::size_of::<f32>());
        let samples = chunks
            .by_ref()
            .map(|bytes| f32::from_le_bytes(bytes.try_into().expect("chunk size is four")))
            .collect();
        ensure!(
            chunks.remainder().is_empty(),
            "GStreamer F32LE 音频 buffer 长度不是完整 f32 样本"
        );

        Ok(samples)
    }

    /// Return the number of newly dropped appsink buffers since the previous
    /// call, using GStreamer's monotonic `dropped` property.
    pub fn take_dropped_buffers(&mut self) -> u64 {
        let dropped = self.appsink.property::<u64>("dropped");
        let delta = dropped.saturating_sub(self.last_dropped_buffers);
        self.last_dropped_buffers = dropped;
        delta
    }

    /// Stop the pipeline and release the source. Repeated calls are safe.
    pub fn stop(&mut self) -> Result<()> {
        self.pipeline
            .set_state(gst::State::Null)
            .context("无法停止 GStreamer 音频管线")?;
        Ok(())
    }
}

impl Drop for GstreamerCapture {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn pipeline_error_detail(pipeline: &gst::Pipeline) -> String {
    let Some(bus) = pipeline.bus() else {
        return "pipeline 没有 bus 错误消息".into();
    };
    let Some(message) = bus.timed_pop_filtered(
        gst::ClockTime::from_mseconds(100),
        &[gst::MessageType::Error],
    ) else {
        return "未收到 GStreamer ERROR 消息".into();
    };
    match message.view() {
        gst::MessageView::Error(error) => format!(
            "{}；debug: {}",
            error.error(),
            error.debug().as_deref().unwrap_or("无")
        ),
        _ => "收到无法解析的 GStreamer 错误消息".into(),
    }
}
