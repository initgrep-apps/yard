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

pub struct FileType {
    pub ext: &'static str,
    pub category: Category,
    pub output_ext: &'static str,
}

// One row list feeds the table and the per-category slices processors return.
macro_rules! file_type_table {
    ($($const_name:ident => $category:ident { $($ext:literal => $output:literal),* $(,)? }),* $(,)?) => {
        pub const FILE_TYPES: &[FileType] = &[
            $( $(FileType { ext: $ext, category: Category::$category, output_ext: $output }),* ),*
        ];
        $(
            const $const_name: &[&str] = &[$($ext),*];
        )*
        pub fn extensions_for(category: Category) -> &'static [&'static str] {
            match category {
                $( Category::$category => $const_name, )*
                Category::Other => &[],
            }
        }
    };
}

file_type_table! {
    IMAGE_EXTENSIONS => Image {
        "jpg" => "jpg",
        "jpeg" => "jpeg",
        "png" => "png",
        "webp" => "webp",
        "heic" => "jpg",
        "heif" => "jpg",
    },
    VIDEO_EXTENSIONS => Video {
        "mp4" => "mp4",
        "mov" => "mp4",
    },
    PDF_EXTENSIONS => Pdf {
        "pdf" => "pdf",
    },
    DOCUMENT_EXTENSIONS => Document {
        "docx" => "docx",
        "xlsx" => "xlsx",
        "pptx" => "pptx",
    },
}

fn by_ext(ext: &str) -> Option<&'static FileType> {
    FILE_TYPES.iter().find(|file_type| file_type.ext == ext)
}

pub fn by_name(name: &str) -> Option<&'static FileType> {
    let ext = std::path::Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())?;
    by_ext(&ext.to_lowercase())
}

// Unknown extensions keep the image processor's historical JPEG fallback.
pub fn output_extension(name: &str) -> &'static str {
    by_name(name).map_or("jpg", |file_type| file_type.output_ext)
}

pub fn output_file_name(name: &str) -> String {
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return name.to_string();
    };
    let Some(file_type) = by_ext(&ext.to_lowercase()) else {
        return name.to_string();
    };
    if file_type.output_ext == file_type.ext {
        return name.to_string();
    }
    format!("{stem}.{}", file_type.output_ext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn accepted_extensions_match_the_previous_lists() {
        let frozen: &[(Category, &[&str])] = &[
            (
                Category::Image,
                &["jpg", "jpeg", "png", "webp", "heic", "heif"],
            ),
            (Category::Video, &["mp4", "mov"]),
            (Category::Pdf, &["pdf"]),
            (Category::Document, &["docx", "xlsx", "pptx"]),
        ];
        for (category, expected) in frozen {
            assert_eq!(
                extensions_for(*category),
                *expected,
                "category {category:?} drifted from the previous list"
            );
        }
        assert_eq!(
            FILE_TYPES.len(),
            frozen
                .iter()
                .map(|(_, extensions)| extensions.len())
                .sum::<usize>(),
            "the table must hold exactly the frozen rows"
        );
    }

    #[test]
    fn no_duplicate_extension_is_registered() {
        let unique: HashSet<&str> = FILE_TYPES.iter().map(|row| row.ext).collect();
        assert_eq!(
            unique.len(),
            FILE_TYPES.len(),
            "duplicate extension in FILE_TYPES"
        );
    }

    #[test]
    fn mov_and_heic_keep_their_output_extension_rules() {
        assert_eq!(output_file_name("clip.mov"), "clip.mp4");
        assert_eq!(output_file_name("clip.MOV"), "clip.mp4");
        assert_eq!(output_file_name("a.heic"), "a.jpg");
        assert_eq!(output_file_name("a.HEIC"), "a.jpg");
        assert_eq!(output_file_name("a.heif"), "a.jpg");
    }

    #[test]
    fn unknown_extension_stays_other_and_keeps_its_name() {
        assert_eq!(crate::detection::detect_category("a.xyz"), Category::Other);
        assert_eq!(output_file_name("a.xyz"), "a.xyz");
        assert!(by_name("a.xyz").is_none());
    }

    #[test]
    fn every_row_agrees_across_detection_registry_and_naming() {
        for row in FILE_TYPES {
            let name = format!("file.{}", row.ext);
            assert_eq!(
                crate::detection::detect_category(&name),
                row.category,
                "detection disagrees for {}",
                row.ext
            );
            assert_eq!(
                output_file_name(&name),
                format!("file.{}", row.output_ext),
                "naming disagrees for {}",
                row.ext
            );
            assert_eq!(
                output_extension(&name),
                row.output_ext,
                "output extension disagrees for {}",
                row.ext
            );
        }
    }

    #[test]
    fn output_extension_maps_heic_heif_to_jpg() {
        assert_eq!(output_extension("p/photo.heic"), "jpg");
        assert_eq!(output_extension("p/photo.HEIF"), "jpg");
    }

    #[test]
    fn output_extension_preserves_png_jpg_webp() {
        assert_eq!(output_extension("p/a.png"), "png");
        assert_eq!(output_extension("p/a.JPG"), "jpg");
        assert_eq!(output_extension("p/a.webp"), "webp");
    }

    #[test]
    fn output_extension_defaults_unknown_to_jpg() {
        assert_eq!(output_extension("p/a.tiff"), "jpg");
        assert_eq!(output_extension("p/a"), "jpg");
    }
}
