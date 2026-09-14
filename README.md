# Bar Replay

Replay the market bar by bar and trade it without knowing what comes next.

You pick an instrument and a date. The chart plays forward one candle at a time,
and you place simulated trades seeing exactly what you would have seen at the
time — no more. It is a practice ground for discretionary traders.

**No account. No API key. No server.** Market data goes straight from the data
provider to your machine. Nothing this project operates ever touches it, sees
it, or stores it.

Free and open source under the AGPL-3.0. There is no paid tier and never will
be.

> **Status.** Everything described below works and is covered by tests, but no
> signed release has been published yet. The Windows installer can be built
> from source (see [Building from source](#building-from-source)); macOS and
> Linux installers need to be built on those systems. Until the project buys
> code-signing certificates, installers are **unsigned**, so Windows SmartScreen
> and macOS Gatekeeper will warn the first time you run one.

---

## Getting started in five minutes

1. **Install it.** Download the installer for your system and run it. Nothing
   to sign up for.
2. **Open it.** You land on a short form. It is already filled in with Bitcoin
   (BTCUSDT) and a month of 2024.
3. **Press "Download this range."** This pulls the candles from the provider to
   your computer. Bitcoin takes a few seconds. Currencies take longer — see
   [How long downloads take](#how-long-downloads-take).
4. **Press "Start replay."**
5. **Press play**, or tap the right arrow key to step forward one candle at a
   time. Press Buy or Sell when you see something you like.

That is the whole product. Everything below is detail you can read later.

### The controls

| Control | What it does |
|---|---|
| **play / pause** | Reveals candles automatically. Set the speed beside it. |
| **step ▶ / ◀ step** | One candle forward or back. Arrow keys do the same. |
| **10 ⏩ / ⏪ 10** | Ten candles at a time. Shift + arrow keys. |
| **Jump to** | Skip to a date. |
| **1m … 1w** | Change the timeframe. Your place in the replay does not move. |
| **Review** | Statistics, equity curve, your journal, and export. |
| **Spacebar** | Play/pause. |

**Stepping backwards really does undo things.** If you step back past the moment
a trade filled, that trade is gone — not hidden, gone. Step forward again and it
fills exactly as it did before. Your position can never be a leftover from a
future that has been rewound.

---

## Reading the numbers honestly

A backtester that quietly flatters you is worse than none at all, because it
manufactures confidence you have not earned. So this app tells you when it is
guessing.

### "synthetic spread" vs "real spread"

Top right of the replay screen, always visible.

- **real spread** — the provider publishes genuine bid and ask prices
  (Dukascopy, for currencies), so a historical spread *could* be derived.
  **Not yet implemented:** today the app still charges the spread you typed on
  the setup screen. The badge tells you the data supports better, not that you
  are getting it.
- **synthetic spread** — the provider only publishes traded prices (Binance,
  for crypto). Any spread is a number *you* chose. It is a plausible
  assumption, not history. Do not treat a result that depends on it as proof.

Either way, the spread you are charged right now is the one you set. Set it to
something realistic for your broker, or leave it at zero and remember that your
results are correspondingly optimistic.

### The ⚠ assumed flag on a trade

Sometimes a single one-minute candle touches both your stop loss **and** your
take profit. From the four prices of that candle — open, high, low, close —
there is genuinely no way to know which one happened first.

This app never resolves that in your favour. It assumes **the stop loss came
first**, marks the trade with ⚠, and counts it in "trades that depended on the
stop-first assumption" on the Review screen. Your real result might have been
better. It might not have been. The app will not pretend to know.

### Gaps in the chart

Missing time is always shown as a gap. Candles are never invented, never
interpolated, never carried forward to make the chart look tidy. There are two
kinds and the app does not confuse them:

- **market closed** — a weekend or a holiday. Normal, and drawn the way any
  chart skips non-trading time.
- **data missing** — the market was open and the provider had no data. You get
  an explicit warning bar. That is a hole in the data, not quiet trading, and
  you should not read price action across it.

### Timezones

Everything is stored in UTC. Daily and weekly candles open at midnight in the
instrument's own session timezone, and **that timezone is named on screen** next
to the spread badge. It is never silently assumed.

---

## Where the data comes from

| Provider | Covers | Key needed? |
|---|---|---|
| **Binance** | Crypto, full minute history | No |
| **Dukascopy** | Currencies and gold, minute and tick history | No |
| **CSV file** | Anything you have your own data for | No |
| **Databento** | CME futures (NQ, ES) and equities | Yes — your own |

The two free ones work on first launch with nothing configured. Databento is an
upgrade path, never a requirement.

**Your API key is yours.** It is stored in your operating system's keychain —
Windows Credential Manager, macOS Keychain, or the Linux secret service. It is
never written to a config file, never written to a log, and never sent anywhere
except Databento's own API.

### How long downloads take

Crypto is fast: a month of one-minute Bitcoin candles arrives in about five
seconds.

Currencies are slow. Dukascopy's free feed serves one file per day and
throttles anyone who asks quickly, so a week takes something like ten to twenty
seconds and can stall. The app retries patiently and tells you if it truly
fails. **Start with a week, not a year.**

### Disk space

Downloaded candles are kept so you never fetch them twice. Nothing is deleted
automatically — the cache is yours to manage, and a session keeps its own copy
of the candles it started with so that a provider quietly revising history later
cannot change trades you already took.

---

## Your session is saved continuously

Close the app whenever you like, including with a position open. The position is
saved exactly as it stood — it is never force-closed — and reopening the session
puts you back at the same candle with the same trade open.

Sessions are recorded as a log of what you did, so reopening one replays it.
That is also why the same session always produces the same trade log.

---

## Exporting

**Review → Export CSV + JSON** writes four files next to the session:

- `trades.csv` — every fill
- `round-trips.csv` — entry-to-exit trades with R-multiples
- `journal.csv` — your notes and tags
- `session.json` — all of it in one document

These are files on your disk. Nothing is uploaded.

---

## What this deliberately is not

No automated strategies or optimisers. No live broker connections. No accounts,
no cloud sync, no sharing. No AI features. No server component of any kind.

When more features and less friction have been in conflict, friction won.

---

## Building from source

You need [Rust](https://rustup.rs) and [Node 20+](https://nodejs.org).

```sh
cd ui && npm install && npm run build && cd ..
cargo build --release -p bar-replay-app
```

The binary lands in `target/release/`. For development, run `npm run dev` in
`ui/` and then `cargo run -p bar-replay-app` in another terminal.

To build an installer for your own platform:

```sh
cd ui && npm install && cd ..
cd crates/app && ../../ui/node_modules/.bin/tauri build
```

That produces an MSI and an NSIS setup on Windows, a `.dmg` on macOS, and
`.deb`/`.AppImage` on Linux, under `target/release/bundle/`. They are unsigned
unless you supply your own certificate — see Tauri's signing documentation.

There is also a command-line tool for inspecting the data cache without the UI:

```sh
cargo run -p replay-cli -- instruments
cargo run -p replay-cli -- fetch binance BTCUSDT --from 2024-01-01 --to 2024-02-01
cargo run -p replay-cli -- candles binance BTCUSDT --tf 1h
```

Run the tests with `cargo test --workspace`. The no-lookahead property test
takes about two minutes; that is expected — it runs eight thousand queries.

Design decisions are recorded in [docs/adr](docs/adr) and the data model in
[docs/schema.md](docs/schema.md). Read those before proposing architectural
changes; several of them are deliberate and load-bearing.

## License

AGPL-3.0-only. See [LICENSE](LICENSE).

This license was chosen to prevent a closed-source hosted fork. Market data must
never be routed through infrastructure this project operates — that constraint
is legal, not stylistic, and any change that breaks it will be rejected.
