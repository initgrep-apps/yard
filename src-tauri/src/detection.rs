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

use crate::models::Category;

pub fn detect_category(filename: &str) -> Category {
    crate::registry::by_name(filename).map_or(Category::Other, |file_type| file_type.category)
}

#[cfg(test)]
pub fn default_target_bytes(category: Category) -> u64 {
    let settings = crate::settings::Settings::default();
    let key = match category {
        Category::Image => "image",
        Category::Video => "video",
        Category::Pdf => "pdf",
        Category::Document => "document",
        Category::Other => "other",
    };
    settings.target_bytes_for(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_category_agrees_with_every_registry_row() {
        for row in crate::registry::FILE_TYPES {
            let name = format!("file.{}", row.ext);
            assert_eq!(detect_category(&name), row.category, "ext: {}", row.ext);
            assert_eq!(
                detect_category(&name.to_uppercase()),
                row.category,
                "uppercase ext: {}",
                row.ext
            );
        }
    }

    #[test]
    fn test_detect_unknown() {
        assert_eq!(detect_category("archive.zip"), Category::Other);
        assert_eq!(detect_category("file.xyz"), Category::Other);
        assert_eq!(detect_category("noextension"), Category::Other);
    }

    #[test]
    fn test_default_targets() {
        assert_eq!(default_target_bytes(Category::Image), 200 * 1024);
        assert_eq!(default_target_bytes(Category::Video), 10 * 1024 * 1024);
        assert_eq!(default_target_bytes(Category::Pdf), 2 * 1024 * 1024);
        assert_eq!(default_target_bytes(Category::Document), 1 * 1024 * 1024);
        assert_eq!(default_target_bytes(Category::Other), 0);
    }

    #[test]
    fn test_detect_multi_dot_filename() {
        assert_eq!(detect_category("file.v1.jpg"), Category::Image);
        assert_eq!(detect_category("my.document.v2.pdf"), Category::Pdf);
        assert_eq!(detect_category("video.clip.final.mp4"), Category::Video);
    }

    #[test]
    fn test_detect_hidden_file() {
        assert_eq!(detect_category(".hidden.jpg"), Category::Image);
        assert_eq!(detect_category(".config.png"), Category::Image);
    }

    #[test]
    fn test_detect_empty_string() {
        assert_eq!(detect_category(""), Category::Other);
    }

    #[test]
    fn test_detect_whitespace_only() {
        assert_eq!(detect_category("   "), Category::Other);
    }

    #[test]
    fn test_detect_dotless_names_not_extensions() {
        assert_eq!(detect_category("jpg"), Category::Other);
        assert_eq!(detect_category("pdf"), Category::Other);
        assert_eq!(detect_category("mp4"), Category::Other);
    }

    #[test]
    fn test_detect_trailing_dot() {
        assert_eq!(detect_category("file."), Category::Other);
    }

    #[test]
    fn test_detect_dotfile_no_ext() {
        assert_eq!(detect_category(".hidden"), Category::Other);
    }
}
