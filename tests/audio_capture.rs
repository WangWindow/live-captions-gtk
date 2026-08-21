use live_captions_gtk::audio::AudioBlockAssembler;

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
