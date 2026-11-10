//! What an in-app swarm costs (D56, amendments "Workers and cost"): prices
//! per model, spend from the API's `usage`, the estimate before a start,
//! and the cap.
//!
//! Prices are Anthropic's first-party rates in US dollars per million
//! tokens, from the claude-api skill (its model table, cached 2026-10-06,
//! and its prompt caching notes): cache reads cost 0.1x the input price,
//! except 0.05x on Claude Opus 5.5 and 0.025x on Claude Fable 5.1 and
//! Claude Mythos 5.1; a cache write (5 minute TTL) costs 1.25x. A model
//! without a row (a typo, a dated id, a model newer than the table) is
//! billed at the table's highest row, with a note on the cost, so the cap
//! always holds (gate P0-1).

use guhit_model::SwarmCost;

use crate::ai::client::Usage;

/// The cap when the person set none (`swarm_cap_usd` in settings.json).
pub const DEFAULT_CAP_USD: f64 = 3.0;
/// settings.json key of the person's cap.
pub const CAP_SETTING: &str = "swarm_cap_usd";
/// The highest cap the window may set.
pub const MAX_CAP_USD: f64 = 100.0;

/// Cache write price as a multiple of the input price (5 minute TTL).
const CACHE_WRITE_X: f64 = 1.25;

/// One model's price, US dollars per million tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    /// Claude Haiku 5.5 bills prompts over 100K tokens at a higher rate:
    /// (prompt tokens above which, input, output).
    pub long: Option<(u64, f64, f64)>,
}

const fn price(input: f64, output: f64, read_x: f64) -> Price {
    Price { input, output, cache_read: input * read_x, cache_write: input * CACHE_WRITE_X, long: None }
}

/// The table's highest row, every figure the most any row has: what a
/// model the table does not know is billed at.
pub const TOP: Price = price(10.0, 50.0, 0.1);

/// The note on a cost billed at `TOP`.
pub const PRICE_UNKNOWN: &str = "Billed at the highest rate: price unknown";

/// The price of `model`, by its exact id. None for a model the table does
/// not know (`billed` then gives `TOP`).
pub fn price_of(model: &str) -> Option<Price> {
    Some(match model.trim() {
        "claude-fable-5-1" | "claude-mythos-5-1" => price(10.0, 50.0, 0.025),
        "claude-fable-5" => price(10.0, 50.0, 0.1),
        "claude-opus-5-5" => price(4.0, 20.0, 0.05),
        "claude-opus-5" | "claude-opus-4-8" | "claude-opus-4-7" | "claude-opus-4-6" => price(5.0, 25.0, 0.1),
        "claude-sonnet-5-5" | "claude-sonnet-5" => price(2.0, 10.0, 0.1),
        "claude-sonnet-4-6" => price(3.0, 15.0, 0.1),
        "claude-haiku-5-5" => Price { long: Some((100_000, 0.50, 2.50)), ..price(0.10, 0.50, 0.1) },
        "claude-haiku-4-5" => price(1.0, 5.0, 0.1),
        _ => return None,
    })
}

/// What `model` is billed at: its row, else `TOP`. True when it has a row.
pub fn billed(model: &str) -> (Price, bool) {
    match price_of(model) {
        Some(p) => (p, true),
        None => (TOP, false),
    }
}

/// The model as people read it, by its exact id (the claude-api skill's
/// model table). None for a model the table does not know.
pub fn label_of(model: &str) -> Option<&'static str> {
    Some(match model.trim() {
        "claude-fable-5-1" => "Claude Fable 5.1",
        "claude-mythos-5-1" => "Claude Mythos 5.1",
        "claude-fable-5" => "Claude Fable 5",
        "claude-opus-5-5" => "Claude Opus 5.5",
        "claude-opus-5" => "Claude Opus 5",
        "claude-opus-4-8" => "Claude Opus 4.8",
        "claude-opus-4-7" => "Claude Opus 4.7",
        "claude-opus-4-6" => "Claude Opus 4.6",
        "claude-sonnet-5-5" => "Claude Sonnet 5.5",
        "claude-sonnet-5" => "Claude Sonnet 5",
        "claude-sonnet-4-6" => "Claude Sonnet 4.6",
        "claude-haiku-5-5" => "Claude Haiku 5.5",
        "claude-haiku-4-5" => "Claude Haiku 4.5",
        _ => return None,
    })
}

/// A new cost for a swarm on `model`: its label, the estimate and the cap,
/// with `PRICE_UNKNOWN` for a model without a row.
pub fn new_cost(model: &str, cap_usd: f64, sizes: &Sizes) -> SwarmCost {
    let (low, high) = estimate(model, sizes);
    let (_, known) = billed(model);
    SwarmCost {
        model: model.to_string(),
        model_label: Some(label_of(model).map(str::to_string).unwrap_or_else(|| model.trim().to_string())),
        price_note: (!known).then(|| PRICE_UNKNOWN.to_string()),
        low_usd: Some(low),
        high_usd: Some(high),
        cap_usd,
        ..SwarmCost::default()
    }
}

/// What one response cost, in US dollars.
pub fn usd(usage: &Usage, p: &Price) -> f64 {
    let (input, output, read, write) = match p.long {
        Some((over, input, output)) if usage.prompt_tokens() > over => {
            (input, output, input * (p.cache_read / p.input), input * CACHE_WRITE_X)
        }
        _ => (p.input, p.output, p.cache_read, p.cache_write),
    };
    (usage.input_tokens as f64 * input
        + usage.output_tokens as f64 * output
        + usage.cache_read_tokens as f64 * read
        + usage.cache_write_tokens as f64 * write)
        / 1e6
}

/// Count one response into `cost`: its tokens, and its dollars at the price
/// of the model that answered (`answered`), else of the model the swarm
/// asked for, else at `TOP` with `PRICE_UNKNOWN` on the cost. Every
/// response costs something, so the cap always holds.
pub fn add(cost: &mut SwarmCost, usage: &Usage, answered: Option<&str>) {
    cost.input_tokens += usage.input_tokens;
    cost.output_tokens += usage.output_tokens;
    cost.cache_read_tokens += usage.cache_read_tokens;
    cost.cache_write_tokens += usage.cache_write_tokens;
    let known = answered.map(str::trim).filter(|m| !m.is_empty()).and_then(price_of).or_else(|| price_of(&cost.model));
    let price = known.unwrap_or_else(|| {
        cost.price_note = Some(PRICE_UNKNOWN.to_string());
        TOP
    });
    cost.spent_usd += usd(usage, &price);
}

/// A rough token count for text sent to the model: 4 characters a token.
pub fn tokens_of(chars: usize) -> u64 {
    (chars as u64).div_ceil(4)
}

/// What the next request of a worker may cost, to hold against the cap:
/// what it sent last time read from the cache, the new part of the prompt
/// written to it, and `output` tokens out. `prev_prompt` 0 (the first
/// request) writes the whole prompt.
pub fn next_request_usd(p: &Price, prev_prompt: u64, prompt: u64, output: u64) -> f64 {
    let read = prev_prompt.min(prompt);
    let usage = Usage { input_tokens: 0, output_tokens: output, cache_read_tokens: read, cache_write_tokens: prompt - read };
    usd(&usage, p)
}

// ================================================================ estimate

/// Parts the estimate assumes: a small plan and a big one.
const LOW_PARTS: u64 = 3;
const HIGH_PARTS: u64 = 5;
/// Rounds a worker takes, low and high (at most `worker::MAX_WORKER_ROUNDS`).
const LOW_ROUNDS: u64 = 8;
const HIGH_ROUNDS: u64 = 16;
/// Tokens a round adds to a worker's conversation (its tool calls and their
/// results) and tokens it writes (thinking and the calls).
const GROWTH_PER_ROUND: u64 = 1200;
const OUTPUT_PER_ROUND: u64 = 1000;
/// Tokens of the lead's answer: the plan.
const LEAD_OUTPUT: u64 = 2500;

/// Sizes the estimate starts from, in tokens.
#[derive(Debug, Clone, Copy)]
pub struct Sizes {
    /// The workers' system prompt and tools: one cached prefix for all.
    pub worker_prefix: u64,
    /// The lead's system prompt and tool.
    pub lead_prefix: u64,
    /// The project context and the brief each conversation starts with.
    pub context: u64,
}

/// One worker of `rounds` rounds: the first request writes its prompt (the
/// shared prefix is read, the first worker's request wrote it), each later
/// one reads what came before and writes what the round added.
fn worker_usd(p: &Price, s: &Sizes, rounds: u64) -> f64 {
    let mut total = usd(
        &Usage {
            input_tokens: 0,
            output_tokens: OUTPUT_PER_ROUND,
            cache_read_tokens: s.worker_prefix,
            cache_write_tokens: s.context,
        },
        p,
    );
    for r in 1..rounds {
        let before = s.worker_prefix + s.context + r * GROWTH_PER_ROUND;
        total += usd(
            &Usage { input_tokens: 0, output_tokens: OUTPUT_PER_ROUND, cache_read_tokens: before, cache_write_tokens: GROWTH_PER_ROUND },
            p,
        );
    }
    total
}

/// The low and high figures for a swarm on `model`, spending nothing: the
/// lead's plan, plus 3 parts of 8 rounds (low) to 5 parts of 16 (high), the
/// shared prefix written once. A model without a price is estimated at
/// `TOP`.
pub fn estimate(model: &str, s: &Sizes) -> (f64, f64) {
    let (p, _) = billed(model);
    let lead = usd(
        &Usage { input_tokens: 0, output_tokens: LEAD_OUTPUT, cache_read_tokens: 0, cache_write_tokens: s.lead_prefix + s.context },
        &p,
    );
    let prefix_write = s.worker_prefix as f64 * (p.cache_write - p.cache_read) / 1e6;
    let figure = |parts: u64, rounds: u64| round_cents(lead + prefix_write + parts as f64 * worker_usd(&p, s, rounds));
    (figure(LOW_PARTS, LOW_ROUNDS), figure(HIGH_PARTS, HIGH_ROUNDS))
}

fn round_cents(usd: f64) -> f64 {
    (usd * 100.0).round() / 100.0
}

/// A cap a person may set: more than nothing, at most `MAX_CAP_USD`.
pub fn check_cap(cap: f64) -> Result<f64, guhit_model::IpcError> {
    if cap.is_finite() && cap > 0.0 && cap <= MAX_CAP_USD {
        Ok(cap)
    } else {
        Err(guhit_model::IpcError::new(
            "bad_args",
            format!("The spending cap is more than US$0 and at most US${MAX_CAP_USD:.0}."),
        ))
    }
}

/// The cap in settings.json, else `DEFAULT_CAP_USD`.
pub fn cap_setting(data_dir: &std::path::Path) -> f64 {
    crate::files::read_settings(data_dir)
        .get(CAP_SETTING)
        .and_then(serde_json::Value::as_f64)
        .and_then(|c| check_cap(c).ok())
        .unwrap_or(DEFAULT_CAP_USD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_follow_the_skill_table() {
        let opus = price_of("claude-opus-5-5").unwrap();
        assert_eq!((opus.input, opus.output), (4.0, 20.0));
        assert!((opus.cache_read - 0.20).abs() < 1e-9, "Opus 5.5 reads at 0.05x");
        assert!((opus.cache_write - 5.0).abs() < 1e-9, "writes at 1.25x");
        assert!((price_of("claude-fable-5-1").unwrap().cache_read - 0.25).abs() < 1e-9, "Fable 5.1 reads at 0.025x");
        assert!((price_of("claude-sonnet-5-5").unwrap().cache_read - 0.20).abs() < 1e-9);
        assert_eq!(price_of("claude-opus-5").unwrap().input, 5.0);
        assert!(price_of("claude-opus-5-5-20260401").is_none(), "exact ids only");
        assert!(price_of("gpt-x").is_none());
    }

    #[test]
    fn spend_counts_every_kind_of_token() {
        let u = Usage { input_tokens: 1_000_000, output_tokens: 1_000_000, cache_read_tokens: 1_000_000, cache_write_tokens: 1_000_000 };
        let p = price_of("claude-opus-5-5").unwrap();
        assert!((usd(&u, &p) - (4.0 + 20.0 + 0.2 + 5.0)).abs() < 1e-9);
        // Haiku 5.5 over 100K prompt tokens bills the higher rate.
        let h = price_of("claude-haiku-5-5").unwrap();
        let small = Usage { input_tokens: 50_000, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0 };
        let big = Usage { input_tokens: 200_000, ..small };
        assert!((usd(&small, &h) - 0.005).abs() < 1e-9);
        assert!((usd(&big, &h) - 0.1).abs() < 1e-9);
    }

    #[test]
    fn a_fallback_is_priced_at_the_model_that_answered() {
        let mut cost = SwarmCost { model: "claude-opus-5-5".into(), ..SwarmCost::default() };
        let u = Usage { input_tokens: 1_000_000, ..Usage::default() };
        add(&mut cost, &u, Some("claude-opus-4-8"));
        assert!((cost.spent_usd - 5.0).abs() < 1e-9);
        add(&mut cost, &u, None);
        assert!((cost.spent_usd - 9.0).abs() < 1e-9);
        assert_eq!(cost.input_tokens, 2_000_000);
        // An answering model without a row is billed at the one asked for.
        add(&mut cost, &u, Some("claude-opus-5-5-20260401"));
        assert!((cost.spent_usd - 13.0).abs() < 1e-9);
        assert!(cost.price_note.is_none());
        // Neither has a row: the highest row, flagged.
        let mut unknown = SwarmCost { model: "my-model".into(), ..SwarmCost::default() };
        add(&mut unknown, &u, None);
        assert_eq!((unknown.spent_usd, unknown.input_tokens), (10.0, 1_000_000), "the highest row");
        assert_eq!(unknown.price_note.as_deref(), Some(PRICE_UNKNOWN));
    }

    #[test]
    fn the_top_row_is_the_most_any_row_charges_and_every_row_has_a_label() {
        for id in [
            "claude-fable-5-1",
            "claude-mythos-5-1",
            "claude-fable-5",
            "claude-opus-5-5",
            "claude-opus-5",
            "claude-opus-4-8",
            "claude-opus-4-7",
            "claude-opus-4-6",
            "claude-sonnet-5-5",
            "claude-sonnet-5",
            "claude-sonnet-4-6",
            "claude-haiku-5-5",
            "claude-haiku-4-5",
        ] {
            let p = price_of(id).unwrap();
            assert!(p.input <= TOP.input && p.output <= TOP.output, "{id}");
            assert!(p.cache_read <= TOP.cache_read && p.cache_write <= TOP.cache_write, "{id}");
            assert!(label_of(id).is_some(), "{id}");
        }
        assert_eq!(label_of("claude-opus-5-5"), Some("Claude Opus 5.5"));
        assert_eq!(billed("claude-opus-5-5-20260401"), (TOP, false));
        let s = Sizes { worker_prefix: 12_000, lead_prefix: 2_000, context: 3_000 };
        let c = new_cost("claude-opus-5-5", 3.0, &s);
        assert_eq!((c.model_label.as_deref(), c.price_note.as_deref()), (Some("Claude Opus 5.5"), None));
        let c = new_cost("claude-opus-5-5-20260401", 3.0, &s);
        assert_eq!(c.model_label.as_deref(), Some("claude-opus-5-5-20260401"));
        assert_eq!(c.price_note.as_deref(), Some(PRICE_UNKNOWN));
    }

    #[test]
    fn the_estimate_has_a_low_and_a_high_figure() {
        let s = Sizes { worker_prefix: 12_000, lead_prefix: 2_000, context: 3_000 };
        let (low, high) = estimate("claude-opus-5-5", &s);
        assert!(low > 0.0 && low < high, "{low} < {high}");
        assert!(high < 10.0, "{high}");
        // A pricier model costs more, and a model without a price the most.
        let fable = estimate("claude-fable-5-1", &s).1;
        assert!(fable > high);
        assert!(estimate("someone-elses-model", &s).1 >= fable);
    }

    #[test]
    fn caps_are_checked() {
        assert!(check_cap(3.0).is_ok());
        assert!(check_cap(0.0).is_err());
        assert!(check_cap(f64::NAN).is_err());
        assert!(check_cap(1000.0).is_err());
    }
}
