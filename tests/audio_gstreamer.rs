use std::time::Duration;

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_audio as gst_audio;

struct PipelineGuard(gst::Pipeline);

impl Drop for PipelineGuard {
    fn drop(&mut self) {
        let _ = self.0.set_state(gst::State::Null);
    }
}

#[test]
fn fake_source_produces_standard_audio_sample() {
    gst::init().expect("GStreamer should initialize");

    let element = gst::parse::launch(
        "audiotestsrc is-live=true wave=sine ! audioconvert ! audioresample ! \
         audio/x-raw,format=F32LE,layout=interleaved,channels=1,rate=16000 ! \
         appsink name=audio_sink sync=false max-buffers=4 drop=true",
    )
    .expect("the deterministic fake-source pipeline should parse");
    let pipeline = element
        .downcast::<gst::Pipeline>()
        .expect("parse::launch should return a pipeline");
    let sink = pipeline
        .by_name("audio_sink")
        .expect("the pipeline should contain the named appsink")
        .downcast::<gst_app::AppSink>()
        .expect("the named element should be an appsink");
    let pipeline = PipelineGuard(pipeline);

    pipeline
        .0
        .set_state(gst::State::Playing)
        .expect("the fake-source pipeline should enter Playing");

    let sample = sink
        .try_pull_sample(gst::ClockTime::from_seconds(2))
        .expect("the fake source should produce a sample within two seconds");
    let caps = sample
        .caps()
        .expect("the sample should carry negotiated caps");
    let audio_info = gst_audio::AudioInfo::from_caps(caps)
        .expect("the negotiated raw audio caps should produce AudioInfo");

    assert_eq!(audio_info.format(), gst_audio::AudioFormat::F32le);
    assert_eq!(audio_info.layout(), gst_audio::AudioLayout::Interleaved);
    assert_eq!(audio_info.channels(), 1);
    assert_eq!(audio_info.rate(), 16_000);

    let buffer = sample
        .buffer_owned()
        .expect("the sample should contain an audio buffer");
    let audio_buffer = gst_audio::AudioBuffer::from_buffer_readable(buffer, &audio_info)
        .expect("the negotiated buffer should be readable as audio");
    let plane = audio_buffer
        .plane_data(0)
        .expect("interleaved audio should have one readable plane");

    assert!(!plane.is_empty());
    assert_eq!(plane.len() % std::mem::size_of::<f32>(), 0);
}

#[test]
fn appsink_queue_is_bounded_and_drops_when_configured() {
    gst::init().expect("GStreamer should initialize");

    let element = gst::parse::launch(
        "audiotestsrc is-live=true ! audioconvert ! audioresample ! \
         audio/x-raw,format=F32LE,layout=interleaved,channels=1,rate=16000 ! \
         appsink name=audio_sink sync=false max-buffers=1 drop=true",
    )
    .expect("the bounded fake-source pipeline should parse");
    let pipeline = element
        .downcast::<gst::Pipeline>()
        .expect("parse::launch should return a pipeline");
    let sink = pipeline
        .by_name("audio_sink")
        .expect("the pipeline should contain the named appsink")
        .downcast::<gst_app::AppSink>()
        .expect("the named element should be an appsink");
    let pipeline = PipelineGuard(pipeline);

    pipeline
        .0
        .set_state(gst::State::Playing)
        .expect("the bounded fake-source pipeline should enter Playing");
    std::thread::sleep(Duration::from_millis(100));

    let _ = sink
        .try_pull_sample(gst::ClockTime::from_mseconds(10))
        .expect("the fake source should produce at least one sample");
    let dropped: u64 = sink.property("dropped");

    assert!(
        dropped > 0,
        "a live source should report dropped buffers with a one-buffer queue"
    );
}
