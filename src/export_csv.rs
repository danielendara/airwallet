use std::fs;
use std::path::PathBuf;

use crate::data::{LedgerSort, Wallet};
use crate::money::format_money_input;

/// Write a UTF-8 CSV ledger (with BOM for Excel) to `path`.
/// Amounts are unformatted decimals so spreadsheets can sum them.
pub fn write_csv_ledger(path: &PathBuf, wallets: &[Wallet]) -> Result<PathBuf, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    let mut csv = String::from('\u{feff}');
    csv.push_str("Wallet,Date,Description,Amount,Balance\r\n");

    for wallet in wallets {
        for row in wallet.ledger_rows_sorted(LedgerSort::OldestFirst) {
            csv.push_str(&csv_text_field(&wallet.child_name));
            csv.push(',');
            csv.push_str(&csv_text_field(&row.date.label()));
            csv.push(',');
            csv.push_str(&csv_text_field(row.description));
            csv.push(',');
            csv.push_str(&csv_number_field(&format_money_input(row.amount_cents)));
            csv.push(',');
            csv.push_str(&csv_number_field(&format_money_input(row.balance_cents)));
            csv.push_str("\r\n");
        }
    }

    fs::write(path, csv).map_err(|err| err.to_string())?;
    Ok(path.clone())
}

/// Quotes a field per RFC 4180. Does not neutralize formula-trigger characters;
/// use `csv_text_field` for any value that may contain free-form user text.
fn csv_number_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        let mut escaped = String::from('"');
        escaped.push_str(&value.replace('"', "\"\""));
        escaped.push('"');
        escaped
    } else {
        value.to_owned()
    }
}

/// Like `csv_number_field`, but also neutralizes CSV/spreadsheet formula injection:
/// values starting with `=`, `+`, `-`, `@`, tab, or CR are prefixed with `'` so
/// Excel/LibreOffice/Sheets treat them as literal text rather than formulas.
fn csv_text_field(value: &str) -> String {
    let needs_formula_guard = value.starts_with(['=', '+', '-', '@', '\t', '\r']);

    if needs_formula_guard {
        let mut guarded = String::from('\'');
        guarded.push_str(value);
        csv_number_field(&guarded)
    } else {
        csv_number_field(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Entry;
    use chrono::NaiveDate;
    use tempfile::tempdir;

    #[test]
    fn writes_csv_with_bom_quotes_and_numeric_amounts() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("ledger.csv");
        let wallets = vec![Wallet {
            child_name: "Alex, Sam".to_owned(),
            starting_balance_cents: 2_000,
            entries: vec![Entry {
                date: NaiveDate::from_ymd_opt(2026, 7, 11).unwrap(),
                description: "Book \"sale\"".to_owned(),
                amount_cents: -750,
            }],
        }];

        let written = write_csv_ledger(&path, &wallets).unwrap();
        let csv = std::fs::read_to_string(&written).unwrap();

        assert_eq!(written, path);
        assert!(csv.starts_with('\u{feff}'));
        assert!(csv.contains("Wallet,Date,Description,Amount,Balance"));
        assert!(csv.contains("\"Alex, Sam\""));
        assert!(csv.contains("Start"));
        assert!(csv.contains("Starting balance"));
        assert!(csv.contains("07/11/2026"));
        assert!(csv.contains("\"Book \"\"sale\"\"\""));
        assert!(csv.contains(",-7.50,"));
        assert!(csv.contains(",12.50\r\n"));
        assert!(!csv.contains('$'));
    }

    #[test]
    fn guards_formula_trigger_characters_in_text_fields_but_not_amounts() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ledger.csv");
        let wallets = vec![Wallet {
            child_name: "=cmd|' /C calc'!A0".to_owned(),
            starting_balance_cents: 0,
            entries: vec![Entry {
                date: NaiveDate::from_ymd_opt(2026, 7, 11).unwrap(),
                description: "+HYPERLINK(\"http://example.com\")".to_owned(),
                amount_cents: -750,
            }],
        }];

        let written = write_csv_ledger(&path, &wallets).unwrap();
        let csv = std::fs::read_to_string(&written).unwrap();

        assert!(csv.contains("'=cmd|' /C calc'!A0"));
        assert!(csv.contains("\"'+HYPERLINK(\"\"http://example.com\"\")\""));
        // Amount/Balance columns legitimately start with '-' and must stay numeric.
        assert!(csv.contains(",-7.50,"));
        assert!(!csv.contains("'-7.50"));
    }

    #[test]
    fn text_field_helper_guards_all_trigger_characters() {
        for trigger in ['=', '+', '-', '@', '\t', '\r'] {
            let value = format!("{trigger}rest");
            let guarded = csv_text_field(&value);
            assert!(
                guarded.starts_with('\'') || guarded.starts_with("\"'"),
                "expected guard prefix for {trigger:?}, got {guarded:?}"
            );
        }
        assert_eq!(csv_text_field("plain text"), "plain text");
    }
}
