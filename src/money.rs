pub fn parse_dollars_to_cents(input: &str) -> Result<i64, String> {
    let trimmed = input.trim();
    let trimmed = trimmed.strip_prefix('$').unwrap_or(trimmed).trim();
    if trimmed.is_empty() {
        return Err("Enter a dollar amount.".to_owned());
    }

    let (negative, amount) = match trimmed.strip_prefix('-') {
        Some(amount) => (true, amount),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };

    if amount.is_empty() {
        return Err("Enter a dollar amount.".to_owned());
    }

    let (dollars, cents) = match amount.split_once('.') {
        Some((dollars, cents)) => {
            if cents.contains('.') {
                return Err("Use only one decimal point.".to_owned());
            }
            (dollars, cents)
        }
        None => (amount, "0"),
    };

    if dollars.is_empty() && cents.is_empty() {
        return Err("Enter a dollar amount.".to_owned());
    }

    // Ledger/sidebar copy uses grouping (`$1,234.56`, thin spaces). Strip those
    // from the dollar part so a paste round-trips through `format_money`.
    let dollars: String = dollars
        .chars()
        .filter(|character| *character != ',' && !character.is_whitespace())
        .collect();

    if !dollars.chars().all(|character| character.is_ascii_digit())
        || !cents.chars().all(|character| character.is_ascii_digit())
    {
        return Err("Use digits and at most one decimal point.".to_owned());
    }

    let dollars = if dollars.is_empty() {
        0
    } else {
        dollars.parse::<i64>().map_err(|err| err.to_string())?
    };
    let cents = match cents.len() {
        0 => 0,
        1 => cents.parse::<i64>().map_err(|err| err.to_string())? * 10,
        2 => cents.parse::<i64>().map_err(|err| err.to_string())?,
        _ => return Err("Use at most two decimal places.".to_owned()),
    };

    let amount = dollars
        .checked_mul(100)
        .and_then(|dollars| dollars.checked_add(cents))
        .ok_or_else(|| "Amount is too large.".to_owned())?;

    if negative {
        amount
            .checked_neg()
            .ok_or_else(|| "Amount is too large.".to_owned())
    } else {
        Ok(amount)
    }
}

fn group_thousands(mut n: u64) -> String {
    if n < 1000 {
        return n.to_string();
    }

    let mut groups = Vec::new();
    while n >= 1000 {
        groups.push(format!("{:03}", n % 1000));
        n /= 1000;
    }

    let mut grouped = n.to_string();
    for group in groups.into_iter().rev() {
        grouped.push(',');
        grouped.push_str(&group);
    }
    grouped
}

pub fn format_money(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let absolute = cents.unsigned_abs();
    format!(
        "{sign}${}.{:02}",
        group_thousands(absolute / 100),
        absolute % 100
    )
}

pub fn format_money_input(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let absolute = cents.unsigned_abs();
    format!("{sign}{}.{:02}", absolute / 100, absolute % 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_whole_dollars() {
        assert_eq!(parse_dollars_to_cents("10").unwrap(), 1000);
    }

    #[test]
    fn parses_dollars_and_cents() {
        assert_eq!(parse_dollars_to_cents("$10.50").unwrap(), 1050);
    }

    #[test]
    fn parses_flexible_money_inputs() {
        assert_eq!(parse_dollars_to_cents("10.").unwrap(), 1000);
        assert_eq!(parse_dollars_to_cents(".50").unwrap(), 50);
        assert_eq!(parse_dollars_to_cents("$ 10").unwrap(), 1000);
        assert_eq!(parse_dollars_to_cents("-10.50").unwrap(), -1050);
        assert_eq!(parse_dollars_to_cents("-.50").unwrap(), -50);
    }

    #[test]
    fn parses_grouped_thousands_from_displayed_money() {
        assert_eq!(parse_dollars_to_cents("$1,234.56").unwrap(), 123_456);
        assert_eq!(parse_dollars_to_cents("1,000").unwrap(), 100_000);
        assert_eq!(parse_dollars_to_cents("1,000,000.00").unwrap(), 100_000_000);
        assert_eq!(parse_dollars_to_cents("1\u{2009}234.56").unwrap(), 123_456);
        assert_eq!(parse_dollars_to_cents("1\u{202F}234.56").unwrap(), 123_456);
        assert_eq!(
            parse_dollars_to_cents(&format_money(123_456)).unwrap(),
            123_456
        );
        assert_eq!(
            parse_dollars_to_cents(&format_money(100_000_000)).unwrap(),
            100_000_000
        );
    }

    #[test]
    fn rejects_invalid_money_inputs() {
        assert!(parse_dollars_to_cents("").is_err());
        assert!(parse_dollars_to_cents("$").is_err());
        assert!(parse_dollars_to_cents("10.999").is_err());
        assert!(parse_dollars_to_cents("10.1.1").is_err());
        assert!(parse_dollars_to_cents("ten").is_err());
    }

    #[test]
    fn rejects_money_inputs_that_overflow_cents() {
        assert!(parse_dollars_to_cents("92233720368547758.08").is_err());
    }

    #[test]
    fn formats_money() {
        assert_eq!(format_money(19400), "$194.00");
        assert_eq!(format_money(-500), "-$5.00");
        assert_eq!(format_money(123_456), "$1,234.56");
        assert_eq!(format_money(-100_000_000), "-$1,000,000.00");
    }

    #[test]
    fn formats_minimum_i64_without_panicking() {
        assert_eq!(format_money(i64::MIN), "-$92,233,720,368,547,758.08");
        // Input format preserves the sign instead of dropping it.
        assert_eq!(format_money_input(i64::MIN), "-92233720368547758.08");
        assert_eq!(format_money_input(-500), "-5.00");
    }
}
