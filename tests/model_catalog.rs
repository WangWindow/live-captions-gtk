use std::fs;
use std::io::Write;

use live_captions_gtk::presets::{
    find_model_by_dir, ModelCategory, ModelInfo, ASR_MODELS, PUNCT_MODELS,
};

#[test]
fn every_catalog_file_declares_a_required_name() {
    for model in ASR_MODELS.iter().chain(PUNCT_MODELS) {
        assert!(model
            .archive_url
            .starts_with("https://github.com/k2-fsa/sherpa-onnx/releases/download/"));
        assert!(model.archive_url.ends_with(".tar.bz2"));
        for file in model.files {
            assert!(!file.filename.is_empty());
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
    assert_eq!(
        find_model_by_dir(&root).map(|model| model.category),
        Some(ModelCategory::Asr)
    );

    fs::remove_dir_all(&root).expect("the temporary model directory should be removed");
}
