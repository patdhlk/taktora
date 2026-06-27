//! A line-oriented parser for the common DBC subset.
//!
//! Handles `VERSION`, `BU_` (nodes), `BO_` (messages), `SG_` (signals), and
//! `VAL_` (value tables). Other keyword lines (`CM_`, `BA_`, `BA_DEF_`, …) are
//! recognised as known-but-unmodelled and skipped, not errored — a DBC in the
//! wild is full of them and they do not affect the message IR.

use crate::ast::{ByteOrder, DbcDatabase, DbcMessage, DbcSignal, DbcValueTable, Multiplexer};

/// What went wrong, and the kind of token involved.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseErrorKind {
    /// A `BO_` header was malformed.
    #[error("malformed message (BO_) header")]
    Message,
    /// A `SG_` signal line was malformed.
    #[error("malformed signal (SG_) line")]
    Signal,
    /// A `VAL_` value-table line was malformed.
    #[error("malformed value table (VAL_) line")]
    ValueTable,
    /// A `SG_` line appeared before any `BO_` to attach it to.
    #[error("signal (SG_) with no preceding message (BO_)")]
    OrphanSignal,
    /// A numeric field could not be parsed.
    #[error("invalid number")]
    Number,
}

/// A parse failure, located by 1-based line number.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("line {line}: {kind}")]
pub struct ParseError {
    /// 1-based source line.
    pub line: usize,
    /// What kind of token failed.
    pub kind: ParseErrorKind,
}

impl ParseError {
    const fn at(line: usize, kind: ParseErrorKind) -> Self {
        Self { line, kind }
    }
}

/// Parse DBC source text into a [`DbcDatabase`].
///
/// # Errors
///
/// Returns the first [`ParseError`] encountered, with its source line.
pub fn parse(text: &str) -> Result<DbcDatabase, ParseError> {
    let mut db = DbcDatabase::default();
    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        let Some((keyword, rest)) = split_keyword(line) else {
            continue; // blank line or a bare continuation token
        };
        match keyword {
            "VERSION" => db.version = Some(unquote(rest).to_owned()),
            "BU_:" | "BU_" => parse_nodes(rest, &mut db),
            "BO_" => {
                let msg = parse_message(rest, line_no)?;
                db.messages.push(msg);
            }
            "SG_" => {
                let sig = parse_signal(rest, line_no)?;
                db.messages
                    .last_mut()
                    .ok_or_else(|| ParseError::at(line_no, ParseErrorKind::OrphanSignal))?
                    .signals
                    .push(sig);
            }
            "VAL_" => {
                if let Some(t) = parse_value_table(rest, line_no)? {
                    db.value_tables.push(t);
                }
            }
            // Known-but-unmodelled keywords and anything else: skip.
            _ => {}
        }
    }
    Ok(db)
}

/// Split the first whitespace-delimited keyword off a line.
fn split_keyword(line: &str) -> Option<(&str, &str)> {
    if line.is_empty() {
        return None;
    }
    let kw_end = line.find(char::is_whitespace).unwrap_or(line.len());
    let (kw, rest) = line.split_at(kw_end);
    // `BU_:` has the colon glued to the keyword; normalise the no-rest case too.
    Some((kw, rest.trim_start()))
}

fn parse_nodes(rest: &str, db: &mut DbcDatabase) {
    db.nodes = rest
        .split_whitespace()
        .filter(|t| *t != ":")
        .map(ToOwned::to_owned)
        .collect();
}

/// `BO_ 256 EngineData: 8 ECU`
fn parse_message(rest: &str, line: usize) -> Result<DbcMessage, ParseError> {
    let err = || ParseError::at(line, ParseErrorKind::Message);
    let mut it = rest.split_whitespace();
    let id: u32 = it
        .next()
        .ok_or_else(err)?
        .parse()
        .map_err(|_| ParseError::at(line, ParseErrorKind::Number))?;
    let name = it.next().ok_or_else(err)?.trim_end_matches(':').to_owned();
    let dlc: u8 = it
        .next()
        .ok_or_else(err)?
        .parse()
        .map_err(|_| ParseError::at(line, ParseErrorKind::Number))?;
    let transmitter = it.next().unwrap_or("").to_owned();
    Ok(DbcMessage {
        id,
        name,
        dlc,
        transmitter,
        signals: Vec::new(),
    })
}

/// `Speed [M|m0] : 0|16@1+ (0.1,0) [0|6553.5] "km/h" Dash,Logger`
///
/// The line is parsed in three independent pieces — the LHS name/multiplexer
/// before the colon, the quoted unit and receiver list, and the three
/// whitespace-separated numeric groups (bitspec, factor/offset, min/max) — each
/// handled by a small helper so no single function carries the whole grammar.
fn parse_signal(rest: &str, line: usize) -> Result<DbcSignal, ParseError> {
    let err = || ParseError::at(line, ParseErrorKind::Signal);

    let (lhs, after_colon) = rest.split_once(':').ok_or_else(err)?;
    let (name, multiplexer) = parse_signal_lhs(lhs, line)?;

    // The unit is a quoted string that may itself contain spaces, so split the
    // numeric prefix off at the first quote rather than by whitespace alone.
    let quote_start = after_colon.find('"').ok_or_else(err)?;
    let (numeric, quoted_and_rest) = after_colon.split_at(quote_start);
    let (unit, receivers) = parse_unit_receivers(quoted_and_rest, line)?;

    let mut nums = numeric.split_whitespace();
    let bitspec = nums.next().ok_or_else(err)?; // 0|16@1+
    let factor_offset = nums.next().ok_or_else(err)?; // (0.1,0)
    let min_max = nums.next().ok_or_else(err)?; // [0|6553.5]

    let (start_bit, bit_len, byte_order, signed) = parse_bitspec(bitspec, line)?;
    let (factor, offset) = parse_pair(factor_offset, ',', line)?;
    let (min, max) = parse_pair(min_max, '|', line)?;

    Ok(DbcSignal {
        name,
        multiplexer,
        start_bit,
        bit_len,
        byte_order,
        signed,
        factor,
        offset,
        min,
        max,
        unit,
        receivers,
    })
}

/// Parse the LHS of a signal line (`Speed [M|m0]`) into name and multiplexer.
fn parse_signal_lhs(lhs: &str, line: usize) -> Result<(String, Multiplexer), ParseError> {
    let err = || ParseError::at(line, ParseErrorKind::Signal);
    let num = || ParseError::at(line, ParseErrorKind::Number);
    let mut lhs_tokens = lhs.split_whitespace();
    let name = lhs_tokens.next().ok_or_else(err)?.to_owned();
    let multiplexer = match lhs_tokens.next() {
        None => Multiplexer::None,
        Some("M") => Multiplexer::Multiplexor,
        Some(tok) => {
            let n = tok
                .strip_prefix('m')
                .ok_or_else(err)?
                .parse()
                .map_err(|_| num())?;
            Multiplexer::Multiplexed(n)
        }
    };
    Ok((name, multiplexer))
}

/// Parse the quoted unit and trailing receiver list from `"km/h" Dash,Logger`.
/// `s` must start at the opening quote.
fn parse_unit_receivers(s: &str, line: usize) -> Result<(String, Vec<String>), ParseError> {
    let err = || ParseError::at(line, ParseErrorKind::Signal);
    let body = s.strip_prefix('"').ok_or_else(err)?;
    let unit_end = body.find('"').ok_or_else(err)?;
    let unit = body[..unit_end].to_owned();
    let receivers = body[unit_end + 1..]
        .split([' ', '\t', ','])
        .filter(|t| !t.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    Ok((unit, receivers))
}

/// Parse a bitspec `start|len@order sign` (e.g. `0|16@1+`).
fn parse_bitspec(bitspec: &str, line: usize) -> Result<(u16, u16, ByteOrder, bool), ParseError> {
    let err = || ParseError::at(line, ParseErrorKind::Signal);
    let num = || ParseError::at(line, ParseErrorKind::Number);
    let (start_s, after_start) = bitspec.split_once('|').ok_or_else(err)?;
    let (len_s, order_sign) = after_start.split_once('@').ok_or_else(err)?;
    let start_bit: u16 = start_s.parse().map_err(|_| num())?;
    let bit_len: u16 = len_s.parse().map_err(|_| num())?;
    let mut os = order_sign.chars();
    let byte_order = match os.next() {
        Some('1') => ByteOrder::LittleEndian,
        Some('0') => ByteOrder::BigEndian,
        _ => return Err(err()),
    };
    let signed = match os.next() {
        Some('-') => true,
        Some('+') => false,
        _ => return Err(err()),
    };
    Ok((start_bit, bit_len, byte_order, signed))
}

/// Parse a bracketed numeric pair, e.g. `(0.1,0)` (sep `,`) or `[0|6553.5]`
/// (sep `|`). Surrounding `()`/`[]` brackets are stripped before splitting.
fn parse_pair(s: &str, sep: char, line: usize) -> Result<(f64, f64), ParseError> {
    let err = || ParseError::at(line, ParseErrorKind::Signal);
    let num = || ParseError::at(line, ParseErrorKind::Number);
    let (a_s, b_s) = s
        .trim_start_matches(['(', '['])
        .trim_end_matches([')', ']'])
        .split_once(sep)
        .ok_or_else(err)?;
    let a: f64 = a_s.parse().map_err(|_| num())?;
    let b: f64 = b_s.parse().map_err(|_| num())?;
    Ok((a, b))
}

/// `VAL_ 256 Gear 0 "Neutral" 1 "First" ;`
///
/// Returns `Ok(None)` for the env-variable form of `VAL_` (no numeric message
/// id), which this slice does not model.
fn parse_value_table(rest: &str, line: usize) -> Result<Option<DbcValueTable>, ParseError> {
    let err = || ParseError::at(line, ParseErrorKind::ValueTable);
    let trimmed = rest.trim_end_matches(';').trim();
    let (id_tok, after_id) = trimmed.split_once(char::is_whitespace).ok_or_else(err)?;
    let Ok(message_id) = id_tok.parse::<u32>() else {
        return Ok(None); // environment-variable value description: skip
    };
    let after_id = after_id.trim_start();
    let (signal, mut after_sig) = after_id.split_once(char::is_whitespace).ok_or_else(err)?;
    after_sig = after_sig.trim();

    let mut entries = Vec::new();
    while !after_sig.is_empty() {
        let (val_tok, rest_after_val) =
            after_sig.split_once(char::is_whitespace).ok_or_else(err)?;
        let value: i64 = val_tok
            .parse()
            .map_err(|_| ParseError::at(line, ParseErrorKind::Number))?;
        let rest_after_val = rest_after_val.trim_start();
        let body = rest_after_val.strip_prefix('"').ok_or_else(err)?;
        let close = body.find('"').ok_or_else(err)?;
        entries.push((value, body[..close].to_owned()));
        after_sig = body[close + 1..].trim_start();
    }

    Ok(Some(DbcValueTable {
        message_id,
        signal: signal.to_owned(),
        entries,
    }))
}

fn unquote(s: &str) -> &str {
    s.trim().trim_matches('"')
}
