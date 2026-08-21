use std::time::{Duration, Instant};

use live_captions_gtk::asr::PunctuationScheduler;

#[test]
fn punctuation_is_debounced_until_text_stabilizes() {
    let start = Instant::now();
    let mut scheduler = PunctuationScheduler::new(Duration::from_millis(400));

    assert!(scheduler.should_run("你好", false, start));
    scheduler.record("你好", false, start);
    assert!(!scheduler.should_run("你好", false, start + Duration::from_millis(100)));
    assert!(!scheduler.should_run("你好世界", false, start + Duration::from_millis(100)));
    assert!(scheduler.should_run("你好世界", false, start + Duration::from_millis(400)));
}

#[test]
fn endpoint_forces_one_final_punctuation_pass() {
    let start = Instant::now();
    let mut scheduler = PunctuationScheduler::new(Duration::from_secs(10));

    scheduler.record("完成", false, start);
    assert!(scheduler.should_run("完成", true, start + Duration::from_millis(1)));
    scheduler.record("完成", true, start + Duration::from_millis(1));
    assert!(!scheduler.should_run("完成", true, start + Duration::from_millis(2)));
}
