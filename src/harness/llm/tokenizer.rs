// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Fast, process-local token counting for model families with a known BPE.
//!
//! Unknown model families return `None` and continue using Talon's
//! conservative character fallback. Provider-reported usage remains
//! authoritative after a request.

use tiktoken_rs::{bpe_for_model, CoreBPE};

#[derive(Clone, Copy)]
pub struct Tokenizer {
    bpe: &'static CoreBPE,
}

impl Tokenizer {
    pub fn for_model(model: &str) -> Option<Self> {
        bpe_for_model(model).ok().map(|bpe| Self { bpe })
    }

    #[inline]
    pub fn count_text(&self, text: &str) -> usize {
        self.bpe.encode_ordinary(text).len()
    }
}

#[cfg(test)]
mod tests {
    use super::Tokenizer;

    #[test]
    fn recognizes_openai_model_families() {
        let tokenizer = Tokenizer::for_model("gpt-4o").expect("gpt-4o tokenizer");
        assert!(tokenizer.count_text("hello world") > 0);
    }

    #[test]
    fn leaves_unknown_model_families_to_fallback() {
        assert!(Tokenizer::for_model("accounts/fireworks/models/inkling").is_none());
    }
}
