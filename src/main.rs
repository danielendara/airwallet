use chrono::Local;
use eframe::egui;
use eframe::egui::Color32;
use std::path::PathBuf;
use std::time::{Duration, Instant};

mod crypto;
mod data;
mod io;
mod money;
mod print_html;
mod theme;
mod views;

pub const APP_NAME: &str = "Cofferly";
pub const DATA_FILE_NAME: &str = "data.json";
const PIN_LENGTH: usize = 4;
const LOCK_SCREEN_IMAGE_BYTES: &[u8] = include_bytes!("../assets/cofferly-lock.jpg");
const OPEN_COFFER_IMAGE_BYTES: &[u8] = include_bytes!("../assets/cofferly-open.png");
/// Forgiving default so parents are not locked mid-chore; still protects a
/// shared family PC left open.
const AUTO_LOCK_AFTER: Duration = Duration::from_secs(10 * 60);
const UI_STATE_KEY: &str = "cofferly/ui_state";

use crypto::SessionCrypto;
use data::{
    default_app_data, valid_cents, valid_child_name, valid_description, valid_pin, AppData, Entry,
    EntryKind, LedgerSort, OwnedLedgerRow, Wallet,
};
use io::{cleanup_temp_print_artifacts, data_path, save_encrypted};
use money::{format_money, format_money_input, parse_dollars_to_cents};
use print_html::{ledger_file_stem, write_printable_ledger};
use theme::{app_icon, balance_color, configure_style};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1080.0, 720.0])
            .with_min_inner_size([820.0, 560.0])
            .with_title(APP_NAME)
            .with_app_id("com.cofferly.app")
            .with_icon(app_icon()),
        persist_window: true,
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

struct CofferlyApp {
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
    new_pin_input: String,
    parent_unlocked: bool,
    save_enabled: bool,
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
}

struct UnlockResult {
    outcome: Result<(AppData, SessionCrypto), String>,
}

impl CofferlyApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_style(&cc.egui_ctx);
        cleanup_temp_print_artifacts();

        let data_path = data_path();
        let (raw_bytes, raw_load_error) = match io::load_raw(&data_path) {
            Ok(raw_bytes) => (raw_bytes, None),
            Err(err) => (None, Some(err)),
        };

        let (save_enabled, status) = if let Some(err) = raw_load_error {
            (
                false,
                Status::error(format!(
                    "Could not read saved data: {err}. Changes are disabled."
                )),
            )
        } else if let Some(bytes) = &raw_bytes {
            if crypto::is_current_format(bytes) {
                (
                    true,
                    Status::info("Enter the parent PIN to unlock Cofferly."),
                )
            } else {
                (
                    false,
                    Status::error(
                        "Saved data uses an unsupported format. Move the file aside to start fresh, or restore a current encrypted backup. Changes are disabled.",
                    ),
                )
            }
        } else {
            (
                true,
                Status::info("Enter the parent PIN to unlock Cofferly."),
            )
        };
        let data = default_app_data();

        let (selected_wallet, ledger_sort) = restore_ui_state(cc, data.wallets.len());
        let (lock_screen_image, lock_screen_bg) = load_lock_screen_image(&cc.egui_ctx);
        let open_coffer_image = load_open_coffer_image(&cc.egui_ctx);

        Self {
            data,
            raw_bytes,
            session: None,
            selected_wallet,
            ledger_sort,
            ledger_cache: None,
            draft: EntryDraft {
                description: String::new(),
                amount: String::new(),
                kind: EntryKind::Deduction,
            },
            starting_balance_input: String::new(),
            child_name_input: String::new(),
            new_child_name_input: String::new(),
            pin_digits: Default::default(),
            pending_pin_focus: Some(0),
            new_pin_input: String::new(),
            parent_unlocked: false,
            save_enabled,
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
                    self.set_status_err("Wrong PIN or data has been tampered with.");
                    return;
                }
                Err(_) => {
                    self.clear_pin_digits();
                    self.session = None;
                    self.set_status_err("Wrong PIN or data has been tampered with.");
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
            self.set_status_ok("Parent mode unlocked.");
        } else {
            self.clear_pin_digits();
            self.set_status_err("Wrong PIN. Try again.");
        }
    }

    fn apply_unlock(&mut self, data: AppData, session: SessionCrypto) {
        // Clamp selection against the loaded wallet count.
        if self.selected_wallet >= data.wallets.len() {
            self.selected_wallet = 0;
        }
        self.data = data;
        self.session = Some(session);
        self.parent_unlocked = true;
        self.clear_pin_digits();
        self.invalidate_ledger_cache();
        self.touch_interaction();
        self.set_status_ok("Parent mode unlocked.");
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
                        self.session = None;
                        self.set_status_err(err);
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
        self.set_status_info("Locked. Enter the parent PIN to make changes.");
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
        ctx.request_repaint_after(remaining);
    }

    fn touch_interaction(&mut self) {
        self.last_interaction = Instant::now();
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

    fn update_pin(&mut self) {
        if !self.can_change("Unlock parent mode before changing the PIN.") {
            return;
        }
        self.undo = None;
        self.confirm_delete_wallet = false;

        if !valid_pin(&self.new_pin_input) {
            self.set_status_err("Choose exactly 4 digits for the parent PIN.");
            return;
        }

        let mut updated_data = self.data.clone();
        updated_data.parent_pin = self.new_pin_input.clone();
        let new_pin = updated_data.parent_pin.clone();

        // Re-wrap the existing data key under the new PIN (one Argon2id run).
        if let Some(session) = &mut self.session {
            if let Err(err) = session.rewrap_for_pin(&new_pin) {
                self.set_status_err(format!("Could not update PIN: {err}"));
                return;
            }
        }

        match self.save_encrypted_data_and_refresh(&updated_data, &new_pin) {
            Ok(()) => {
                self.data = updated_data;
                self.new_pin_input.clear();
                self.set_status_ok("Parent PIN updated.");
            }
            Err(err) => self.set_status_err(format!("Could not save: {err}")),
        }
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
            date: Local::now().date_naive(),
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

        match write_printable_ledger(&self.print_path(false), &[self.selected_wallet().clone()]) {
            Ok(path) => self.open_printable_file(&path),
            Err(err) => self.set_status_err(format!("Could not create printable ledger: {err}")),
        }
    }

    fn print_all_wallets(&mut self) {
        if !self.save_enabled {
            self.set_status_err("Saved data could not be loaded, so printing is disabled.");
            return;
        }

        match write_printable_ledger(&self.print_path(true), &self.data.wallets) {
            Ok(path) => self.open_printable_file(&path),
            Err(err) => self.set_status_err(format!("Could not create printable ledger: {err}")),
        }
    }

    fn open_printable_file(&mut self, path: &PathBuf) {
        match opener::open(path) {
            Ok(()) => self.set_status_ok(format!("Opened printable ledger: {}", path.display())),
            Err(err) => {
                self.set_status_err(format!(
                    "Printable ledger saved to {}, but could not open it: {err}",
                    path.display()
                ));
            }
        }
    }

    fn print_path(&self, all_wallets: bool) -> PathBuf {
        let file_name = if all_wallets {
            "cofferly-ledgers.html".to_owned()
        } else {
            format!(
                "cofferly-{}-ledger.html",
                ledger_file_stem(&self.selected_wallet().child_name)
            )
        };

        // Ephemeral location — never store plaintext ledgers next to encrypted data.
        std::env::temp_dir().join(file_name)
    }

    fn save_with_success(&mut self, success_status: impl Into<String>) {
        if !self.save_enabled {
            self.set_status_err("Saved data could not be loaded, so changes are disabled.");
            return;
        }

        // Serialize from &self.data without an extra full clone of the tree for the
        // save path: we still need pin ownership, so clone only the pin string.
        let pin = self.data.parent_pin.clone();
        let save_result = self.save_encrypted_data_and_refresh_ref(&pin);

        match save_result {
            Ok(()) => self.set_status_ok(success_status),
            Err(err) => self.set_status_err(format!("Could not save: {err}")),
        }
    }

    fn save_encrypted_data_and_refresh(&mut self, data: &AppData, pin: &str) -> Result<(), String> {
        let encrypted = save_encrypted(&self.data_path, data, pin, &mut self.session)?;
        self.raw_bytes = Some(encrypted);
        Ok(())
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

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.note_input_activity(&ctx);
        self.poll_unlock(&ctx);
        self.auto_lock_if_idle(&ctx);

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
                            ui.label(
                                egui::RichText::new("Parent mode unlocked")
                                    .size(11.0)
                                    .strong()
                                    .color(theme::ACCENT_DARK),
                            );
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
                            self.new_pin_input.clear();
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

#[cfg(test)]
mod app_tests {
    use super::*;
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
            draft: EntryDraft {
                description: String::new(),
                amount: String::new(),
                kind: EntryKind::Deduction,
            },
            starting_balance_input: String::new(),
            child_name_input: String::new(),
            new_child_name_input: String::new(),
            pin_digits: Default::default(),
            pending_pin_focus: None,
            new_pin_input: String::new(),
            parent_unlocked: true,
            save_enabled: true,
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
        };
        (app, dir)
    }

    fn saved_data(app: &CofferlyApp, pin: &str) -> AppData {
        let raw = std::fs::read(&app.data_path).unwrap();
        assert!(crypto::is_current_format(&raw));
        let (plaintext, _) = crypto::decrypt(&raw, pin).unwrap();
        serde_json::from_slice(&plaintext).unwrap()
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
        assert_eq!(app.status.text, "Parent mode unlocked.");
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
        assert_eq!(app.status.text, "Wrong PIN or data has been tampered with.");
        assert_eq!(app.status.severity, StatusSeverity::Error);
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

        app.poll_unlock(&egui::Context::default());

        assert!(app.parent_unlocked);
        assert_eq!(std::fs::read(&app.data_path).unwrap(), encrypted);
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
    fn changing_pin_reencrypts_data_and_rejects_the_old_pin() {
        let (mut app, _dir) = test_app();
        // Establish a session with an initial save.
        app.draft.kind = EntryKind::Deposit;
        app.draft.description = "Seed".to_owned();
        app.draft.amount = "1".to_owned();
        app.add_entry();
        assert!(app.session.is_some());

        app.new_pin_input = "9876".to_owned();
        app.update_pin();

        assert_eq!(app.data.parent_pin, "9876");
        assert!(app.new_pin_input.is_empty());
        let raw = std::fs::read(&app.data_path).unwrap();
        assert!(crypto::decrypt(&raw, "1234").is_err());
        assert_eq!(saved_data(&app, "9876").parent_pin, "9876");
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
        let path = app.print_path(true);
        assert!(path.starts_with(std::env::temp_dir()));
        assert!(path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("cofferly-"));
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
    fn auto_lock_triggers_after_inactivity_threshold() {
        let (mut app, _dir) = test_app();
        app.parent_unlocked = true;
        app.last_interaction = Instant::now() - AUTO_LOCK_AFTER - Duration::from_secs(1);
        let ctx = egui::Context::default();
        app.auto_lock_if_idle(&ctx);
        assert!(!app.parent_unlocked);
        assert!(app.status.text.contains("inactivity"));
    }
}
