//! What the browser tells a renderer of the system's fonts (Chromium's
//! `font_data_service.mojom`): the names at the start, then a family's
//! faces, a script's fallback and the files the faces are in, each when the
//! renderer's text first needs it (`RendererHostMsg::FontFamily`,
//! `FontFallback`, `FontFiles`). Families and files are the browser's
//! numbers, which it checks: a renderer is never told a path, and asks of
//! nothing it wasn't told.

use serde::{Deserialize, Serialize};

/// Every name a family of the system's goes by, and the family's number;
/// the families each generic family stands for, in the order the
/// renderer's text knows the generic families in. Every family's number is
/// below `families`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontNames {
    pub names: Vec<(String, u32)>,
    pub families: u32,
    pub generics: Vec<Vec<u32>>,
}

/// A face's slant.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FaceStyle {
    Normal,
    Italic,
    Oblique(Option<f32>),
}

/// One face of a family: its file's number, which of the file's faces it
/// is, and what the system says of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontFace {
    pub file: u32,
    pub index: u32,
    pub weight: f32,
    pub style: FaceStyle,
    pub width: f32,
}

/// A family's name as the system has it, and its faces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontFamilyFaces {
    pub name: String,
    pub faces: Vec<FontFace>,
}

/// How many files a renderer asks for at once: as many as go with one
/// message.
pub const FONT_FILES_PER_CALL: usize = 16;

#[cfg(test)]
mod tests {
    #[test]
    fn as_many_files_are_asked_for_as_go_with_a_message() {
        assert_eq!(
            Ok(super::FONT_FILES_PER_CALL),
            usize::try_from(ipc::core::frame::MAX_HANDLES)
        );
    }
}
