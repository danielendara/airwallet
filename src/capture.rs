//! Maintainer screenshot capture for README / docs.
//!
//! ```text
//! COFFERLY_CAPTURE=story-unlock,wallet,settings \
//! COFFERLY_CAPTURE_DIR=docs/screenshots \
//! COFFERLY_DATA_DIR=/tmp/cofferly-capture \
//! cargo run --release
//! ```
//!
//! Do not set these in normal family use.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::NaiveDate;
use eframe::egui::{self, ColorImage};

use crate::crypto::{self, SessionCrypto};
use crate::data::{AppData, Entry, EntryKind, Wallet};
use crate::io;
use crate::money::format_money_input;
use crate::story::{self, STORY_LENGTH};
use crate::{CofferlyApp, LockMode, Status};

const DEMO_STORY: [&str; STORY_LENGTH] = ["apple", "lantern", "diamond", "leaf", "fox", "flower"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureTarget {
    StoryUnlock,
    Wallet,
    Settings,
}

impl CaptureTarget {
    fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "story-unlock" | "story" | "unlock" => Some(Self::StoryUnlock),
            "wallet" | "ledger" => Some(Self::Wallet),
            "settings" => Some(Self::Settings),
            _ => None,
        }
    }

    fn file_name(self) -> &'static str {
        match self {
            Self::StoryUnlock => "cofferly-story-unlock.png",
            Self::Wallet => "cofferly-wallet-screen.png",
            Self::Settings => "cofferly-settings-screen.png",
        }
    }
}

pub struct CaptureSession {
    out_dir: PathBuf,
    queue: VecDeque<CaptureTarget>,
    current: Option<CaptureTarget>,
    frames_on_target: u32,
    waiting_for_shot: bool,
    done: bool,
}

impl CaptureSession {
    /// Returns `None` when capture mode is not requested.
    pub fn from_env() -> Option<Self> {
        let raw = std::env::var("COFFERLY_CAPTURE").ok()?;
        if raw.trim().is_empty() {
            return None;
        }
        let mut queue = VecDeque::new();
        for part in raw.split(',') {
            if let Some(target) = CaptureTarget::parse(part) {
                queue.push_back(target);
            } else if !part.trim().is_empty() {
                eprintln!(
                    "COFFERLY_CAPTURE: unknown target '{part}' (use story-unlock,wallet,settings)"
                );
            }
        }
        if queue.is_empty() {
            return None;
        }
        let out_dir = std::env::var("COFFERLY_CAPTURE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("docs/screenshots"));
        Some(Self {
            out_dir,
            queue,
            current: None,
            frames_on_target: 0,
            waiting_for_shot: false,
            done: false,
        })
    }

    pub fn tick(&mut self, app: &mut CofferlyApp, ctx: &egui::Context) {
        if self.done {
            return;
        }

        if self.current.is_none() {
            let Some(next) = self.queue.pop_front() else {
                self.done = true;
                eprintln!(
                    "COFFERLY_CAPTURE: all screenshots written to {}",
                    self.out_dir.display()
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                // Ensure the maintainer script does not hang if the OS ignores Close.
                std::process::exit(0);
            };
            prepare_target(app, next);
            self.current = Some(next);
            self.frames_on_target = 0;
            self.waiting_for_shot = false;
            return;
        }

        self.frames_on_target = self.frames_on_target.saturating_add(1);
        if !self.waiting_for_shot && self.frames_on_target >= 5 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            self.waiting_for_shot = true;
        }

        let mut saved_path: Option<PathBuf> = None;
        ctx.input(|input| {
            for event in &input.events {
                if let egui::Event::Screenshot { image, .. } = event {
                    if let Some(target) = self.current {
                        let path = self.out_dir.join(target.file_name());
                        match save_color_image(image, &path) {
                            Ok(()) => {
                                eprintln!("COFFERLY_CAPTURE: wrote {}", path.display());
                                saved_path = Some(path);
                            }
                            Err(err) => {
                                eprintln!(
                                    "COFFERLY_CAPTURE: failed to write {}: {err}",
                                    path.display()
                                );
                                // Advance anyway so we do not loop forever.
                                saved_path = Some(path);
                            }
                        }
                    }
                }
            }
        });

        if saved_path.is_some() {
            self.current = None;
            self.waiting_for_shot = false;
            self.frames_on_target = 0;
        }
    }
}

fn prepare_target(app: &mut CofferlyApp, target: CaptureTarget) {
    match target {
        CaptureTarget::StoryUnlock => prepare_story_unlock(app),
        CaptureTarget::Wallet => prepare_wallet(app, false),
        CaptureTarget::Settings => prepare_wallet(app, true),
    }
    app.ledger_cache = None;
    app.last_interaction = std::time::Instant::now();
}

fn demo_wallets() -> Vec<Wallet> {
    let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day).expect("valid demo date");
    vec![
        Wallet {
            child_name: "Child 1".to_owned(),
            starting_balance_cents: 2_500,
            entries: vec![
                Entry {
                    date: d(2026, 7, 1),
                    description: "Weekly allowance".to_owned(),
                    amount_cents: 1_000,
                },
                Entry {
                    date: d(2026, 7, 12),
                    description: "Birthday gift from Grandma".to_owned(),
                    amount_cents: 2_000,
                },
                Entry {
                    date: d(2026, 7, 18),
                    description: "Art supplies".to_owned(),
                    amount_cents: -650,
                },
            ],
        },
        Wallet {
            child_name: "Child 2".to_owned(),
            starting_balance_cents: 1_500,
            entries: vec![
                Entry {
                    date: d(2026, 7, 1),
                    description: "Weekly allowance".to_owned(),
                    amount_cents: 1_000,
                },
                Entry {
                    date: d(2026, 7, 8),
                    description: "Chore bonus".to_owned(),
                    amount_cents: 500,
                },
                Entry {
                    date: d(2026, 7, 20),
                    description: "Book fair".to_owned(),
                    amount_cents: -875,
                },
            ],
        },
    ]
}

fn demo_app_data() -> AppData {
    AppData {
        parent_pin: String::new(),
        wallets: demo_wallets(),
    }
}

fn demo_pin() -> zeroize::Zeroizing<String> {
    story::encode(&DEMO_STORY).expect("demo story encodes")
}

/// Catalog order so the unlock grid is stable in documentation screenshots.
fn stable_display_order() -> Vec<&'static str> {
    story::CATALOG.iter().map(|(id, _)| *id).collect()
}

fn persist_demo(app: &mut CofferlyApp, data: &AppData) -> SessionCrypto {
    let pin = demo_pin();
    let mut session_slot: Option<SessionCrypto> = None;
    let plain = serde_json::to_vec(data).expect("demo data serializes");
    let encrypted = crypto::encrypt(&plain, pin.as_str(), &mut session_slot).expect("demo encrypt");
    io::save_encrypted_bytes(&app.data_path, &encrypted).expect("demo write vault");
    app.raw_bytes = Some(encrypted);
    session_slot.expect("session established during encrypt")
}

fn prepare_story_unlock(app: &mut CofferlyApp) {
    let data = demo_app_data();
    let _session = persist_demo(app, &data);

    app.data = data;
    app.session = None;
    app.parent_unlocked = false;
    app.save_enabled = true;
    app.lock_mode = LockMode::Story;
    app.pending_story = None;
    app.story_selections.clear();
    app.display_order = stable_display_order();
    app.show_settings = false;
    app.status = Status::info("Choose your Coffer Story to unlock Cofferly.");
    app.selected_wallet = 1;
    app.child_name_input = "Child 2".to_owned();
    app.starting_balance_input = format_money_input(1_500);
    app.draft.kind = EntryKind::Deduction;
    app.draft.description.clear();
    app.draft.amount.clear();
}

fn prepare_wallet(app: &mut CofferlyApp, show_settings: bool) {
    let data = demo_app_data();
    let session = persist_demo(app, &data);

    app.data = data;
    app.session = Some(session);
    app.parent_unlocked = true;
    app.save_enabled = true;
    app.lock_mode = LockMode::Story;
    app.pending_story = None;
    app.story_selections.clear();
    app.show_settings = show_settings;
    app.confirm_delete_wallet = false;
    app.status = Status::info("Parent mode unlocked.");
    app.selected_wallet = 1;
    app.child_name_input = "Child 2".to_owned();
    app.starting_balance_input = format_money_input(app.data.wallets[1].starting_balance_cents);
    app.draft.kind = EntryKind::Deduction;
    app.draft.description.clear();
    app.draft.amount.clear();
    app.new_child_name_input.clear();
}

fn save_color_image(image: &Arc<ColorImage>, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let [w, h] = image.size;
    let mut rgba = Vec::with_capacity(w * h * 4);
    for pixel in &image.pixels {
        rgba.push(pixel.r());
        rgba.push(pixel.g());
        rgba.push(pixel.b());
        rgba.push(pixel.a());
    }
    image::RgbaImage::from_raw(w as u32, h as u32, rgba)
        .ok_or_else(|| "invalid screenshot buffer".to_owned())?
        .save(path)
        .map_err(|e| e.to_string())
}
