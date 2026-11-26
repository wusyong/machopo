//! The browser's side of the system's fonts (Chromium's
//! `FontDataServiceImpl`): one thread for the process has them, and
//! answers every renderer's questions of them, which the IO thread checks
//! and passes on. Its own thread, as Chromium's runs on a sequence that
//! may wait: the first answer of a family opens each of its files, a few
//! milliseconds no renderer's other messages wait behind. The answer goes
//! straight to the renderer, whose main thread waits for it.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use crossbeam_channel::Sender;
use protocol::font::{FONT_FILES_PER_CALL, FaceStyle, FontFace, FontFamilyFaces, FontNames};
use protocol::renderer::RendererMsg;
use renderer::{RendererSender, SystemFontManager};

use crate::bad_message::BadMessageReason;

/// Whether this platform's renderers ask the browser for the system's
/// fonts (all three kinds: a process in a sandbox, one out of it, a thread
/// of the browser's). Elsewhere a renderer still finds them itself.
pub(crate) const SERVES_SYSTEM_FONTS: bool = cfg!(windows);

/// A renderer's question, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FontRequest {
    Family { family: u32, request_id: u64 },
    Fallback { script: [u8; 4], request_id: u64 },
    Files { files: Vec<u32>, request_id: u64 },
}

impl FontRequest {
    /// The answer that answers nothing, for a question that is no
    /// question: the main thread that asked waits no more.
    fn nothing(self) -> RendererMsg {
        match self {
            Self::Family { request_id, .. } => RendererMsg::FontFamily {
                request_id,
                faces: None,
            },
            Self::Fallback { request_id, .. } => RendererMsg::FontFallback {
                request_id,
                families: Vec::new(),
            },
            Self::Files { request_id, .. } => RendererMsg::FontFiles {
                request_id,
                files: Vec::new(),
            },
        }
    }

    /// Answers nothing to `renderer`, whose question was bad.
    pub(crate) fn refuse(self, renderer: &RendererSender) {
        let _ = renderer.send(self.nothing());
    }
}

/// The font thread, and what the IO thread checks a question by.
pub(crate) struct FontDataService {
    requests: Sender<(RendererSender, FontRequest)>,
    names: FontNames,
    /// How many files the font thread has numbered: no renderer was given
    /// a number past them.
    files_numbered: Arc<AtomicU32>,
}

impl FontDataService {
    /// The process's, started the first time a renderer is given the
    /// system's fonts, which the thread finds then; none if it couldn't
    /// start. Whoever starts it waits for the names (what the renderer
    /// would have taken to find them itself, once for the process).
    pub(crate) fn get() -> Option<&'static Self> {
        static SERVICE: OnceLock<Option<FontDataService>> = OnceLock::new();
        SERVICE
            .get_or_init(|| Self::start(|| SystemFontManager::new(renderer::SystemFonts::find())))
            .as_ref()
    }

    /// A font thread with the fonts `fonts` makes there.
    pub(crate) fn start(
        fonts: impl FnOnce() -> SystemFontManager + Send + 'static,
    ) -> Option<Self> {
        let (requests, asked) = crossbeam_channel::unbounded::<(RendererSender, FontRequest)>();
        let (named, names) = crossbeam_channel::bounded(1);
        let files_numbered = Arc::new(AtomicU32::new(0));
        let numbered = files_numbered.clone();
        std::thread::Builder::new()
            .name("FontDataService".into())
            .spawn(move || {
                let mut fonts = fonts();
                let _ = named.send(font_names(fonts.names()));
                while let Ok((renderer, request)) = asked.recv() {
                    let answer = answer(&mut fonts, request);
                    // Before the renderer has the numbers, the IO thread
                    // takes them.
                    numbered.store(fonts.files_numbered(), Ordering::Release);
                    if let Some(request_id) = send(&renderer, answer) {
                        // Files it couldn't be sent: it hears of none, and
                        // waits no more.
                        let _ = renderer.send(RendererMsg::FontFiles {
                            request_id,
                            files: Vec::new(),
                        });
                    }
                }
            })
            .ok()?;
        let names = names.recv().ok()?;
        Some(Self {
            requests,
            names,
            files_numbered,
        })
    }

    /// What every renderer is given at the start.
    pub(crate) fn names(&self) -> &FontNames {
        &self.names
    }

    /// `request` from the renderer `renderer` is, on the IO thread: to the
    /// font thread if it is one the renderer may ask, else answered with
    /// nothing, and why it was bad.
    pub(crate) fn ask(
        &self,
        renderer: &RendererSender,
        request: FontRequest,
    ) -> Result<(), BadMessageReason> {
        let good = match &request {
            FontRequest::Family { family, .. } => *family < self.names.families,
            // An ISO 15924 tag: fontique looks it up as it is.
            FontRequest::Fallback { script, .. } => script.iter().all(u8::is_ascii_alphabetic),
            FontRequest::Files { files, .. } => {
                let numbered = self.files_numbered.load(Ordering::Acquire);
                !files.is_empty()
                    && files.len() <= FONT_FILES_PER_CALL
                    && files.iter().all(|&file| file < numbered)
            }
        };
        if !good {
            request.refuse(renderer);
            return Err(BadMessageReason::FontRequest);
        }
        if let Err(crossbeam_channel::SendError((renderer, request))) =
            self.requests.send((renderer.clone(), request))
        {
            // No font thread: nothing to give.
            request.refuse(&renderer);
        }
        Ok(())
    }
}

/// `request`'s answer.
fn answer(fonts: &mut SystemFontManager, request: FontRequest) -> RendererMsg {
    match request {
        FontRequest::Family { family, request_id } => RendererMsg::FontFamily {
            request_id,
            faces: fonts.family(family).map(family_faces),
        },
        FontRequest::Fallback { script, request_id } => RendererMsg::FontFallback {
            request_id,
            families: fonts.fallback(script),
        },
        FontRequest::Files { files, request_id } => RendererMsg::FontFiles {
            request_id,
            files: files
                .iter()
                .map(|&file| ipc::file::ReadOnlyFile::open(fonts.file(file)?).ok())
                .collect(),
        },
    }
}

/// Sends `answer`; if it was of files that couldn't be sent, the question
/// it answered.
fn send(renderer: &RendererSender, answer: RendererMsg) -> Option<u64> {
    let request_id = match &answer {
        RendererMsg::FontFiles { request_id, .. } => Some(*request_id),
        _ => None,
    };
    renderer.send(answer).err().and(request_id)
}

fn font_names(names: &renderer::FontNames) -> FontNames {
    FontNames {
        names: names.names.clone(),
        families: names.families,
        generics: names.generics.clone(),
    }
}

fn family_faces(faces: renderer::FontFamilyFaces) -> FontFamilyFaces {
    FontFamilyFaces {
        name: faces.name,
        faces: faces
            .faces
            .into_iter()
            .map(|face| FontFace {
                file: face.file,
                index: face.index,
                weight: face.weight,
                style: match face.style {
                    renderer::FaceStyle::Normal => FaceStyle::Normal,
                    renderer::FaceStyle::Italic => FaceStyle::Italic,
                    renderer::FaceStyle::Oblique(angle) => FaceStyle::Oblique(angle),
                },
                width: face.width,
            })
            .collect(),
    }
}
