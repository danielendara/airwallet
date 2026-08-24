use chrono::Local;
use eframe::egui;
use eframe::egui::Color32;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

mod capture;
mod crypto;
mod data;
mod export_csv;
mod io;
mod money;
mod print_html;
mod story;
mod theme;
mod views;

pub const APP_NAME: &str = "Cofferly";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DATA_FILE_NAME: &str = "vault.cofferly";
const PREVIOUS_DATA_FILE_NAME: &str = "data.json";
const PIN_LENGTH: usize = 4;
const LOCK_SCREEN_IMAGE_BYTES: &[u8] = include_bytes!("../assets/cofferly-lock.jpg");
const OPEN_COFFER_IMAGE_BYTES: &[u8] = include_bytes!("../assets/cofferly-open.png");
/// Forgiving default so parents are not locked mid-chore; still protects a
/// shared family PC left open.
const AUTO_LOCK_AFTER: Duration = Duration::from_secs(10 * 60);
/// Show a quiet countdown for the last two minutes before auto-lock.
const AUTO_LOCK_WARN: Duration = Duration::from_secs(2 * 60);
/// Escalating delays after consecutive wrong PINs. This slows automated UI
/// guessing without creating a permanent lockout for a parent.
const UNLOCK_COOLDOWN_MINUTES: [u64; 6] = [1, 2, 5, 15, 30, 60];
const UI_STATE_KEY: &str = "cofferly/ui_state";

use crypto::SessionCrypto;
use data::{
    default_app_data, format_ledger_date, parse_ledger_date, valid_cents, valid_child_name,
    valid_description, AppData, Entry, EntryKind, LedgerSort, OwnedLedgerRow, Wallet,
};
use export_csv::write_csv_ledger;
use io::{
    cleanup_temp_print_artifacts, data_path, prepare_data_vault, reserve_private_temp_path,
    save_encrypted,
};
use money::{format_money, format_money_input, parse_dollars_to_cents};
use print_html::{ledger_file_stem, write_printable_ledger};
use theme::{app_icon, balance_color, configure_style};

fn main() -> eframe::Result<()> {
    let capturing = std::env::var_os("COFFERLY_CAPTURE").is_some();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(if capturing {
                // Tall enough for Settings sections used in README screenshots.
                [1280.0, 1200.0]
            } else {
                [1080.0, 720.0]
            })
            .with_min_inner_size([820.0, 560.0])
            .with_title(APP_NAME)
            .with_app_id("com.cofferly.app")
            .with_icon(app_icon()),
        // Avoid restoring a previous window size over documentation captures.
        persist_window: !capturing,
        ..Default::default()
    };

    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| Ok(Box::new(CofferlyApp::new(cc)))),
    )
}

#[derive(Debug, Clone)]
struct EntryDraft {
    description: String,
    amount: String,
    kind: EntryKind,
    date_input: String,
}

impl EntryDraft {
    fn new() -> Self {
        Self {
            description: String::new(),
            amount: String::new(),
            kind: EntryKind::Deduction,
            date_input: format_ledger_date(Local::now().date_naive()),
        }
    }
}

/// The entry most recently removed from a wallet, held briefly so the user can
/// undo the deletion. Cleared by any new mutation.
#[derive(Debug, Clone)]
struct RemovableEntry {
    wallet_index: usize,
    entry: Entry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusSeverity {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone)]
pub struct Status {
    pub text: String,
    pub severity: StatusSeverity,
}

impl Status {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            severity: StatusSeverity::Info,
        }
    }

    pub fn success(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            severity: StatusSeverity::Success,
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            severity: StatusSeverity::Error,
        }
    }
}

/// Non-sensitive UI prefs restored via eframe storage (never unlock state or PINs).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct UiState {
    selected_wallet: usize,
    ledger_sort_newest_first: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LockMode {
    SetupReveal,
    SetupConfirm,
    Story,
    LegacyPin,
    MigrateReveal,
    MigrateConfirm,
    ChangeReveal,
    ChangeConfirm,
}

pub(crate) struct CofferlyApp {
    data: AppData,
    raw_bytes: Option<Vec<u8>>,
    /// Present while parent mode is unlocked; enables saves without re-running Argon2id.
    session: Option<SessionCrypto>,
    selected_wallet: usize,
    ledger_sort: LedgerSort,
    /// Cached sorted ledger for the selected wallet; invalidated on mutation / selection / sort.
    ledger_cache: Option<(usize, LedgerSort, Vec<OwnedLedgerRow>)>,
    draft: EntryDraft,
    starting_balance_input: String,
    child_name_input: String,
    new_child_name_input: String,
    pin_digits: [String; PIN_LENGTH],
    pending_pin_focus: Option<usize>,
    lock_mode: LockMode,
    pending_story: Option<[&'static str; story::STORY_LENGTH]>,
    story_selections: Vec<&'static str>,
    display_order: Vec<&'static str>,
    story_icon_textures: HashMap<&'static str, egui::TextureHandle>,
    parent_unlocked: bool,
    save_enabled: bool,
    /// True for the launch that copied `data.json`; keeps the recovery reminder
    /// visible after the parent proves the new vault can be decrypted.
    previous_data_backup_preserved: bool,
    status: Status,
    data_path: PathBuf,
    lock_screen_image: Option<egui::TextureHandle>,
    lock_screen_bg: egui::Color32,
    open_coffer_image: Option<egui::TextureHandle>,
    show_settings: bool,
    confirm_delete_wallet: bool,
    undo: Option<RemovableEntry>,
    last_interaction: Instant,
    /// True while Argon2id / decrypt runs off the UI thread.
    unlocking: bool,
    unlock_rx: Option<std::sync::mpsc::Receiver<UnlockResult>>,
    failed_unlock_attempts: u32,
    unlock_cooldown_until: Option<Instant>,
    /// Maintainer-only README capture sequence (`COFFERLY_CAPTURE`).
    capture: Option<capture::CaptureSession>,
    /// Paths of temp exports/recovery cards written this session, so they can
    /// be deleted on lock/exit instead of lingering until the next launch.
    temp_artifact_paths: Vec<PathBuf>,
}

struct UnlockResult {
    outcome: Result<(AppData, SessionCrypto), String>,
}

impl CofferlyApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_style(&cc.egui_ctx);
        cleanup_temp_print_artifacts();

        let data_path = data_path();
        let (preserved_previous_file, preparation_error) = match prepare_data_vault() {
            Ok(preparation) => (preparation.preserved_previous_file, None),
            Err(err) => (None, Some(err)),
        };
        let previous_data_backup_preserved = preserved_previous_file.is_some();
        let (raw_bytes, storage_error) = if let Some(err) = preparation_error {
            (None, Some(err))
        } else {
            match io::load_raw(&data_path) {
                Ok(raw_bytes) => (raw_bytes, None),
                Err(err) => (None, Some(err)),
            }
        };

        let (save_enabled, lock_mode, status) = if let Some(err) = storage_error {
            (
                false,
                LockMode::Story,
                Status::error(format!(
                    "Could not prepare saved data: {err}. Changes are disabled."
                )),
            )
        } else if let Some(bytes) = &raw_bytes {
            if crypto::is_current_format(bytes) {
                let mode = if bytes.first() == Some(&crypto::LEGACY_PIN_VERSION) {
                    LockMode::LegacyPin
                } else {
                    LockMode::Story
                };
                let message = if previous_data_backup_preserved {
                    if mode == LockMode::LegacyPin {
                        "Copied encrypted data into vault.cofferly. The original data.json remains untouched as a backup. Enter the legacy PIN to enroll a Coffer Story."
                    } else {
                        "Copied encrypted data into vault.cofferly. The original data.json remains untouched as a backup. Choose your Coffer Story to verify the vault."
                    }
                } else if mode == LockMode::LegacyPin {
                    "Enter the legacy 4-digit PIN to migrate to Coffer Story."
                } else {
                    "Choose your Coffer Story to unlock Cofferly."
                };
                (true, mode, Status::info(message))
            } else {
                (
                    false,
                    LockMode::Story,
                    Status::error(
                        "Saved data uses an unsupported format. Move the file aside to start fresh, or restore a current encrypted backup. Changes are disabled.",
                    ),
                )
            }
        } else {
            (
                true,
                LockMode::SetupReveal,
                Status::info("Cofferly created a six-object Coffer Story for you."),
            )
        };
        let data = default_app_data();

        let (selected_wallet, ledger_sort) = restore_ui_state(cc, data.wallets.len());
        let (lock_screen_image, lock_screen_bg) = load_lock_screen_image(&cc.egui_ctx);
        let open_coffer_image = load_open_coffer_image(&cc.egui_ctx);
        let story_icon_textures = load_story_icon_textures(&cc.egui_ctx);

        Self {
            data,
            raw_bytes,
            session: None,
            selected_wallet,
            ledger_sort,
            ledger_cache: None,
            draft: EntryDraft::new(),
            starting_balance_input: String::new(),
            child_name_input: String::new(),
            new_child_name_input: String::new(),
            pin_digits: Default::default(),
            pending_pin_focus: Some(0),
            lock_mode,
            pending_story: story::generate().ok(),
            story_selections: Vec::new(),
            display_order: story::shuffled_catalog()
                .unwrap_or_else(|_| story::CATALOG.iter().map(|(id, _)| *id).collect()),
            story_icon_textures,
            parent_unlocked: false,
            save_enabled,
            previous_data_backup_preserved,
            status,
            data_path,
            lock_screen_image,
            lock_screen_bg,
            open_coffer_image,
            show_settings: false,
            confirm_delete_wallet: false,
            undo: None,
            last_interaction: Instant::now(),
            unlocking: false,
            unlock_rx: None,
            failed_unlock_attempts: 0,
            unlock_cooldown_until: None,
            capture: capture::CaptureSession::from_env(),
            temp_artifact_paths: Vec::new(),
        }
    }

    fn set_status_info(&mut self, text: impl Into<String>) {
        self.status = Status::info(text);
    }

    fn set_status_ok(&mut self, text: impl Into<String>) {
        self.status = Status::success(text);
    }

    fn set_status_err(&mut self, text: impl Into<String>) {
        self.status = Status::error(text);
    }

    fn invalidate_ledger_cache(&mut self) {
        self.ledger_cache = None;
    }

    fn cached_ledger_rows(&mut self) -> &[OwnedLedgerRow] {
        let wallet_index = self.selected_wallet;
        let sort = self.ledger_sort;
        let needs_rebuild = match &self.ledger_cache {
            Some((idx, cached_sort, _)) => *idx != wallet_index || *cached_sort != sort,
            None => true,
        };

        if needs_rebuild {
            let rows = self.data.wallets[wallet_index].ledger_rows_sorted_owned(sort);
            self.ledger_cache = Some((wallet_index, sort, rows));
        }

        &self.ledger_cache.as_ref().unwrap().2
    }

    fn selected_wallet(&self) -> &Wallet {
        &self.data.wallets[self.selected_wallet]
    }

    fn selected_wallet_mut(&mut self) -> &mut Wallet {
        &mut self.data.wallets[self.selected_wallet]
    }

    /// Start PIN verification. Heavy Argon2id work runs on a background thread so
    /// the window stays responsive; results are applied in [`Self::poll_unlock`].
    fn start_unlock(&mut self) {
        if self.unlocking {
            return;
        }

        if let Some(remaining) = self.unlock_cooldown_remaining() {
            self.clear_pin_digits();
            self.set_status_err(format!(
                "Too many wrong PIN attempts. Try again in {}.",
                format_cooldown(remaining)
            ));
            return;
        }

        if !self.save_enabled {
            self.clear_pin_digits();
            self.set_status_err(
                "Cannot unlock while the saved data file is unreadable or unsupported.",
            );
            return;
        }

        let entered = self.entered_parent_pin();
        if entered.len() != PIN_LENGTH {
            self.set_status_err("Enter all 4 digits of the parent PIN.");
            return;
        }

        // Background path only for encrypted blobs (Argon2id is expensive).
        if let Some(raw) = &self.raw_bytes {
            if crypto::is_current_format(raw) {
                let raw = raw.clone();
                let pin = entered;
                let (tx, rx) = std::sync::mpsc::channel();
                self.unlock_rx = Some(rx);
                self.unlocking = true;
                self.set_status_info("Unlocking…");
                std::thread::spawn(move || {
                    let outcome = match crypto::decrypt(&raw, &pin) {
                        Ok((plain, session)) => match serde_json::from_slice::<AppData>(&plain) {
                            Ok(loaded) => match data::normalize_app_data(loaded) {
                                Some(normalized) => Ok((normalized, session)),
                                None => Err("Saved data is invalid after decryption.".to_string()),
                            },
                            Err(err) => Err(format!("Could not parse decrypted data: {err}")),
                        },
                        Err(_) => Err("Wrong PIN or data has been tampered with.".to_string()),
                    };
                    let _ = tx.send(UnlockResult { outcome });
                });
                return;
            }
        }

        // A clean first run does not need Argon2 yet, so stay on the UI thread.
        self.unlock_parent_sync();
    }

    /// Synchronous unlock for first-run paths and unit tests.
    fn unlock_parent_sync(&mut self) {
        let entered = self.entered_parent_pin();

        if let Some(raw) = &self.raw_bytes {
            if !crypto::is_current_format(raw) {
                self.clear_pin_digits();
                self.session = None;
                self.set_status_err(
                    "Cannot unlock while the saved data file is unreadable or unsupported.",
                );
                return;
            }

            match crypto::decrypt(raw, &entered) {
                Ok((plain, session)) => {
                    if let Ok(loaded) = serde_json::from_slice::<AppData>(&plain) {
                        if let Some(normalized) = data::normalize_app_data(loaded) {
                            self.apply_unlock(normalized, session);
                            return;
                        }
                    }
                    self.clear_pin_digits();
                    self.session = None;
                    self.register_unlock_failure(
                        "Wrong credential or data has been tampered with.",
                    );
                    return;
                }
                Err(_) => {
                    self.clear_pin_digits();
                    self.session = None;
                    self.register_unlock_failure(
                        "Wrong credential or data has been tampered with.",
                    );
                    return;
                }
            }
        }

        // First run: establish a session when the parent first saves, but unlock now.
        if entered == self.data.parent_pin {
            self.parent_unlocked = true;
            self.clear_pin_digits();
            self.invalidate_ledger_cache();
            self.touch_interaction();
            self.reset_pin_failures();
            self.set_status_ok("Parent mode unlocked.");
        } else {
            self.clear_pin_digits();
            self.register_unlock_failure("Wrong PIN.");
        }
    }

    fn apply_unlock(&mut self, data: AppData, session: SessionCrypto) {
        // Clamp selection against the loaded wallet count.
        if self.selected_wallet >= data.wallets.len() {
            self.selected_wallet = 0;
        }
        self.data = data;
        let legacy = session.version() == crypto::LEGACY_PIN_VERSION;
        self.session = Some(session);
        self.parent_unlocked = !legacy;
        self.clear_pin_digits();
        self.reset_story_entry();
        self.invalidate_ledger_cache();
        self.touch_interaction();
        self.reset_pin_failures();
        if legacy {
            self.lock_mode = LockMode::MigrateReveal;
            self.pending_story = story::generate().ok();
            self.set_status_info(
                "Legacy PIN accepted. Enroll your Coffer Story to finish migration.",
            );
        } else {
            self.set_status_ok(if self.previous_data_backup_preserved {
                "Coffer Story unlocked. Verify your wallets before removing the data.json backup."
            } else {
                "Coffer Story unlocked."
            });
        }
    }

    fn poll_unlock(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.unlock_rx else {
            return;
        };

        match rx.try_recv() {
            Ok(result) => {
                self.unlocking = false;
                self.unlock_rx = None;
                match result.outcome {
                    Ok((data, session)) => self.apply_unlock(data, session),
                    Err(err) => {
                        self.clear_pin_digits();
                        self.reset_story_entry();
                        self.session = None;
                        self.register_unlock_failure(&err);
                    }
                }
                ctx.request_repaint();
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint();
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.unlocking = false;
                self.unlock_rx = None;
                self.set_status_err("Unlock failed unexpectedly. Try again.");
            }
        }
    }

    fn lock_parent(&mut self) {
        self.parent_unlocked = false;
        self.session = None;
        self.show_settings = false;
        self.confirm_delete_wallet = false;
        self.clear_pin_digits();
        self.cleanup_temp_artifacts();
        self.set_status_info("Locked. Enter the parent PIN to make changes.");
    }

    /// Tracks a temp export/recovery-card path so it can be deleted on lock/exit.
    fn track_temp_artifact(&mut self, path: PathBuf) {
        self.temp_artifact_paths.push(path);
    }

    /// Deletes every tracked temp artifact. Best-effort: a file already opened
    /// by another app may still be in use, and that's fine to leave for the
    /// next launch's `cleanup_temp_print_artifacts` sweep.
    fn cleanup_temp_artifacts(&mut self) {
        for path in self.temp_artifact_paths.drain(..) {
            let _ = std::fs::remove_file(path);
        }
    }

    fn auto_lock_if_idle(&mut self, ctx: &egui::Context) {
        if !self.parent_unlocked {
            return;
        }

        let idle = self.last_interaction.elapsed();
        if idle >= AUTO_LOCK_AFTER {
            self.lock_parent();
            self.set_status_info("Locked automatically after inactivity.");
            return;
        }

        let remaining = AUTO_LOCK_AFTER.saturating_sub(idle);
        if remaining <= AUTO_LOCK_WARN {
            ctx.request_repaint_after(Duration::from_secs(1));
        } else {
            ctx.request_repaint_after(remaining.saturating_sub(AUTO_LOCK_WARN));
        }
    }

    fn auto_lock_remaining(&self) -> Option<Duration> {
        if !self.parent_unlocked {
            return None;
        }
        Some(AUTO_LOCK_AFTER.saturating_sub(self.last_interaction.elapsed()))
    }

    fn touch_interaction(&mut self) {
        self.last_interaction = Instant::now();
    }

    fn unlock_cooldown_remaining(&self) -> Option<Duration> {
        self.unlock_cooldown_until
            .and_then(|until| until.checked_duration_since(Instant::now()))
            .filter(|remaining| !remaining.is_zero())
    }

    fn register_unlock_failure(&mut self, message: &str) {
        self.failed_unlock_attempts = self.failed_unlock_attempts.saturating_add(1);
        let cooldown = unlock_cooldown_duration(self.failed_unlock_attempts);
        if cooldown.is_zero() {
            self.unlock_cooldown_until = None;
            self.set_status_err(message.to_owned());
            return;
        }
        self.unlock_cooldown_until = Some(Instant::now() + cooldown);
        self.set_status_err(format!(
            "{message} Try again in {}.",
            format_cooldown(cooldown)
        ));
    }

    fn reset_pin_failures(&mut self) {
        self.failed_unlock_attempts = 0;
        self.unlock_cooldown_until = None;
    }

    fn reset_story_entry(&mut self) {
        self.story_selections.clear();
        if let Ok(order) = story::shuffled_catalog() {
            self.display_order = order;
        }
    }

    fn regenerate_story(&mut self) {
        match story::generate() {
            Ok(story) => {
                self.pending_story = Some(story);
            }
            Err(err) => self.set_status_err(err),
        }
    }

    pub(crate) fn begin_story_change(&mut self) {
        if !self.can_change("Unlock parent mode before changing the Coffer Story.") {
            return;
        }
        match story::generate() {
            Ok(story) => {
                self.pending_story = Some(story);
                self.reset_story_entry();
                self.parent_unlocked = false;
                self.show_settings = false;
                self.lock_mode = LockMode::ChangeReveal;
                self.set_status_info("Cofferly generated a replacement Coffer Story.");
            }
            Err(err) => self.set_status_err(err),
        }
    }

    /// Cancels a Coffer Story change and restores the already-unlocked parent
    /// mode. No crypto work is needed: the session was never rewrapped and
    /// the vault file was never touched until a successful confirm.
    pub(crate) fn cancel_story_change(&mut self) {
        self.pending_story = None;
        self.reset_story_entry();
        self.lock_mode = LockMode::Story;
        self.parent_unlocked = true;
        self.set_status_info("Coffer Story unchanged.");
    }

    /// Cancels a legacy-PIN-to-Coffer-Story migration entirely, dropping the
    /// in-memory session and returning to the PIN screen. Mirrors what the
    /// failed-save path in `confirm_story_setup` already does.
    pub(crate) fn cancel_story_migration(&mut self) {
        self.session = None;
        self.pending_story = None;
        self.clear_pin_digits();
        self.reset_story_entry();
        self.lock_mode = LockMode::LegacyPin;
        self.set_status_info("Migration canceled. Enter the legacy PIN to unlock.");
    }

    /// Returns from a confirm step to the matching reveal step so the parent
    /// can look at the story again. The pending story stays in memory by
    /// design until a successful (or canceled) confirm.
    pub(crate) fn back_to_story_reveal(&mut self) {
        self.lock_mode = match self.lock_mode {
            LockMode::SetupConfirm => LockMode::SetupReveal,
            LockMode::MigrateConfirm => LockMode::MigrateReveal,
            LockMode::ChangeConfirm => LockMode::ChangeReveal,
            other => other,
        };
        self.reset_story_entry();
        self.set_status_info("Here is your Coffer Story again.");
    }

    pub(crate) fn print_recovery_card(&mut self) {
        let Some(story) = self.pending_story else {
            self.set_status_err("No Coffer Story is available to print.");
            return;
        };
        let items = story
            .iter()
            .enumerate()
            .map(|(index, id)| {
                format!(
                    "<li><strong>{}</strong> — {}</li>",
                    index + 1,
                    story::label(id).unwrap_or(id)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let html = format!(
            "<!doctype html><meta charset=\"utf-8\"><title>Cofferly recovery card</title><h1>Cofferly recovery card</h1><p>This six-object Coffer Story unlocks your encrypted ledger. Store this card away from the computer and children. Without it, recovery is impossible.</p><ol>{items}</ol>"
        );
        let path = match reserve_private_temp_path("recovery-card", "html") {
            Ok(path) => path,
            Err(err) => {
                self.set_status_err(format!("Could not create recovery card: {err}"));
                return;
            }
        };
        match std::fs::write(&path, html)
            .and_then(|_| opener::open(&path).map_err(std::io::Error::other))
        {
            Ok(()) => {
                self.track_temp_artifact(path);
                self.set_status_ok("Opened recovery card. Store the printed copy safely.");
            }
            Err(err) => {
                let _ = std::fs::remove_file(&path);
                self.set_status_err(format!("Could not create recovery card: {err}"));
            }
        }
    }

    fn confirm_story_setup(&mut self) {
        let Some(story) = self.pending_story else {
            self.set_status_err("Could not create a Coffer Story.");
            return;
        };
        let Ok(secret) = story::encode(&story) else {
            self.set_status_err("Could not encode Coffer Story.");
            return;
        };
        match self.lock_mode {
            LockMode::SetupConfirm => {
                self.session = SessionCrypto::establish(&secret).ok();
                if self.session.is_none() {
                    self.set_status_err("Could not secure the new Coffer Story.");
                    return;
                }
                if let Err(err) = self.save_encrypted_data_and_refresh_ref(&secret) {
                    self.session = None;
                    self.set_status_err(format!("Could not save new Coffer Story: {err}"));
                    return;
                }
                self.parent_unlocked = true;
                self.lock_mode = LockMode::Story;
                self.pending_story = None;
                self.reset_story_entry();
                self.reset_pin_failures();
                self.set_status_ok("Coffer Story saved. Parent mode unlocked.");
            }
            LockMode::MigrateConfirm | LockMode::ChangeConfirm => {
                let Some(session) = &mut self.session else {
                    self.set_status_err(
                        "Your unlock session expired. Unlock Cofferly again to retry.",
                    );
                    return;
                };
                if let Err(err) = session.rewrap_for_secret(&secret) {
                    self.set_status_err(format!("Could not prepare migration: {err}"));
                    return;
                }
                if let Err(err) = self.save_encrypted_data_and_refresh_ref(&secret) {
                    // The atomic write left the v2 bytes untouched. Discard the
                    // rewrapped in-memory session too, so retrying always starts
                    // from the recoverable legacy envelope.
                    self.session = None;
                    self.lock_mode = if self.lock_mode == LockMode::MigrateConfirm {
                        LockMode::LegacyPin
                    } else {
                        LockMode::Story
                    };
                    self.clear_pin_digits();
                    self.reset_story_entry();
                    self.set_status_err(format!(
                        "Could not save the new Coffer Story: {err}. The existing encrypted file is unchanged; unlock it again to retry."
                    ));
                    return;
                }
                let was_migration = self.lock_mode == LockMode::MigrateConfirm;
                self.data.parent_pin.clear();
                self.parent_unlocked = true;
                self.lock_mode = LockMode::Story;
                self.pending_story = None;
                self.reset_story_entry();
                self.reset_pin_failures();
                self.set_status_ok(if was_migration {
                    "Coffer Story enrolled. Legacy PIN no longer unlocks this file."
                } else {
                    "Coffer Story changed. The previous story no longer unlocks this file."
                });
            }
            _ => {}
        }
    }

    fn submit_story(&mut self) {
        if self.story_selections.len() != story::STORY_LENGTH {
            return;
        }
        if let Some(remaining) = self.unlock_cooldown_remaining() {
            self.reset_story_entry();
            self.set_status_err(format!(
                "Too many wrong attempts. Try again in {}.",
                format_cooldown(remaining)
            ));
            return;
        }
        match self.lock_mode {
            LockMode::SetupConfirm | LockMode::MigrateConfirm | LockMode::ChangeConfirm => {
                if self
                    .pending_story
                    .as_ref()
                    .is_some_and(|expected| expected.as_slice() == self.story_selections.as_slice())
                {
                    self.confirm_story_setup();
                } else {
                    // A mismatch here can't be an attacker guessing — the
                    // expected story was just shown one screen back — so this
                    // never touches the wrong-credential cooldown.
                    self.reset_story_entry();
                    self.set_status_err("That didn't match. Try selecting it again.");
                }
            }
            LockMode::Story => {
                let Ok(secret) = story::encode(&self.story_selections) else {
                    self.reset_story_entry();
                    self.register_unlock_failure("Invalid Coffer Story.");
                    return;
                };
                let Some(raw) = self.raw_bytes.clone() else {
                    return;
                };
                self.unlocking = true;
                self.set_status_info("Unlocking…");
                let (tx, rx) = std::sync::mpsc::channel();
                self.unlock_rx = Some(rx);
                std::thread::spawn(move || {
                    let outcome = match crypto::decrypt(&raw, &secret) {
                        Ok((plain, session)) => match serde_json::from_slice::<AppData>(&plain) {
                            Ok(data) => data::normalize_app_data(data)
                                .map(|data| (data, session))
                                .ok_or_else(|| {
                                    "Saved data is invalid after decryption.".to_owned()
                                }),
                            Err(_) => Err("Saved data is invalid after decryption.".to_owned()),
                        },
                        Err(_) => {
                            Err("Wrong Coffer Story or data has been tampered with.".to_owned())
                        }
                    };
                    let _ = tx.send(UnlockResult { outcome });
                });
            }
            _ => {}
        }
    }

    pub(crate) fn select_story_object(&mut self, id: &'static str) {
        if self.unlocking
            || self.unlock_cooldown_remaining().is_some()
            || self.story_selections.len() >= story::STORY_LENGTH
        {
            return;
        }
        self.story_selections.push(id);
        if self.story_selections.len() == story::STORY_LENGTH {
            self.submit_story();
        }
    }

    /// Undoes the most recent pick. Submission fires automatically at the
    /// sixth pick, so this is only ever reachable with up to five selected.
    pub(crate) fn remove_last_story_selection(&mut self) {
        if self.unlocking || self.unlock_cooldown_remaining().is_some() {
            return;
        }
        self.story_selections.pop();
    }

    fn note_input_activity(&mut self, ctx: &egui::Context) {
        let has_events = ctx.input(|i| !i.events.is_empty());
        if has_events {
            self.touch_interaction();
        }
    }

    fn entered_parent_pin(&self) -> String {
        self.pin_digits.concat()
    }

    fn clear_pin_digits(&mut self) {
        for digit in &mut self.pin_digits {
            digit.clear();
        }
        self.pending_pin_focus = Some(0);
    }

    fn parent_pin_complete(&self) -> bool {
        self.pin_digits.iter().all(|digit| digit.len() == 1)
    }

    fn normalize_pin_digit_input(&mut self, index: usize) {
        let digits: Vec<char> = self.pin_digits[index]
            .chars()
            .filter(char::is_ascii_digit)
            .collect();

        if digits.is_empty() {
            self.pin_digits[index].clear();
            self.pending_pin_focus = Some(index);
            return;
        }

        if digits.len() == 1 {
            self.pin_digits[index] = digits[0].to_string();
            if index + 1 < PIN_LENGTH {
                self.pending_pin_focus = Some(index + 1);
            }
            return;
        }

        let mut last_filled = index;
        for (offset, digit) in digits.into_iter().enumerate() {
            let target = index + offset;
            if target >= PIN_LENGTH {
                break;
            }

            self.pin_digits[target] = digit.to_string();
            last_filled = target;
        }

        self.pending_pin_focus = Some((last_filled + 1).min(PIN_LENGTH - 1));
    }

    fn add_entry(&mut self) {
        if !self.can_change("Unlock parent mode before adding entries.") {
            return;
        }
        self.undo = None;
        self.confirm_delete_wallet = false;

        let amount = match parse_dollars_to_cents(&self.draft.amount) {
            Ok(amount) if amount > 0 => amount,
            _ => {
                self.set_status_err("Enter a valid amount, like 10 or 10.50.");
                return;
            }
        };
        if !valid_cents(amount) {
            self.set_status_err("Enter a smaller amount.");
            return;
        }

        let description = self.draft.description.trim().to_owned();
        if !valid_description(&self.draft.description) {
            self.set_status_err("Add a description (1-100 characters).");
            return;
        }

        let date = match parse_ledger_date(&self.draft.date_input) {
            Ok(date) => date,
            Err(err) => {
                self.set_status_err(err);
                return;
            }
        };
        if date > Local::now().date_naive() {
            self.set_status_err("Use today or an earlier date.");
            return;
        }

        let action = match self.draft.kind {
            EntryKind::Deposit => "Added",
            EntryKind::Deduction => "Deducted",
        };
        let signed_amount = match self.draft.kind {
            EntryKind::Deposit => amount,
            EntryKind::Deduction => -amount,
        };

        let wallet_name = self.selected_wallet().child_name.clone();

        self.selected_wallet_mut().entries.push(Entry {
            date,
            description: description.clone(),
            amount_cents: signed_amount,
        });
        if !self.selected_wallet().balances_are_valid() {
            self.selected_wallet_mut().entries.pop();
            self.set_status_err(
                "That entry would put the wallet outside Cofferly's supported range.",
            );
            return;
        }

        let status = format!(
            "{action} {} for {}: {description}.",
            format_money(amount),
            wallet_name
        );

        self.draft.description.clear();
        self.draft.amount.clear();
        self.draft.date_input = format_ledger_date(Local::now().date_naive());
        self.invalidate_ledger_cache();
        self.save_with_success(status);
    }

    fn update_starting_balance(&mut self) {
        if !self.can_change("Unlock parent mode before changing balances.") {
            return;
        }
        self.undo = None;
        self.confirm_delete_wallet = false;

        let Ok(balance) = parse_dollars_to_cents(&self.starting_balance_input) else {
            self.set_status_err("Enter a valid starting balance, like 90 or 90.00.");
            return;
        };
        if !valid_cents(balance) {
            self.set_status_err("Enter a smaller starting balance.");
            return;
        }

        let wallet = self.selected_wallet_mut();
        let previous_balance = wallet.starting_balance_cents;
        wallet.starting_balance_cents = balance;
        if !wallet.balances_are_valid() {
            wallet.starting_balance_cents = previous_balance;
            self.set_status_err(
                "That starting balance would put the wallet outside Cofferly's supported range.",
            );
            return;
        }

        let wallet_name = self.selected_wallet().child_name.clone();
        self.starting_balance_input.clear();
        self.invalidate_ledger_cache();
        self.save_with_success(format!(
            "Updated {} starting balance to {}.",
            wallet_name,
            format_money(balance)
        ));
    }

    fn rename_selected_child(&mut self) {
        if !self.can_change("Unlock parent mode before renaming wallets.") {
            return;
        }
        self.undo = None;
        self.confirm_delete_wallet = false;

        let name = self.child_name_input.trim().to_owned();
        if !valid_child_name(&name) {
            self.set_status_err("Use a child name between 1 and 40 characters.");
            return;
        }

        let previous_child_name = std::mem::take(&mut self.selected_wallet_mut().child_name);
        self.selected_wallet_mut().child_name = name;
        self.child_name_input.clear();
        self.invalidate_ledger_cache();
        self.save_with_success(format!(
            "Renamed {previous_child_name} to {}.",
            self.selected_wallet().child_name
        ));
    }

    fn add_child_wallet(&mut self) {
        if !self.can_change("Unlock parent mode before adding wallets.") {
            return;
        }
        self.undo = None;
        self.confirm_delete_wallet = false;

        let name = self.new_child_name_input.trim().to_owned();
        if !valid_child_name(&name) {
            self.set_status_err("Use a child name between 1 and 40 characters.");
            return;
        }

        self.data.wallets.push(Wallet {
            child_name: name.clone(),
            starting_balance_cents: 0,
            entries: Vec::new(),
        });
        self.selected_wallet = self.data.wallets.len() - 1;
        self.new_child_name_input.clear();
        self.invalidate_ledger_cache();
        self.save_with_success(format!("Added wallet for {name}."));
    }

    fn remove_latest_entry(&mut self) {
        if !self.can_change("Unlock parent mode before removing entries.") {
            return;
        }
        self.confirm_delete_wallet = false;

        let wallet_name = self.selected_wallet().child_name.clone();
        if let Some(entry) = self.selected_wallet_mut().entries.pop() {
            self.undo = Some(RemovableEntry {
                wallet_index: self.selected_wallet,
                entry: entry.clone(),
            });
            self.invalidate_ledger_cache();
            self.save_with_success(format!(
                "Removed latest entry from {}: {} {}. Undo available.",
                wallet_name,
                format_money(entry.amount_cents),
                entry.description
            ));
        } else {
            self.set_status_info("There are no entries to remove.");
        }
    }

    fn undo_remove_entry(&mut self) {
        if !self.can_change("Unlock parent mode before undoing.") {
            return;
        }
        self.confirm_delete_wallet = false;

        let Some(removable) = self.undo.take() else {
            return;
        };

        let Some(wallet) = self.data.wallets.get_mut(removable.wallet_index) else {
            self.set_status_err("Can't undo — that wallet no longer exists.");
            return;
        };

        wallet.entries.push(removable.entry.clone());
        let wallet_name = wallet.child_name.clone();
        self.invalidate_ledger_cache();
        self.save_with_success(format!(
            "Restored entry for {}: {} {}.",
            wallet_name,
            format_money(removable.entry.amount_cents),
            removable.entry.description
        ));
    }

    fn delete_selected_wallet(&mut self) {
        if !self.can_change("Unlock parent mode before deleting wallets.") {
            return;
        }

        if self.data.wallets.len() <= 1 {
            self.set_status_err("Keep at least one wallet.");
            return;
        }

        let wallet_name = self.selected_wallet().child_name.clone();
        let removed_index = self.selected_wallet;
        self.data.wallets.remove(removed_index);
        self.undo = None;
        self.confirm_delete_wallet = false;
        if self.selected_wallet >= self.data.wallets.len() {
            self.selected_wallet = self.data.wallets.len() - 1;
        }
        self.invalidate_ledger_cache();
        self.save_with_success(format!("Deleted wallet for {wallet_name}."));
    }

    fn print_selected_wallet(&mut self) {
        if !self.save_enabled {
            self.set_status_err("Saved data could not be loaded, so printing is disabled.");
            return;
        }

        let Ok(path) = self.print_path(false) else {
            self.set_status_err("Could not create printable ledger: temp file unavailable.");
            return;
        };
        match write_printable_ledger(&path, &[self.selected_wallet().clone()]) {
            Ok(path) => self.open_export_file(path, "printable ledger"),
            Err(err) => self.set_status_err(format!("Could not create printable ledger: {err}")),
        }
    }

    fn print_all_wallets(&mut self) {
        if !self.save_enabled {
            self.set_status_err("Saved data could not be loaded, so printing is disabled.");
            return;
        }

        let Ok(path) = self.print_path(true) else {
            self.set_status_err("Could not create printable ledger: temp file unavailable.");
            return;
        };
        match write_printable_ledger(&path, &self.data.wallets) {
            Ok(path) => self.open_export_file(path, "printable ledger"),
            Err(err) => self.set_status_err(format!("Could not create printable ledger: {err}")),
        }
    }

    fn export_selected_wallet_csv(&mut self) {
        if !self.save_enabled {
            self.set_status_err("Saved data could not be loaded, so export is disabled.");
            return;
        }

        let Ok(path) = self.csv_path(false) else {
            self.set_status_err("Could not create CSV ledger: temp file unavailable.");
            return;
        };
        match write_csv_ledger(&path, &[self.selected_wallet().clone()]) {
            Ok(path) => self.open_export_file(path, "CSV ledger"),
            Err(err) => self.set_status_err(format!("Could not create CSV ledger: {err}")),
        }
    }

    fn export_all_wallets_csv(&mut self) {
        if !self.save_enabled {
            self.set_status_err("Saved data could not be loaded, so export is disabled.");
            return;
        }

        let Ok(path) = self.csv_path(true) else {
            self.set_status_err("Could not create CSV ledger: temp file unavailable.");
            return;
        };
        match write_csv_ledger(&path, &self.data.wallets) {
            Ok(path) => self.open_export_file(path, "CSV ledger"),
            Err(err) => self.set_status_err(format!("Could not create CSV ledger: {err}")),
        }
    }

    fn open_export_file(&mut self, path: PathBuf, kind: &str) {
        self.track_temp_artifact(path.clone());
        match opener::open(&path) {
            Ok(()) => self.set_status_ok(format!("Opened {kind}: {}", path.display())),
            Err(err) => {
                self.set_status_err(format!(
                    "{kind} saved to {}, but could not open it: {err}",
                    path.display()
                ));
            }
        }
    }

    fn print_path(&self, all_wallets: bool) -> Result<PathBuf, String> {
        self.export_temp_path(all_wallets, "html")
    }

    fn csv_path(&self, all_wallets: bool) -> Result<PathBuf, String> {
        self.export_temp_path(all_wallets, "csv")
    }

    fn export_temp_path(&self, all_wallets: bool, ext: &str) -> Result<PathBuf, String> {
        let stem = if all_wallets {
            "ledgers".to_owned()
        } else {
            format!(
                "{}-ledger",
                ledger_file_stem(&self.selected_wallet().child_name)
            )
        };

        // Ephemeral, unpredictably-named location — never store plaintext
        // ledgers next to encrypted data.
        reserve_private_temp_path(&stem, ext)
    }

    fn save_with_success(&mut self, success_status: impl Into<String>) {
        if !self.save_enabled {
            self.set_status_err("Saved data could not be loaded, so changes are disabled.");
            return;
        }

        // Serialize from &self.data without an extra full clone of the tree for the
        // save path: we still need pin ownership, so clone only the pin string.
        let secret = if self.session.is_some() {
            String::new()
        } else {
            self.data.parent_pin.clone()
        };
        let save_result = self.save_encrypted_data_and_refresh_ref(&secret);

        match save_result {
            Ok(()) => self.set_status_ok(success_status),
            Err(err) => self.set_status_err(format!("Could not save: {err}")),
        }
    }

    fn save_encrypted_data_and_refresh_ref(&mut self, pin: &str) -> Result<(), String> {
        let encrypted = save_encrypted(&self.data_path, &self.data, pin, &mut self.session)?;
        self.raw_bytes = Some(encrypted);
        Ok(())
    }

    fn can_change(&mut self, locked_status: &str) -> bool {
        if !self.save_enabled {
            self.set_status_err("Saved data could not be loaded, so changes are disabled.");
            return false;
        }

        if !self.parent_unlocked {
            self.set_status_err(locked_status);
            return false;
        }

        true
    }
}

impl eframe::App for CofferlyApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let state = UiState {
            selected_wallet: self.selected_wallet,
            ledger_sort_newest_first: matches!(self.ledger_sort, LedgerSort::NewestFirst),
        };
        eframe::set_value(storage, UI_STATE_KEY, &state);
    }

    fn on_exit(&mut self) {
        self.cleanup_temp_artifacts();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Capture before auto-lock so demo frames are not interrupted.
        if let Some(mut session) = self.capture.take() {
            session.tick(self, &ctx);
            self.capture = Some(session);
        }
        self.note_input_activity(&ctx);
        self.poll_unlock(&ctx);
        if self.capture.is_none() {
            self.auto_lock_if_idle(&ctx);
        }

        if !self.parent_unlocked {
            self.lock_screen(ui);
            return;
        }

        egui::Panel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .inner_margin(egui::Margin::symmetric(18, 10))
                    .stroke(egui::Stroke::new(1.0, theme::BORDER)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(
                        egui::RichText::new(APP_NAME)
                            .size(24.0)
                            .strong()
                            .color(theme::TEXT_PRIMARY),
                    );
                    ui.add_space(6.0);
                    egui::Frame::new()
                        .fill(theme::ACCENT_LIGHT)
                        .corner_radius(egui::CornerRadius::same(12))
                        .inner_margin(egui::Margin::symmetric(10, 5))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("Parent mode unlocked")
                                        .size(11.0)
                                        .strong()
                                        .color(theme::ACCENT_DARK),
                                );
                                if let Some(remaining) = self.auto_lock_remaining() {
                                    if remaining <= AUTO_LOCK_WARN && !remaining.is_zero() {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "· Locks in {}",
                                                format_cooldown(remaining)
                                            ))
                                            .size(11.0)
                                            .color(theme::TEXT_SECONDARY),
                                        );
                                    }
                                }
                            });
                        });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_sized(
                                [88.0, 36.0],
                                egui::Button::new(
                                    egui::RichText::new("Lock").strong().color(Color32::WHITE),
                                )
                                .fill(theme::ACCENT_DARK)
                                .stroke(egui::Stroke::NONE),
                            )
                            .clicked()
                        {
                            self.lock_parent();
                        }
                        if ui
                            .add_sized([108.0, 36.0], egui::Button::new("Settings"))
                            .clicked()
                        {
                            let wallet = self.selected_wallet();
                            let name = wallet.child_name.clone();
                            let bal = wallet.current_balance_cents();
                            self.child_name_input = name;
                            self.starting_balance_input = format_money_input(bal);
                            self.new_child_name_input.clear();
                            self.confirm_delete_wallet = false;
                            self.show_settings = true;
                        }
                        ui.label(
                            egui::RichText::new("Saved locally")
                                .size(11.0)
                                .color(theme::TEXT_SECONDARY),
                        );
                    });
                });
            });

        egui::Panel::left("wallet_picker")
            .resizable(false)
            .min_size(252.0)
            .max_size(252.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::FAINT_BG)
                    .inner_margin(egui::Margin::same(16))
                    .stroke(egui::Stroke::new(1.0, theme::BORDER)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label(
                            egui::RichText::new("Family wallets")
                                .strong()
                                .size(16.0)
                                .color(theme::TEXT_PRIMARY),
                        );
                        ui.label(
                            egui::RichText::new("Choose a child to view")
                                .size(12.0)
                                .color(theme::TEXT_SECONDARY),
                        );
                        ui.add_space(8.0);

                        for index in 0..self.data.wallets.len() {
                            let selected = self.selected_wallet == index;
                            let child_name = self.data.wallets[index].child_name.clone();
                            let balance = self.data.wallets[index].current_balance_cents();
                            let accessible_label =
                                format!("{}, balance {}", child_name, format_money(balance));

                            let response = ui.add_sized(
                                [220.0, 64.0],
                                egui::Button::selectable(selected, "")
                                    .fill(if selected {
                                        theme::ACCENT
                                    } else {
                                        theme::CARD_BG
                                    })
                                    .stroke(if selected {
                                        egui::Stroke::new(1.0, theme::ACCENT)
                                    } else {
                                        egui::Stroke::new(1.0, theme::BORDER)
                                    }),
                            );

                            response.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::WidgetType::SelectableLabel,
                                    true,
                                    selected,
                                    accessible_label.clone(),
                                )
                            });

                            if response.clicked() {
                                self.selected_wallet = index;
                                self.confirm_delete_wallet = false;
                                self.invalidate_ledger_cache();
                            }

                            let rect = response.rect;
                            let painter = ui.painter_at(rect);

                            let text_color = if selected {
                                Color32::WHITE
                            } else {
                                theme::TEXT_PRIMARY
                            };
                            let balance_color = if selected {
                                Color32::WHITE
                            } else {
                                balance_color(balance)
                            };

                            painter.text(
                                rect.left_top() + egui::vec2(14.0, 12.0),
                                egui::Align2::LEFT_TOP,
                                &child_name,
                                egui::FontId::proportional(15.0),
                                text_color,
                            );

                            painter.text(
                                rect.left_bottom() + egui::vec2(14.0, -12.0),
                                egui::Align2::LEFT_BOTTOM,
                                format_money(balance),
                                egui::FontId::proportional(13.0),
                                balance_color,
                            );
                        }

                        ui.add_space(6.0);

                        if ui
                            .add_sized([220.0, 34.0], egui::Button::new("Print this wallet"))
                            .clicked()
                        {
                            self.print_selected_wallet();
                        }
                        if ui
                            .add_sized([220.0, 34.0], egui::Button::new("Print all wallets"))
                            .clicked()
                        {
                            self.print_all_wallets();
                        }
                        if ui
                            .add_sized([220.0, 34.0], egui::Button::new("Export this wallet CSV"))
                            .clicked()
                        {
                            self.export_selected_wallet_csv();
                        }
                        if ui
                            .add_sized([220.0, 34.0], egui::Button::new("Export all wallets CSV"))
                            .clicked()
                        {
                            self.export_all_wallets_csv();
                        }

                        ui.add_space(12.0);
                        self.entry_form(ui);

                        ui.add_space(10.0);
                        self.status_area(ui);
                    });
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::APP_BG)
                    .inner_margin(egui::Margin::same(22)),
            )
            .show(ui, |ui| {
                self.wallet_header(ui);
                ui.add_space(18.0);

                egui::Frame::new()
                    .fill(theme::CARD_BG)
                    .stroke(egui::Stroke::new(1.0, theme::BORDER))
                    .corner_radius(egui::CornerRadius::same(12))
                    .inner_margin(egui::Margin::same(16))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("Transaction history")
                                .strong()
                                .size(16.0)
                                .color(theme::TEXT_PRIMARY),
                        );
                        ui.label(
                            egui::RichText::new("A clear record of every change")
                                .size(12.0)
                                .color(theme::TEXT_SECONDARY),
                        );
                        ui.add_space(8.0);
                        self.ledger_table(ui);
                    });
            });

        if self.show_settings {
            self.show_settings_window(ui.ctx());
        }
    }
}

impl CofferlyApp {
    fn status_area(&self, ui: &mut egui::Ui) {
        let (fill, text_color, prefix) = match self.status.severity {
            StatusSeverity::Info => (theme::GOLD_LIGHT, theme::TEXT_PRIMARY, ""),
            StatusSeverity::Success => (theme::SUCCESS_LIGHT, theme::ACCENT_DARK, ""),
            StatusSeverity::Error => (theme::ERROR_LIGHT, theme::NEGATIVE, "⚠ "),
        };

        let display = format!("{prefix}{}", self.status.text);

        egui::Frame::new()
            .fill(fill)
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_max_width(200.0);
                ui.label(
                    egui::RichText::new(display)
                        .size(11.0)
                        .strong()
                        .color(text_color),
                );
            });
    }
}

fn restore_ui_state(cc: &eframe::CreationContext<'_>, wallet_count: usize) -> (usize, LedgerSort) {
    let wallet_count = wallet_count.max(1);
    let Some(storage) = cc.storage else {
        return (0, LedgerSort::NewestFirst);
    };
    let Some(state) = eframe::get_value::<UiState>(storage, UI_STATE_KEY) else {
        return (0, LedgerSort::NewestFirst);
    };
    let selected = state.selected_wallet.min(wallet_count.saturating_sub(1));
    let sort = if state.ledger_sort_newest_first {
        LedgerSort::NewestFirst
    } else {
        LedgerSort::OldestFirst
    };
    (selected, sort)
}

pub(crate) fn pin_digit_id(index: usize) -> egui::Id {
    egui::Id::new(("parent_pin_digit", index))
}

/// Free wrong attempts before the cooldown starts. Absorbs an honest misclick
/// without materially changing brute-force math against the ~4×10⁸-story keyspace.
const UNLOCK_GRACE_ATTEMPTS: u32 = 2;

fn unlock_cooldown_duration(failed_attempts: u32) -> Duration {
    if failed_attempts <= UNLOCK_GRACE_ATTEMPTS {
        return Duration::ZERO;
    }
    let index = (failed_attempts - UNLOCK_GRACE_ATTEMPTS - 1)
        .min(UNLOCK_COOLDOWN_MINUTES.len() as u32 - 1) as usize;
    Duration::from_secs(UNLOCK_COOLDOWN_MINUTES[index] * 60)
}

fn format_cooldown(duration: Duration) -> String {
    let seconds = duration
        .as_secs()
        .saturating_add(u64::from(duration.subsec_nanos() > 0))
        .max(1);
    if seconds >= 60 {
        let minutes = seconds.div_ceil(60);
        if minutes == 1 {
            "1 minute".to_owned()
        } else {
            format!("{minutes} minutes")
        }
    } else if seconds == 1 {
        "1 second".to_owned()
    } else {
        format!("{seconds} seconds")
    }
}

fn load_lock_screen_image(ctx: &egui::Context) -> (Option<egui::TextureHandle>, egui::Color32) {
    let dyn_image = match image::load_from_memory(LOCK_SCREEN_IMAGE_BYTES) {
        Ok(img) => img,
        Err(_) => return (None, egui::Color32::from_rgb(232, 227, 223)),
    };
    let rgba = dyn_image.to_rgba8();

    let bg_color = if rgba.width() > 0 && rgba.height() > 0 {
        let p = rgba.get_pixel(0, 0);
        egui::Color32::from_rgb(p[0], p[1], p[2])
    } else {
        egui::Color32::from_rgb(232, 227, 223)
    };

    let size = [rgba.width() as usize, rgba.height() as usize];
    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());

    let texture = ctx.load_texture(
        "cofferly-lock-image",
        color_image,
        egui::TextureOptions::LINEAR,
    );

    (Some(texture), bg_color)
}

fn load_open_coffer_image(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let rgba = image::load_from_memory(OPEN_COFFER_IMAGE_BYTES)
        .ok()?
        .to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());

    Some(ctx.load_texture(
        "cofferly-open-coffer-image",
        color_image,
        egui::TextureOptions::NEAREST,
    ))
}

fn load_story_icon_textures(ctx: &egui::Context) -> HashMap<&'static str, egui::TextureHandle> {
    macro_rules! add_icons {
        ($($id:literal),+ $(,)?) => {{
            let mut textures = HashMap::new();
            $(
                let bytes = include_bytes!(concat!("../assets/story-icons/", $id, ".png"));
                if let Ok(image) = image::load_from_memory(bytes) {
                    let rgba = image.to_rgba8();
                    let size = [rgba.width() as usize, rgba.height() as usize];
                    textures.insert(
                        $id,
                        ctx.load_texture(
                            concat!("coffer-story-icon-", $id),
                            egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
                            egui::TextureOptions::LINEAR,
                        ),
                    );
                }
            )+
            textures
        }};
    }

    add_icons!(
        "acorn", "anchor", "apple", "balloon", "book", "bridge", "candle", "castle", "cat",
        "cloud", "compass", "crown", "diamond", "drum", "feather", "fish", "flower", "fox",
        "globe", "sun", "hammer", "hat", "heart", "house", "key", "kite", "lantern", "leaf",
        "lemon", "map",
    )
}

#[cfg(test)]
mod app_tests {
    use super::*;
    use chrono::NaiveDate;
    use eframe::App as _;
    use tempfile::{tempdir, TempDir};

    fn test_app() -> (CofferlyApp, TempDir) {
        let dir = tempdir().unwrap();
        let app = CofferlyApp {
            data: default_app_data(),
            raw_bytes: None,
            session: None,
            selected_wallet: 0,
            ledger_sort: LedgerSort::NewestFirst,
            ledger_cache: None,
            draft: EntryDraft::new(),
            starting_balance_input: String::new(),
            child_name_input: String::new(),
            new_child_name_input: String::new(),
            pin_digits: Default::default(),
            pending_pin_focus: None,
            lock_mode: LockMode::Story,
            pending_story: None,
            story_selections: Vec::new(),
            display_order: story::CATALOG.iter().map(|(id, _)| *id).collect(),
            story_icon_textures: HashMap::new(),
            parent_unlocked: true,
            save_enabled: true,
            previous_data_backup_preserved: false,
            status: Status::info(String::new()),
            data_path: dir.path().join(DATA_FILE_NAME),
            lock_screen_image: None,
            lock_screen_bg: theme::APP_BG,
            open_coffer_image: None,
            show_settings: false,
            confirm_delete_wallet: false,
            undo: None,
            last_interaction: Instant::now(),
            unlocking: false,
            unlock_rx: None,
            failed_unlock_attempts: 0,
            unlock_cooldown_until: None,
            capture: None,
            temp_artifact_paths: Vec::new(),
        };
        (app, dir)
    }

    fn saved_data(app: &CofferlyApp, pin: &str) -> AppData {
        let raw = std::fs::read(&app.data_path).unwrap();
        assert!(crypto::is_current_format(&raw));
        let (plaintext, _) = crypto::decrypt(&raw, pin).unwrap();
        serde_json::from_slice(&plaintext).unwrap()
    }

    fn test_story() -> [&'static str; story::STORY_LENGTH] {
        ["acorn", "anchor", "apple", "balloon", "book", "bridge"]
    }

    #[test]
    fn confirmed_first_run_story_is_saved_immediately_without_serializing_the_story() {
        let (mut app, _dir) = test_app();
        let selected = test_story();
        app.parent_unlocked = false;
        app.lock_mode = LockMode::SetupConfirm;
        app.pending_story = Some(selected);
        app.story_selections = selected.into();

        app.submit_story();

        let raw = std::fs::read(&app.data_path).unwrap();
        assert_eq!(raw[0], crypto::STORY_VERSION);
        let secret = story::encode(&selected).unwrap();
        let (plain, _) = crypto::decrypt(&raw, &secret).unwrap();
        assert!(!String::from_utf8_lossy(&plain).contains("coffer-story-v1:"));
        assert!(app.parent_unlocked);
    }

    #[test]
    fn all_six_story_selections_remain_visible_while_unlocking() {
        let (mut app, _dir) = test_app();
        let selected = test_story();
        let secret = story::encode(&selected).unwrap();
        let mut session = None;
        app.raw_bytes = Some(
            crypto::encrypt(
                &serde_json::to_vec(&default_app_data()).unwrap(),
                &secret,
                &mut session,
            )
            .unwrap(),
        );
        app.parent_unlocked = false;
        app.lock_mode = LockMode::Story;

        for id in selected {
            app.select_story_object(id);
        }

        assert!(app.unlocking);
        assert_eq!(app.story_selections.as_slice(), selected);
    }

    #[test]
    fn legacy_pin_migration_preserves_data_and_rejects_the_old_pin() {
        let (mut app, _dir) = test_app();
        let mut legacy_data = default_app_data();
        legacy_data.wallets[0].child_name = "Kept through migration".to_owned();
        let mut legacy_session = None;
        let mut raw = crypto::encrypt(
            &serde_json::to_vec(&legacy_data).unwrap(),
            "1234",
            &mut legacy_session,
        )
        .unwrap();
        raw[0] = crypto::LEGACY_PIN_VERSION;
        std::fs::write(&app.data_path, &raw).unwrap();
        app.raw_bytes = Some(raw);
        app.parent_unlocked = false;
        app.lock_mode = LockMode::LegacyPin;
        app.pin_digits = ["1".into(), "2".into(), "3".into(), "4".into()];

        app.unlock_parent_sync();

        assert_eq!(app.lock_mode, LockMode::MigrateReveal);
        assert!(!app.parent_unlocked);
        let selected = test_story();
        app.pending_story = Some(selected);
        app.lock_mode = LockMode::MigrateConfirm;
        app.story_selections = selected.into();
        app.submit_story();

        let migrated = std::fs::read(&app.data_path).unwrap();
        assert_eq!(migrated[0], crypto::STORY_VERSION);
        assert!(crypto::decrypt(&migrated, "1234").is_err());
        let secret = story::encode(&selected).unwrap();
        let (plain, _) = crypto::decrypt(&migrated, &secret).unwrap();
        let loaded = serde_json::from_slice::<AppData>(&plain).unwrap();
        assert_eq!(loaded.wallets[0].child_name, "Kept through migration");
    }

    #[test]
    fn changing_story_rewraps_the_existing_data_key() {
        let (mut app, _dir) = test_app();
        let old_story = test_story();
        let old_secret = story::encode(&old_story).unwrap();
        let mut original_session = None;
        let original = crypto::encrypt(
            &serde_json::to_vec(&default_app_data()).unwrap(),
            &old_secret,
            &mut original_session,
        )
        .unwrap();
        std::fs::write(&app.data_path, &original).unwrap();
        let (_, session) = crypto::decrypt(&original, &old_secret).unwrap();
        app.raw_bytes = Some(original);
        app.session = Some(session);
        app.parent_unlocked = false;
        app.lock_mode = LockMode::ChangeConfirm;
        let replacement = ["crown", "diamond", "drum", "feather", "fish", "flower"];
        app.pending_story = Some(replacement);
        app.story_selections = replacement.into();

        app.submit_story();

        let changed = std::fs::read(&app.data_path).unwrap();
        assert!(crypto::decrypt(&changed, &old_secret).is_err());
        let replacement_secret = story::encode(&replacement).unwrap();
        assert!(crypto::decrypt(&changed, &replacement_secret).is_ok());
        assert!(app.parent_unlocked);
    }

    #[cfg(unix)]
    #[test]
    fn failed_legacy_migration_write_leaves_the_original_v2_file_recoverable() {
        use std::os::unix::fs::PermissionsExt;

        let (mut app, dir) = test_app();
        let mut session = None;
        let mut original = crypto::encrypt(
            &serde_json::to_vec(&default_app_data()).unwrap(),
            "1234",
            &mut session,
        )
        .unwrap();
        original[0] = crypto::LEGACY_PIN_VERSION;
        std::fs::write(&app.data_path, &original).unwrap();
        app.raw_bytes = Some(original.clone());
        app.parent_unlocked = false;
        app.lock_mode = LockMode::LegacyPin;
        app.pin_digits = ["1".into(), "2".into(), "3".into(), "4".into()];
        app.unlock_parent_sync();
        let selected = test_story();
        app.pending_story = Some(selected);
        app.lock_mode = LockMode::MigrateConfirm;
        app.story_selections = selected.into();

        let original_permissions = std::fs::metadata(dir.path()).unwrap().permissions();
        let mut readonly = original_permissions.clone();
        readonly.set_mode(0o500);
        std::fs::set_permissions(dir.path(), readonly).unwrap();
        app.submit_story();
        std::fs::set_permissions(dir.path(), original_permissions).unwrap();

        let after_failure = std::fs::read(&app.data_path).unwrap();
        assert_eq!(after_failure, original);
        assert!(crypto::decrypt(&after_failure, "1234").is_ok());
        assert!(!app.parent_unlocked);
        assert!(app.session.is_none());
        assert_eq!(app.lock_mode, LockMode::LegacyPin);
    }

    #[test]
    fn open_coffer_asset_is_small_valid_and_transparent() {
        let image = image::load_from_memory(OPEN_COFFER_IMAGE_BYTES)
            .expect("open coffer asset should decode")
            .to_rgba8();

        assert!(image.width() <= 512);
        assert!(image.height() <= 512);
        assert_eq!(image.get_pixel(0, 0)[3], 0);
        assert!(image.pixels().any(|pixel| pixel[3] > 0));
    }

    #[test]
    fn pasted_pin_digits_are_distributed_and_non_digits_are_ignored() {
        let (mut app, _dir) = test_app();
        app.pin_digits[1] = "9a87".to_owned();

        app.normalize_pin_digit_input(1);

        assert_eq!(app.pin_digits, ["", "9", "8", "7"]);
        assert_eq!(app.pending_pin_focus, Some(3));
        assert!(!app.parent_pin_complete());

        app.pin_digits[0] = "1".to_owned();
        assert!(app.parent_pin_complete());
        assert_eq!(app.entered_parent_pin(), "1987");
    }

    #[test]
    fn encrypted_unlock_accepts_the_right_pin_and_clears_pin_fields() {
        let (mut app, _dir) = test_app();
        let mut stored = default_app_data();
        stored.wallets[0].child_name = "Encrypted wallet".to_owned();
        let serialized = serde_json::to_vec(&stored).unwrap();
        let mut session = None;
        app.raw_bytes = Some(crypto::encrypt(&serialized, "2468", &mut session).unwrap());
        app.parent_unlocked = false;
        app.session = None;
        app.pin_digits = ["2".into(), "4".into(), "6".into(), "8".into()];

        app.unlock_parent_sync();

        assert!(app.parent_unlocked);
        assert!(app.session.is_some());
        assert_eq!(app.selected_wallet().child_name, "Encrypted wallet");
        assert!(app.pin_digits.iter().all(String::is_empty));
        assert_eq!(app.pending_pin_focus, Some(0));
        assert_eq!(app.status.text, "Coffer Story unlocked.");
        assert_eq!(app.status.severity, StatusSeverity::Success);
    }

    #[test]
    fn encrypted_unlock_rejects_wrong_pin_without_exposing_data() {
        let (mut app, _dir) = test_app();
        let mut stored = default_app_data();
        stored.wallets[0].child_name = "Secret wallet".to_owned();
        let serialized = serde_json::to_vec(&stored).unwrap();
        let mut session = None;
        app.raw_bytes = Some(crypto::encrypt(&serialized, "2468", &mut session).unwrap());
        app.parent_unlocked = false;
        app.session = None;
        app.pin_digits = ["0".into(), "0".into(), "0".into(), "0".into()];

        app.unlock_parent_sync();

        assert!(!app.parent_unlocked);
        assert!(app.session.is_none());
        assert_ne!(app.selected_wallet().child_name, "Secret wallet");
        assert!(app.pin_digits.iter().all(String::is_empty));
        // The first two wrong attempts are a free grace period (#80): no cooldown yet.
        assert_eq!(
            app.status.text,
            "Wrong credential or data has been tampered with."
        );
        assert_eq!(app.status.severity, StatusSeverity::Error);
        assert!(app.unlock_cooldown_remaining().is_none());
    }

    #[test]
    fn first_two_wrong_pin_attempts_are_free_then_cooldown_escalates_and_bounds() {
        assert_eq!(unlock_cooldown_duration(1), Duration::ZERO);
        assert_eq!(unlock_cooldown_duration(2), Duration::ZERO);
        assert_eq!(unlock_cooldown_duration(3), Duration::from_secs(60));
        assert_eq!(unlock_cooldown_duration(4), Duration::from_secs(2 * 60));
        assert_eq!(unlock_cooldown_duration(5), Duration::from_secs(5 * 60));
        assert_eq!(unlock_cooldown_duration(6), Duration::from_secs(15 * 60));
        assert_eq!(unlock_cooldown_duration(7), Duration::from_secs(30 * 60));
        assert_eq!(unlock_cooldown_duration(8), Duration::from_secs(60 * 60));
        assert_eq!(unlock_cooldown_duration(100), Duration::from_secs(60 * 60));
    }

    #[test]
    fn active_pin_cooldown_blocks_even_the_correct_pin() {
        let (mut app, _dir) = test_app();
        app.parent_unlocked = false;
        app.failed_unlock_attempts = 3;
        app.unlock_cooldown_until = Some(Instant::now() + Duration::from_secs(5 * 60));
        app.pin_digits = ["1".into(), "2".into(), "3".into(), "4".into()];

        app.start_unlock();

        assert!(!app.parent_unlocked);
        assert!(app.pin_digits.iter().all(String::is_empty));
        assert!(app.status.text.starts_with("Too many wrong PIN attempts."));
    }

    #[test]
    fn story_confirm_mismatch_never_starts_a_cooldown() {
        let (mut app, _dir) = test_app();
        app.lock_mode = LockMode::SetupConfirm;
        app.pending_story = Some(test_story());
        app.story_selections = ["book", "bridge", "acorn", "anchor", "apple", "balloon"].into();

        app.submit_story();

        assert_eq!(app.lock_mode, LockMode::SetupConfirm);
        assert_eq!(app.failed_unlock_attempts, 0);
        assert!(app.unlock_cooldown_remaining().is_none());
        assert_eq!(
            app.status.text,
            "That didn't match. Try selecting it again."
        );
        assert!(app.story_selections.is_empty());
    }

    #[test]
    fn remove_last_story_selection_undoes_the_most_recent_pick() {
        let (mut app, _dir) = test_app();
        app.select_story_object("acorn");
        app.select_story_object("anchor");

        app.remove_last_story_selection();

        assert_eq!(app.story_selections.as_slice(), ["acorn"]);
    }

    #[test]
    fn cancel_story_change_restores_unlocked_parent_mode_without_touching_the_vault() {
        let (mut app, _dir) = test_app();
        app.lock_mode = LockMode::ChangeConfirm;
        app.parent_unlocked = false;
        app.pending_story = Some(test_story());
        app.story_selections = vec!["acorn"];

        app.cancel_story_change();

        assert_eq!(app.lock_mode, LockMode::Story);
        assert!(app.parent_unlocked);
        assert!(app.pending_story.is_none());
        assert!(app.story_selections.is_empty());
    }

    #[test]
    fn cancel_story_migration_drops_the_session_and_returns_to_legacy_pin() {
        let (mut app, _dir) = test_app();
        let old_story = test_story();
        let secret = story::encode(&old_story).unwrap();
        let mut session = None;
        crypto::encrypt(
            &serde_json::to_vec(&default_app_data()).unwrap(),
            &secret,
            &mut session,
        )
        .unwrap();
        app.lock_mode = LockMode::MigrateConfirm;
        app.session = session;
        app.pending_story = Some(test_story());

        app.cancel_story_migration();

        assert_eq!(app.lock_mode, LockMode::LegacyPin);
        assert!(app.session.is_none());
        assert!(app.pending_story.is_none());
    }

    #[test]
    fn back_to_story_reveal_returns_to_the_matching_reveal_mode() {
        let (mut app, _dir) = test_app();
        for (confirm, reveal) in [
            (LockMode::SetupConfirm, LockMode::SetupReveal),
            (LockMode::MigrateConfirm, LockMode::MigrateReveal),
            (LockMode::ChangeConfirm, LockMode::ChangeReveal),
        ] {
            app.lock_mode = confirm;
            app.story_selections = vec!["acorn"];

            app.back_to_story_reveal();

            assert_eq!(app.lock_mode, reveal);
            assert!(app.story_selections.is_empty());
        }
    }

    #[test]
    fn successful_unlock_resets_pin_cooldown_state() {
        let (mut app, _dir) = test_app();
        app.parent_unlocked = false;
        app.failed_unlock_attempts = 4;
        app.unlock_cooldown_until = None;
        app.pin_digits = ["1".into(), "2".into(), "3".into(), "4".into()];

        app.unlock_parent_sync();

        assert!(app.parent_unlocked);
        assert_eq!(app.failed_unlock_attempts, 0);
        assert!(app.unlock_cooldown_until.is_none());
    }

    #[test]
    fn completed_background_unlock_does_not_rewrite_unchanged_data() {
        let (mut app, _dir) = test_app();
        let stored = default_app_data();
        let serialized = serde_json::to_vec(&stored).unwrap();
        let mut encryption_session = None;
        let encrypted = crypto::encrypt(&serialized, "2468", &mut encryption_session).unwrap();
        std::fs::write(&app.data_path, &encrypted).unwrap();
        let (_, unlock_session) = crypto::decrypt(&encrypted, "2468").unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(UnlockResult {
            outcome: Ok((stored, unlock_session)),
        })
        .unwrap();
        app.unlock_rx = Some(rx);
        app.unlocking = true;
        app.parent_unlocked = false;
        app.previous_data_backup_preserved = true;

        app.poll_unlock(&egui::Context::default());

        assert!(app.parent_unlocked);
        assert_eq!(std::fs::read(&app.data_path).unwrap(), encrypted);
        assert!(app.status.text.contains("data.json backup"));
    }

    #[test]
    fn first_run_unlocks_without_creating_a_file() {
        let (mut app, _dir) = test_app();
        app.parent_unlocked = false;
        app.pin_digits = ["1".into(), "2".into(), "3".into(), "4".into()];

        app.unlock_parent_sync();

        assert!(app.parent_unlocked);
        assert!(!app.data_path.exists());
        assert_eq!(app.status.text, "Parent mode unlocked.");
    }

    #[test]
    fn unsupported_data_cannot_be_unlocked_or_overwritten() {
        let (mut app, _dir) = test_app();
        let unsupported = br#"{"parent_pin":"1234","wallets":[]}"#.to_vec();
        std::fs::write(&app.data_path, &unsupported).unwrap();
        app.raw_bytes = Some(unsupported.clone());
        app.parent_unlocked = false;
        // Exercise the format guard itself even if a caller misclassifies storage as writable.
        app.save_enabled = true;
        app.pin_digits = ["1".into(), "2".into(), "3".into(), "4".into()];

        app.start_unlock();

        assert!(!app.parent_unlocked);
        assert!(app.session.is_none());
        assert_eq!(std::fs::read(&app.data_path).unwrap(), unsupported);
        assert!(app.status.text.contains("Cannot unlock"));
        assert_eq!(app.status.severity, StatusSeverity::Error);
    }

    #[test]
    fn transaction_remove_and_undo_workflow_stays_encrypted() {
        let (mut app, _dir) = test_app();
        app.draft.kind = EntryKind::Deposit;
        app.draft.description = "Weekly allowance".to_owned();
        app.draft.amount = "$10.50".to_owned();

        app.add_entry();

        assert_eq!(app.selected_wallet().current_balance_cents(), 1050);
        assert!(app.draft.description.is_empty());
        assert!(app.status.text.contains("Added $10.50"));
        assert_eq!(app.status.severity, StatusSeverity::Success);
        assert_eq!(saved_data(&app, "1234").wallets[0].entries.len(), 1);
        assert!(app.session.is_some());

        app.remove_latest_entry();
        assert!(app.selected_wallet().entries.is_empty());
        assert!(app.undo.is_some());
        assert!(app.status.text.contains("Undo available"));

        app.undo_remove_entry();
        assert_eq!(app.selected_wallet().current_balance_cents(), 1050);
        assert!(app.undo.is_none());
        assert_eq!(saved_data(&app, "1234").wallets[0].entries.len(), 1);
    }

    #[test]
    fn invalid_transaction_does_not_mutate_or_create_a_file() {
        let (mut app, _dir) = test_app();
        app.draft.kind = EntryKind::Deduction;
        app.draft.description = "Toy".to_owned();
        app.draft.amount = "not money".to_owned();

        app.add_entry();

        assert!(app.selected_wallet().entries.is_empty());
        assert!(!app.data_path.exists());
        assert_eq!(app.status.text, "Enter a valid amount, like 10 or 10.50.");
        assert_eq!(app.status.severity, StatusSeverity::Error);
    }

    #[test]
    fn wallet_management_keeps_at_least_one_wallet() {
        let (mut app, _dir) = test_app();
        app.new_child_name_input = "Sam".to_owned();
        app.add_child_wallet();

        assert_eq!(app.data.wallets.len(), 3);
        assert_eq!(app.selected_wallet().child_name, "Sam");

        app.delete_selected_wallet();
        app.delete_selected_wallet();
        app.delete_selected_wallet();

        assert_eq!(app.data.wallets.len(), 1);
        assert_eq!(app.status.text, "Keep at least one wallet.");
        assert_eq!(saved_data(&app, "1234").wallets.len(), 1);
    }

    #[test]
    fn print_path_uses_temp_directory() {
        let (app, _dir) = test_app();
        let path = app.print_path(true).unwrap();
        assert!(path.starts_with(std::env::temp_dir()));
        assert!(path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("cofferly-"));
        assert!(path.extension().is_some_and(|ext| ext == "html"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_file(&path);

        let csv = app.csv_path(false).unwrap();
        assert!(csv.starts_with(std::env::temp_dir()));
        assert!(csv.extension().is_some_and(|ext| ext == "csv"));
        let _ = std::fs::remove_file(&csv);
    }

    #[test]
    fn print_path_uses_unpredictable_names() {
        let (app, _dir) = test_app();
        let first = app.print_path(true).unwrap();
        let second = app.print_path(true).unwrap();
        assert_ne!(first, second);
        let _ = std::fs::remove_file(&first);
        let _ = std::fs::remove_file(&second);
    }

    #[test]
    fn lock_clears_session_key() {
        let (mut app, _dir) = test_app();
        app.draft.kind = EntryKind::Deposit;
        app.draft.description = "Seed".to_owned();
        app.draft.amount = "1".to_owned();
        app.add_entry();
        assert!(app.session.is_some());

        app.lock_parent();
        assert!(!app.parent_unlocked);
        assert!(app.session.is_none());
    }

    #[test]
    fn lock_deletes_tracked_temp_artifacts() {
        let (mut app, dir) = test_app();
        let artifact = dir.path().join("cofferly-recovery-card-test.html");
        std::fs::write(&artifact, "secret story").unwrap();
        app.track_temp_artifact(artifact.clone());

        app.lock_parent();

        assert!(!artifact.exists());
        assert!(app.temp_artifact_paths.is_empty());
    }

    #[test]
    fn on_exit_deletes_tracked_temp_artifacts() {
        let (mut app, dir) = test_app();
        let artifact = dir.path().join("cofferly-ledger-test.csv");
        std::fs::write(&artifact, "secret ledger").unwrap();
        app.track_temp_artifact(artifact.clone());

        app.on_exit();

        assert!(!artifact.exists());
    }

    #[test]
    fn confirm_story_setup_clears_pending_story_after_success() {
        let (mut app, _dir) = test_app();
        app.lock_mode = LockMode::SetupConfirm;
        let selected = story::generate().unwrap();
        app.pending_story = Some(selected);
        app.story_selections = selected.to_vec();

        app.confirm_story_setup();

        assert!(app.pending_story.is_none());
    }

    #[test]
    fn auto_lock_triggers_after_inactivity_threshold() {
        let (mut app, _dir) = test_app();
        app.parent_unlocked = true;
        app.last_interaction = Instant::now() - AUTO_LOCK_AFTER - Duration::from_secs(1);
        let ctx = egui::Context::default();
        app.auto_lock_if_idle(&ctx);
        assert!(!app.parent_unlocked);
        assert!(app.status.text.contains("inactivity"));
    }

    #[test]
    fn auto_lock_warns_in_the_last_two_minutes() {
        let (mut app, _dir) = test_app();
        app.parent_unlocked = true;
        app.last_interaction = Instant::now() - AUTO_LOCK_AFTER + Duration::from_secs(90);
        let remaining = app.auto_lock_remaining().expect("unlocked");
        assert!(remaining <= AUTO_LOCK_WARN);
        assert!(!remaining.is_zero());
        let ctx = egui::Context::default();
        app.auto_lock_if_idle(&ctx);
        assert!(app.parent_unlocked);
    }

    #[test]
    fn add_entry_can_use_an_earlier_date() {
        let (mut app, _dir) = test_app();
        app.draft.kind = EntryKind::Deposit;
        app.draft.description = "Backdated allowance".to_owned();
        app.draft.amount = "5".to_owned();
        app.draft.date_input = "07/01/2026".to_owned();

        app.add_entry();

        assert_eq!(
            app.selected_wallet().entries[0].date,
            NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()
        );
        assert!(app.status.text.contains("Added $5.00"));
    }

    #[test]
    fn add_entry_rejects_a_future_date() {
        let (mut app, _dir) = test_app();
        app.draft.kind = EntryKind::Deposit;
        app.draft.description = "Tomorrow".to_owned();
        app.draft.amount = "5".to_owned();
        app.draft.date_input = "12/31/2099".to_owned();

        app.add_entry();

        assert!(app.selected_wallet().entries.is_empty());
        assert_eq!(app.status.text, "Use today or an earlier date.");
    }
}
