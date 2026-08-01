// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Fast, process-local token counting for model families with a known BPE.
//!
//! Unknown model families return `None` and continue using Talon's
//! conservative character fallback. Provider-reported usage remains
//! authoritative after a request.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

use tiktoken_rs::{bpe_for_model, CoreBPE};
use tokenizers::Tokenizer as HuggingFaceTokenizer;

type HuggingFace = HuggingFaceTokenizer;

#[derive(Clone)]
pub struct Tokenizer {
    backend: Backend,
}

#[derive(Clone)]
enum Backend {
    Tiktoken(&'static CoreBPE),
    HuggingFace(Arc<HuggingFace>),
}

static HUGGINGFACE_CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<HuggingFace>>>> = OnceLock::new();

impl Tokenizer {
    pub fn for_model(model: &str) -> Option<Self> {
        if let Ok(bpe) = bpe_for_model(model) {
            return Some(Self {
                backend: Backend::Tiktoken(bpe),
            });
        }

        configured_huggingface_tokenizer(model).and_then(|path| Self::from_file(&path).ok())
    }

    /// Load a locally provisioned Hugging Face `tokenizer.json`.
    ///
    /// The file is cached by canonical path, so request handling never repeats
    /// the relatively expensive JSON parse. Network downloads deliberately do
    /// not happen here; tokenizer assets should be baked into the image or
    /// mounted at startup.
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let path = path
            .canonicalize()
            .map_err(|error| format!("canonicalize tokenizer {}: {error}", path.display()))?;
        let cache = HUGGINGFACE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        let mut cache = cache
            .lock()
            .map_err(|_| "tokenizer cache lock poisoned".to_owned())?;
        let tokenizer = if let Some(tokenizer) = cache.get(&path) {
            Arc::clone(tokenizer)
        } else {
            let tokenizer = Arc::new(
                HuggingFaceTokenizer::from_file(&path)
                    .map_err(|error| format!("load tokenizer {}: {error}", path.display()))?,
            );
            cache.insert(path, Arc::clone(&tokenizer));
            tokenizer
        };
        Ok(Self {
            backend: Backend::HuggingFace(tokenizer),
        })
    }

    #[inline]
    pub fn count_text(&self, text: &str) -> usize {
        match &self.backend {
            Backend::Tiktoken(bpe) => bpe.encode_ordinary(text).len(),
            Backend::HuggingFace(tokenizer) => tokenizer
                .encode(text, false)
                .map(|encoding| encoding.len())
                .unwrap_or_else(|_| text.len().div_ceil(3)),
        }
    }
}

fn configured_huggingface_tokenizer(model: &str) -> Option<PathBuf> {
    let root = std::env::var_os("TALON_TOKENIZER_DIR")?;
    let root = PathBuf::from(root);
    if root.is_file() {
        return Some(root);
    }

    let model_name = model.rsplit('/').next().unwrap_or(model);
    [
        root.join(model_name).join("tokenizer.json"),
        root.join(format!("{model_name}.json")),
        root.join("tokenizer.json"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{Backend, HuggingFaceTokenizer, Tokenizer};

    #[test]
    fn recognizes_openai_model_families() {
        let tokenizer = Tokenizer::for_model("gpt-4o").expect("gpt-4o tokenizer");
        assert!(tokenizer.count_text("hello world") > 0);
    }

    #[test]
    fn leaves_unknown_model_families_to_fallback() {
        assert!(Tokenizer::for_model("accounts/fireworks/models/inkling").is_none());
    }

    #[test]
    fn counts_a_locally_loaded_huggingface_tokenizer() {
        let tokenizer = HuggingFaceTokenizer::new(
            tokenizers::models::wordlevel::WordLevel::builder()
                .vocab([("[UNK]".to_owned(), 0), ("hello".to_owned(), 1)].into())
                .unk_token("[UNK]".to_owned())
                .build()
                .expect("word-level tokenizer"),
        );
        let tokenizer = Tokenizer {
            backend: Backend::HuggingFace(Arc::new(tokenizer)),
        };
        assert_eq!(tokenizer.count_text("hello"), 1);
    }
}
