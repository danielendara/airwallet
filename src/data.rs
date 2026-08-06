use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

pub const DEFAULT_PARENT_PIN: &str = "1234";
pub const DEFAULT_CHILD_NAMES: [&str; 2] = ["Child 1", "Child 2"];
pub const MAX_CHILD_NAME_CHARS: usize = 40;
pub const MAX_ABSOLUTE_CENTS: i64 = 99_999_999_999;
pub const MAX_DESCRIPTION_CHARS: usize = 100;
pub const STARTING_BALANCE_DESCRIPTION: &str = "Starting balance";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppData {
    /// Only populated when deserializing a legacy v2 PIN envelope. New story
    /// payloads never serialize an authentication secret into the ledger.
    #[serde(default, skip_serializing)]
    pub parent_pin: String,
    pub wallets: Vec<Wallet>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    pub child_name: String,
    pub starting_balance_cents: i64,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub date: NaiveDate,
    pub description: String,
    pub amount_cents: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Deposit,
    Deduction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerSort {
    NewestFirst,
    OldestFirst,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerRowDate {
    Start,
    Entry(NaiveDate),
}

impl LedgerRowDate {
    pub fn label(self) -> String {
        match self {
            Self::Start => "Start".to_owned(),
            Self::Entry(date) => date.format("%m/%d/%Y").to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LedgerRow<'a> {
    pub date: LedgerRowDate,
    pub description: &'a str,
    pub amount_cents: i64,
    pub balance_cents: i64,
}

/// Owned ledger row for UI caching (avoids rebuilding borrows every frame).
#[derive(Debug, Clone)]
pub struct OwnedLedgerRow {
    pub date: LedgerRowDate,
    pub description: String,
    pub amount_cents: i64,
    pub balance_cents: i64,
}

impl OwnedLedgerRow {
    pub fn from_borrowed(row: LedgerRow<'_>) -> Self {
        Self {
            date: row.date,
            description: row.description.to_owned(),
            amount_cents: row.amount_cents,
            balance_cents: row.balance_cents,
        }
    }
}

impl LedgerSort {
    pub fn toggle(&mut self) {
        *self = match self {
            Self::NewestFirst => Self::OldestFirst,
            Self::OldestFirst => Self::NewestFirst,
        };
    }
}

impl Wallet {
    pub fn current_balance_cents(&self) -> i64 {
        self.entries
            .iter()
            .fold(self.starting_balance_cents, |balance, entry| {
                clamp_cents(balance.saturating_add(entry.amount_cents))
            })
    }

    pub fn ledger_rows(&self) -> Vec<LedgerRow<'_>> {
        let mut balance = self.starting_balance_cents;
        let mut rows = Vec::with_capacity(self.entries.len() + 1);

        rows.push(LedgerRow {
            date: LedgerRowDate::Start,
            description: STARTING_BALANCE_DESCRIPTION,
            amount_cents: self.starting_balance_cents,
            balance_cents: self.starting_balance_cents,
        });

        for entry in &self.entries {
            balance = clamp_cents(balance.saturating_add(entry.amount_cents));
            rows.push(LedgerRow {
                date: LedgerRowDate::Entry(entry.date),
                description: &entry.description,
                amount_cents: entry.amount_cents,
                balance_cents: balance,
            });
        }

        rows
    }

    pub fn balances_are_valid(&self) -> bool {
        self.checked_running_balances().is_some()
    }

    fn checked_running_balances(&self) -> Option<Vec<i64>> {
        if !valid_cents(self.starting_balance_cents)
            || self
                .entries
                .iter()
                .any(|entry| !valid_cents(entry.amount_cents))
        {
            return None;
        }

        let mut balance = self.starting_balance_cents;
        let mut balances = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            balance = balance.checked_add(entry.amount_cents)?;
            if !valid_cents(balance) {
                return None;
            }
            balances.push(balance);
        }

        Some(balances)
    }

    pub fn ledger_rows_sorted(&self, sort: LedgerSort) -> Vec<LedgerRow<'_>> {
        let mut rows: Vec<_> = self.ledger_rows().into_iter().enumerate().collect();

        rows.sort_by(|(left_index, left_row), (right_index, right_row)| {
            let chronological = compare_ledger_row_dates(left_row.date, right_row.date)
                .then_with(|| left_index.cmp(right_index));

            match sort {
                LedgerSort::NewestFirst => chronological.reverse(),
                LedgerSort::OldestFirst => chronological,
            }
        });

        rows.into_iter().map(|(_, row)| row).collect()
    }

    pub fn ledger_rows_sorted_owned(&self, sort: LedgerSort) -> Vec<OwnedLedgerRow> {
        self.ledger_rows_sorted(sort)
            .into_iter()
            .map(OwnedLedgerRow::from_borrowed)
            .collect()
    }
}

fn compare_ledger_row_dates(left: LedgerRowDate, right: LedgerRowDate) -> Ordering {
    match (left, right) {
        (LedgerRowDate::Start, LedgerRowDate::Start) => Ordering::Equal,
        (LedgerRowDate::Start, LedgerRowDate::Entry(_)) => Ordering::Less,
        (LedgerRowDate::Entry(_), LedgerRowDate::Start) => Ordering::Greater,
        (LedgerRowDate::Entry(left_date), LedgerRowDate::Entry(right_date)) => {
            left_date.cmp(&right_date)
        }
    }
}

pub fn default_app_data() -> AppData {
    AppData {
        parent_pin: DEFAULT_PARENT_PIN.to_owned(),
        wallets: default_wallets(),
    }
}

pub fn default_wallets() -> Vec<Wallet> {
    DEFAULT_CHILD_NAMES
        .iter()
        .map(|name| Wallet {
            child_name: (*name).to_owned(),
            starting_balance_cents: 0,
            entries: Vec::new(),
        })
        .collect()
}

pub fn normalize_app_data(mut data: AppData) -> Option<AppData> {
    if data.wallets.is_empty() {
        return None;
    }

    if data
        .wallets
        .iter()
        .any(|wallet| !wallet.balances_are_valid())
    {
        return None;
    }

    if !valid_pin(&data.parent_pin) {
        data.parent_pin = DEFAULT_PARENT_PIN.to_owned();
    }

    Some(data)
}

pub fn valid_pin(pin: &str) -> bool {
    pin.len() == 4 && pin.chars().all(|character| character.is_ascii_digit())
}

pub fn valid_child_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= MAX_CHILD_NAME_CHARS
}

pub fn valid_cents(cents: i64) -> bool {
    cents.unsigned_abs() <= MAX_ABSOLUTE_CENTS as u64
}

pub fn valid_description(desc: &str) -> bool {
    let trimmed = desc.trim();
    !trimmed.is_empty() && trimmed.chars().count() <= MAX_DESCRIPTION_CHARS
}

fn clamp_cents(cents: i64) -> i64 {
    cents.clamp(-MAX_ABSOLUTE_CENTS, MAX_ABSOLUTE_CENTS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_child_names() {
        assert!(valid_child_name("Child 1"));
        assert!(!valid_child_name(""));
        assert!(!valid_child_name("   "));
        assert!(!valid_child_name(
            "This name is too long for the Cofferly sidebar"
        ));
    }

    #[test]
    fn validates_description_and_amount_boundaries() {
        assert!(valid_description("Allowance"));
        assert!(!valid_description("   "));
        assert!(valid_description(&"a".repeat(MAX_DESCRIPTION_CHARS)));
        assert!(!valid_description(&"a".repeat(MAX_DESCRIPTION_CHARS + 1)));

        assert!(valid_cents(MAX_ABSOLUTE_CENTS));
        assert!(valid_cents(-MAX_ABSOLUTE_CENTS));
        assert!(!valid_cents(MAX_ABSOLUTE_CENTS + 1));
        assert!(!valid_cents(-(MAX_ABSOLUTE_CENTS + 1)));
    }

    #[test]
    fn ledger_sort_toggle_and_date_labels_are_predictable() {
        let mut sort = LedgerSort::NewestFirst;
        sort.toggle();
        assert_eq!(sort, LedgerSort::OldestFirst);
        sort.toggle();
        assert_eq!(sort, LedgerSort::NewestFirst);

        assert_eq!(LedgerRowDate::Start.label(), "Start");
        assert_eq!(
            LedgerRowDate::Entry(NaiveDate::from_ymd_opt(2026, 7, 11).unwrap()).label(),
            "07/11/2026"
        );
    }

    #[test]
    fn rejects_empty_loaded_wallets() {
        let data = AppData {
            parent_pin: "1234".to_owned(),
            wallets: Vec::new(),
        };

        assert!(normalize_app_data(data).is_none());
    }

    #[test]
    fn resets_invalid_loaded_pin() {
        let data = AppData {
            parent_pin: "nope".to_owned(),
            wallets: default_wallets(),
        };

        assert_eq!(
            normalize_app_data(data).unwrap().parent_pin,
            DEFAULT_PARENT_PIN
        );
    }

    #[test]
    fn rejects_loaded_wallets_with_out_of_range_amounts() {
        let data = AppData {
            parent_pin: "1234".to_owned(),
            wallets: vec![Wallet {
                child_name: "Child 1".to_owned(),
                starting_balance_cents: MAX_ABSOLUTE_CENTS + 1,
                entries: Vec::new(),
            }],
        };

        assert!(normalize_app_data(data).is_none());
    }

    #[test]
    fn rejects_loaded_wallets_with_overflowing_running_balances() {
        let data = AppData {
            parent_pin: "1234".to_owned(),
            wallets: vec![Wallet {
                child_name: "Child 1".to_owned(),
                starting_balance_cents: MAX_ABSOLUTE_CENTS,
                entries: vec![Entry {
                    date: NaiveDate::from_ymd_opt(2026, 6, 10).unwrap(),
                    description: "Too much".to_owned(),
                    amount_cents: 1,
                }],
            }],
        };

        assert!(normalize_app_data(data).is_none());
    }

    #[test]
    fn sorts_ledger_rows_newest_first_with_historical_balances() {
        let wallet = Wallet {
            child_name: "Child 1".to_owned(),
            starting_balance_cents: 1000,
            entries: vec![
                Entry {
                    date: NaiveDate::from_ymd_opt(2026, 6, 8).unwrap(),
                    description: "First".to_owned(),
                    amount_cents: 500,
                },
                Entry {
                    date: NaiveDate::from_ymd_opt(2026, 6, 9).unwrap(),
                    description: "Second".to_owned(),
                    amount_cents: -200,
                },
                Entry {
                    date: NaiveDate::from_ymd_opt(2026, 6, 9).unwrap(),
                    description: "Latest".to_owned(),
                    amount_cents: 100,
                },
            ],
        };

        let rows = wallet.ledger_rows_sorted(LedgerSort::NewestFirst);
        let descriptions: Vec<_> = rows.iter().map(|row| row.description).collect();
        let balances: Vec<_> = rows.iter().map(|row| row.balance_cents).collect();

        assert_eq!(
            descriptions,
            ["Latest", "Second", "First", "Starting balance"]
        );
        assert_eq!(balances, [1400, 1300, 1500, 1000]);
    }

    #[test]
    fn sorts_ledger_rows_oldest_first() {
        let wallet = Wallet {
            child_name: "Child 1".to_owned(),
            starting_balance_cents: 0,
            entries: vec![
                Entry {
                    date: NaiveDate::from_ymd_opt(2026, 6, 8).unwrap(),
                    description: "First".to_owned(),
                    amount_cents: 100,
                },
                Entry {
                    date: NaiveDate::from_ymd_opt(2026, 6, 9).unwrap(),
                    description: "Second".to_owned(),
                    amount_cents: 100,
                },
            ],
        };

        let rows = wallet.ledger_rows_sorted(LedgerSort::OldestFirst);
        let descriptions: Vec<_> = rows.iter().map(|row| row.description).collect();

        assert_eq!(descriptions, ["Starting balance", "First", "Second"]);
    }
}
