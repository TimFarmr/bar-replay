//! `bar-replay` — fetch and inspect cached market data.
//!
//! The desktop app is the product; this exists so the data layer can be driven
//! and verified without a UI, and so golden fixtures can be regenerated.

mod fetch;
mod format;

use chrono::NaiveDate;
use replay_core::{Error, Result, Timeframe, Timestamp};
use replay_data::{catalog, paths, providers, Store};

const USAGE: &str = "\
bar-replay — inspect the local market-data cache

USAGE:
  bar-replay providers
  bar-replay instruments
  bar-replay fetch <provider> <symbol> --from YYYY-MM-DD --to YYYY-MM-DD
  bar-replay inspect <provider> <symbol>
  bar-replay candles <provider> <symbol> --tf 1h [--cursor YYYY-MM-DD[THH:MM]] [--limit N]

Data is cached under $BAR_REPLAY_HOME (default: <app data>/.bar-replay).
Market data is fetched straight from the provider; nothing this project
operates ever sees it.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(args: &[String]) -> Result<()> {
    let Some(cmd) = args.first().map(String::as_str) else {
        println!("{USAGE}");
        return Ok(());
    };
    match cmd {
        "providers" => cmd_providers(),
        "instruments" => cmd_instruments(),
        "fetch" => cmd_fetch(&args[1..]),
        "inspect" => cmd_inspect(&args[1..]),
        "candles" => cmd_candles(&args[1..]),
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            Ok(())
        }
        other => Err(Error::Provider(format!(
            "unknown command {other:?}\n\n{USAGE}"
        ))),
    }
}

fn cmd_providers() -> Result<()> {
    // Driven by the registry so this listing cannot drift from reality.
    for p in providers::registry() {
        let detail = if p.needs_key {
            "needs your own API key".to_string()
        } else if p.from_file {
            "import a file you supply".to_string()
        } else {
            let n = providers::by_id(p.id)?.instruments()?.len();
            format!("{n} instruments, no key required")
        };
        println!("{:<12} {detail}", p.id);
        println!("{:<12} {}", "", p.label);
    }
    Ok(())
}

fn cmd_instruments() -> Result<()> {
    println!(
        "{:<11} {:<9} {:>9} {:>10}  SPREAD",
        "PROVIDER", "SYMBOL", "DECIMALS", "TZ"
    );
    for i in catalog::builtin() {
        println!(
            "{:<11} {:<9} {:>9} {:>10}  {:?}",
            i.provider,
            i.symbol,
            i.price_decimals,
            i.session_tz.to_string(),
            i.spread_mode
        );
    }
    Ok(())
}

/// `--flag value` pairs; positional args are whatever is left.
struct Opts {
    positional: Vec<String>,
    flags: std::collections::HashMap<String, String>,
}

fn parse_opts(args: &[String]) -> Result<Opts> {
    let mut positional = Vec::new();
    let mut flags = std::collections::HashMap::new();
    let mut i = 0;
    while i < args.len() {
        if let Some(name) = args[i].strip_prefix("--") {
            let value = args
                .get(i + 1)
                .ok_or_else(|| Error::Provider(format!("--{name} needs a value")))?;
            flags.insert(name.to_string(), value.clone());
            i += 2;
        } else {
            positional.push(args[i].clone());
            i += 1;
        }
    }
    Ok(Opts { positional, flags })
}

impl Opts {
    fn at(&self, i: usize, what: &str) -> Result<&str> {
        self.positional
            .get(i)
            .map(String::as_str)
            .ok_or_else(|| Error::Provider(format!("missing <{what}>\n\n{USAGE}")))
    }

    fn date(&self, name: &str) -> Result<Option<Timestamp>> {
        self.flags.get(name).map(|s| parse_when(s)).transpose()
    }

    fn required_date(&self, name: &str) -> Result<Timestamp> {
        self.date(name)?
            .ok_or_else(|| Error::Provider(format!("--{name} is required\n\n{USAGE}")))
    }
}

/// A recent week to suggest in help text. Derived from today rather than
/// written in, so the example does not rot as the years pass.
fn example_week() -> (String, String) {
    let end = chrono::Utc::now().date_naive() - chrono::Duration::days(7);
    let start = end - chrono::Duration::days(7);
    (
        start.format("%Y-%m-%d").to_string(),
        end.format("%Y-%m-%d").to_string(),
    )
}

/// `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM`, always UTC (the data policy: UTC internally).
fn parse_when(s: &str) -> Result<Timestamp> {
    let bad = || Error::Provider(format!("{s:?} is not YYYY-MM-DD or YYYY-MM-DDTHH:MM"));
    let (date, time) = match s.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (s, None),
    };
    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| bad())?;
    let naive = match time {
        Some(t) => {
            let (h, m) = t.split_once(':').ok_or_else(bad)?;
            date.and_hms_opt(
                h.parse().map_err(|_| bad())?,
                m.parse().map_err(|_| bad())?,
                0,
            )
            .ok_or_else(bad)?
        }
        None => date.and_hms_opt(0, 0, 0).ok_or_else(bad)?,
    };
    Ok(Timestamp(naive.and_utc().timestamp_millis()))
}

fn resolve(opts: &Opts) -> Result<replay_core::Instrument> {
    let provider = opts.at(0, "provider")?;
    let symbol = opts.at(1, "symbol")?;
    catalog::find(provider, symbol).ok_or_else(|| {
        Error::Provider(format!(
            "{provider}/{symbol} is not a built-in instrument; run `bar-replay instruments`"
        ))
    })
}

fn cmd_fetch(args: &[String]) -> Result<()> {
    let opts = parse_opts(args)?;
    let inst = resolve(&opts)?;
    let from = opts.required_date("from")?;
    let to = opts.required_date("to")?;
    if to <= from {
        return Err(Error::Provider("--to must be after --from".into()));
    }
    fetch::run(&inst, from, to)
}

fn cmd_inspect(args: &[String]) -> Result<()> {
    let opts = parse_opts(args)?;
    let inst = resolve(&opts)?;
    let root = paths::root();
    let path = paths::base_parquet(&root, &inst.provider, &inst.symbol);
    let store = Store::open()?;
    match store.coverage(&path)? {
        None => {
            println!("{}/{}: nothing cached yet", inst.provider, inst.symbol);
            let (from, to) = example_week();
            println!(
                "  fetch some with: bar-replay fetch {} {} --from {from} --to {to}",
                inst.provider, inst.symbol
            );
        }
        Some(c) => {
            let span_minutes = (c.to.0 - c.from.0) / Timestamp::MINUTE + 1;
            println!("{}/{}", inst.provider, inst.symbol);
            println!("  file     {}", path.display());
            println!("  from     {} UTC", c.from);
            println!("  to       {} UTC", c.to);
            println!("  bars     {}", c.bars);
            println!(
                "  gaps     {} minutes in range have no bar (weekends, holidays, quiet minutes)",
                span_minutes - c.bars
            );
            println!("  session timezone: {}", inst.session_tz);
        }
    }
    Ok(())
}

fn cmd_candles(args: &[String]) -> Result<()> {
    let opts = parse_opts(args)?;
    let inst = resolve(&opts)?;
    let tf = opts.flags.get("tf").map(String::as_str).unwrap_or("1h");
    let tf =
        Timeframe::parse(tf).ok_or_else(|| Error::Provider(format!("unknown timeframe {tf:?}")))?;
    let limit: usize = opts
        .flags
        .get("limit")
        .map(|s| s.parse())
        .transpose()
        .map_err(|_| Error::Provider("--limit must be a number".into()))?
        .unwrap_or(20);

    let root = paths::root();
    let path = paths::base_parquet(&root, &inst.provider, &inst.symbol);
    let store = Store::open()?;
    let Some(cov) = store.coverage(&path)? else {
        return Err(Error::Provider(format!(
            "nothing cached for {}/{}; run `bar-replay fetch` first",
            inst.provider, inst.symbol
        )));
    };
    // Default cursor is the end of what we have: the honest "now" of the cache.
    let cursor = opts.date("cursor")?.unwrap_or(cov.to);
    let candles = store.candles(&path, tf, inst.session_tz, cov.from, cursor)?;
    format::print_candles(&inst, tf, cursor, &candles, limit);
    Ok(())
}
