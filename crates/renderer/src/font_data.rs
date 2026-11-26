//! How the renderer's text asks the browser of the system's fonts
//! (Chromium's `font_data_service::FontManager`, the renderer's end of
//! `FontDataService`): each a call the main thread waits for, every task
//! left queued, as a cookie read is. What the browser gives is kept by the
//! text's font collection, so each family, script and file is asked for
//! once.

use std::sync::Arc;

use protocol::control_channel::BrowserSender;
use protocol::renderer::{RendererHostMsg, RendererMsg};

use crate::task_queue::MainThreadScheduler;

// The text asks for as many files at once as the browser takes.
const _: () = assert!(layout::FONT_FILES_PER_CALL == protocol::font::FONT_FILES_PER_CALL);

pub(crate) struct BrowserFontDataService {
    pub(crate) to_browser: BrowserSender,
    pub(crate) scheduler: Arc<MainThreadScheduler>,
}

impl layout::FontDataService for BrowserFontDataService {
    fn family(&mut self, family: u32) -> Option<layout::FontFamilyFaces> {
        let reply = self.scheduler.sync_call(|request_id| {
            let _ = self
                .to_browser
                .send(RendererHostMsg::GetFontFamily { family, request_id });
        });
        match reply {
            Some(RendererMsg::FontFamily { faces, .. }) => faces.map(family_faces),
            _ => None,
        }
    }

    fn fallback(&mut self, script: [u8; 4]) -> Vec<u32> {
        let reply = self.scheduler.sync_call(|request_id| {
            let _ = self
                .to_browser
                .send(RendererHostMsg::GetFontFallback { script, request_id });
        });
        match reply {
            Some(RendererMsg::FontFallback { families, .. }) => families,
            _ => Vec::new(),
        }
    }

    fn files(&mut self, files: &[u32]) -> Vec<Option<layout::FontFileBytes>> {
        let reply = self.scheduler.sync_call(|request_id| {
            let _ = self.to_browser.send(RendererHostMsg::GetFontFiles {
                files: files.to_vec(),
                request_id,
            });
        });
        match reply {
            Some(RendererMsg::FontFiles { files, .. }) => files
                .into_iter()
                .map(|file| {
                    // Mapped for as long as the collection has it: the
                    // font's glyphs are read from it as text is shaped and
                    // drawn.
                    let bytes: layout::FontFileBytes = Arc::new(file?.map()?);
                    Some(bytes)
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// The names the browser gave, as the renderer's text takes them.
pub(crate) fn font_names(names: protocol::font::FontNames) -> layout::FontNames {
    let protocol::font::FontNames {
        names,
        families,
        generics,
    } = names;
    layout::FontNames {
        names,
        families,
        generics,
    }
}

fn family_faces(faces: protocol::font::FontFamilyFaces) -> layout::FontFamilyFaces {
    layout::FontFamilyFaces {
        name: faces.name,
        faces: faces
            .faces
            .into_iter()
            .map(|face| layout::FontFace {
                file: face.file,
                index: face.index,
                weight: face.weight,
                style: match face.style {
                    protocol::font::FaceStyle::Normal => layout::FaceStyle::Normal,
                    protocol::font::FaceStyle::Italic => layout::FaceStyle::Italic,
                    protocol::font::FaceStyle::Oblique(angle) => layout::FaceStyle::Oblique(angle),
                },
                width: face.width,
            })
            .collect(),
    }
}
