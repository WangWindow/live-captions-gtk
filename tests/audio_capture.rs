use std::sync::atomic::{AtomicU64, Ordering};

use live_captions_gtk::audio::{AudioBlockAssembler, process_chunk};
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Observer, Split};

#[test]
fn callback_processing_consumes_all_frames_beyond_legacy_chunk_size() {
    let frames = 1025;
    let channels = 2;
    let data: Vec<f32> = (0..frames * channels).map(|sample| sample as f32).collect();
    let ring = HeapRb::<f32>::new(frames);
    let (mut producer, mut consumer) = ring.split();
    let dropped_samples = AtomicU64::new(0);

    process_chunk(&data, channels, &mut producer, &dropped_samples, |sample| {
        sample
    });

    assert_eq!(consumer.occupied_len(), frames);
    assert_eq!(dropped_samples.load(Ordering::Relaxed), 0);

    let mixed: Vec<f32> = (0..frames)
        .map(|_| {
            consumer
                .try_pop()
                .expect("每个完整帧都应生成一个单声道采样")
        })
        .collect();
    assert_eq!(mixed.len(), frames);
    assert_eq!(mixed[0], 0.5);
    assert_eq!(mixed[1024], 2048.5);
}

#[test]
fn callback_processing_counts_samples_rejected_by_full_buffer() {
    let ring = HeapRb::<f32>::new(2);
    let (mut producer, mut consumer) = ring.split();
    let dropped_samples = AtomicU64::new(0);
    let data = [1.0_f32, 2.0, 3.0, 4.0];

    process_chunk(&data, 1, &mut producer, &dropped_samples, |sample| sample);

    assert_eq!(consumer.occupied_len(), 2);
    assert_eq!(dropped_samples.load(Ordering::Relaxed), 2);
}

#[test]
fn block_assembler_emits_only_complete_fixed_size_blocks() {
    let mut assembler = AudioBlockAssembler::new(4);

    assembler.push(&[1.0, 2.0, 3.0]);
    assert_eq!(assembler.remaining(), 1);
    assert!(assembler.take_block().is_none());

    assembler.push(&[4.0, 5.0]);
    assert_eq!(assembler.take_block(), Some(vec![1.0, 2.0, 3.0, 4.0]));
    assert_eq!(assembler.remaining(), 3);
    assert!(assembler.take_block().is_none());
}

#[test]
fn block_assembler_clear_discards_pending_audio() {
    let mut assembler = AudioBlockAssembler::new(4);
    assembler.push(&[1.0, 2.0, 3.0]);

    assembler.clear();

    assert_eq!(assembler.remaining(), 4);
    assert!(assembler.take_block().is_none());
}
