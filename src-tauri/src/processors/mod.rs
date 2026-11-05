// Yard — offline batch file compression
// Copyright (C) 2026 Yard Contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

pub mod document;
pub mod image;
pub mod pdf;
pub mod video;

use std::collections::HashMap;

use crate::engine::CompressionProcessor;
use crate::models::Category;

pub struct ProcessorRegistry {
    processors: HashMap<Category, Box<dyn CompressionProcessor>>,
    extension_map: HashMap<String, Category>,
}

impl ProcessorRegistry {
    pub fn new() -> Self {
        let mut registry = ProcessorRegistry {
            processors: HashMap::new(),
            extension_map: HashMap::new(),
        };
        registry.register(Box::new(image::ImageProcessor::new()));
        registry.register(Box::new(pdf::PdfProcessor::new()));
        registry.register(Box::new(video::VideoProcessor::new()));
        registry.register(Box::new(document::DocumentProcessor::new()));
        registry
    }

    fn register(&mut self, processor: Box<dyn CompressionProcessor>) {
        let category = processor.category();
        for ext in processor.supported_extensions() {
            let ext = ext.to_lowercase();
            assert!(
                !self.extension_map.contains_key(&ext),
                "duplicate extension {ext} registered in the file-type registry"
            );
            self.extension_map.insert(ext, category);
        }
        self.processors.insert(category, processor);
    }

    pub fn get_processor(&self, category: Category) -> Option<&dyn CompressionProcessor> {
        self.processors.get(&category).map(|p| p.as_ref())
    }

    pub fn lookup(&self, ext: &str) -> Option<Category> {
        self.extension_map.get(&ext.to_lowercase()).copied()
    }
}

impl Default for ProcessorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{CompressionError, CompressionResult, ProgressSender, SidecarPaths};
    use crate::settings::QualityFloors;
    use std::path::Path;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn registry_maps_every_file_type_to_its_category() {
        let registry = ProcessorRegistry::new();
        for row in crate::registry::FILE_TYPES {
            assert_eq!(
                registry.lookup(row.ext),
                Some(row.category),
                "ext: {}",
                row.ext
            );
            assert_eq!(
                registry.lookup(&row.ext.to_uppercase()),
                Some(row.category),
                "uppercase ext: {}",
                row.ext
            );
        }
        assert_eq!(registry.lookup("xyz"), None);
        assert_eq!(
            registry.extension_map.len(),
            crate::registry::FILE_TYPES.len(),
            "the processor map must hold exactly the file-type rows"
        );
    }

    #[test]
    fn every_file_type_category_has_a_processor() {
        let registry = ProcessorRegistry::new();
        for category in [
            Category::Image,
            Category::Video,
            Category::Pdf,
            Category::Document,
        ] {
            assert!(
                registry.get_processor(category).is_some(),
                "category: {category:?}"
            );
        }
        assert!(registry.get_processor(Category::Other).is_none());
    }

    struct StubProcessor(&'static [&'static str]);

    impl CompressionProcessor for StubProcessor {
        fn category(&self) -> Category {
            Category::Image
        }

        fn supported_extensions(&self) -> &[&str] {
            self.0
        }

        fn compress_to_target(
            &self,
            _input: &Path,
            _output: &Path,
            _target_bytes: u64,
            _quality_floor: &QualityFloors,
            _cancel_token: CancellationToken,
            _progress: ProgressSender,
            _sidecar_paths: &SidecarPaths,
            _temp_dir: &Path,
        ) -> Result<CompressionResult, CompressionError> {
            unreachable!()
        }
    }

    #[test]
    #[should_panic(expected = "duplicate extension")]
    fn duplicate_extension_registration_is_rejected() {
        let mut registry = ProcessorRegistry::new();
        registry.register(Box::new(StubProcessor(&["png"])));
    }
}
