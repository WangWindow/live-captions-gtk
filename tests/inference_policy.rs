use std::time::{Duration, Instant};

use live_captions_gtk::asr::{InferenceMetrics, InferencePolicy};

#[test]
fn automatic_policy_derives_thread_count_without_user_profiles() {
    let one_core = InferencePolicy::from_parallelism(1);
    let four_cores = InferencePolicy::from_parallelism(4);
    let many_cores = InferencePolicy::from_parallelism(64);

    assert_eq!(one_core.num_threads, 1);
    assert_eq!(four_cores.num_threads, 3);
    assert_eq!(many_cores.num_threads, 8);
    assert_eq!(four_cores.decoding_method, "greedy_search");
}

#[test]
fn audio_block_size_follows_input_sample_rate() {
    let policy = InferencePolicy::from_parallelism(4);

    assert_eq!(policy.audio_block_samples(16_000), 1_600);
    assert_eq!(policy.audio_block_samples(48_000), 4_800);
    assert_eq!(policy.audio_block_samples(0), 1);
}

#[test]
fn metrics_emit_a_snapshot_only_after_the_window() {
    let start = Instant::now();
    let mut metrics = InferenceMetrics::new(Duration::from_secs(5), start);

    assert!(
        metrics
            .record(
                start + Duration::from_secs(4),
                Duration::from_secs(1),
                Duration::from_millis(200),
                false,
            )
            .is_none()
    );
    metrics.record_drops(2);

    let snapshot = metrics
        .record(
            start + Duration::from_secs(5),
            Duration::from_secs(1),
            Duration::from_millis(300),
            true,
        )
        .expect("窗口结束后应生成诊断快照");

    assert_eq!(snapshot.blocks, 2);
    assert_eq!(snapshot.dropped_blocks, 2);
    assert_eq!(snapshot.endpoints, 1);
    assert!((snapshot.real_time_factor() - 0.25).abs() < f64::EPSILON);
}
