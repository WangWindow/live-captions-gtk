use std::fs;
use std::io::Write;

use live_captions_gtk::presets::{ModelInfo, ASR_MODELS, PUNCT_MODELS};

#[test]
fn every_catalog_file_has_a_positive_progress_estimate() {
    for model in ASR_MODELS.iter().chain(PUNCT_MODELS) {
        assert!(
            model.estimated_size_bytes() > 0,
            "{} has no size estimate",
            model.name
        );
        for file in model.files {
            assert!(!file.filename.is_empty());
            assert!(
                file.estimated_size_bytes > 0,
                "{} has no file estimate",
                file.filename
            );
        }
    }
}

#[test]
fn model_completion_requires_all_non_empty_files() {
    let model: &ModelInfo = &ASR_MODELS[0];
    let root = std::env::temp_dir().join(format!(
        "live-captions-model-catalog-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("the temporary model directory should be created");

    assert!(!model.is_complete(&root));
    for file in model.files {
        let mut output =
            fs::File::create(root.join(file.filename)).expect("model file should be created");
        output
            .write_all(b"test")
            .expect("the model file should become non-empty");
    }
    assert!(model.is_complete(&root));

    fs::remove_dir_all(&root).expect("the temporary model directory should be removed");
}
