/**
 * Mirrors the Rust DTOs in `crates/app/src/dto.rs`. Field names are camelCase
 * because the backend serializes them that way.
 *
 * Everything here is already cursor-filtered: the UI is never sent data past
 * the replay cursor and asked to hide it (spec §5.1).
 */

/** `ts` is the candle's OPEN time in epoch milliseconds, UTC. */
export interface Candle {
  ts: number
  open: number
  high: number
  low: number
  close: number
  volume: number
  /** False while this candle is still forming at the cursor (spec §5.2). */
  complete: boolean
}

export type SpreadMode = 'historical' | 'synthetic'

/** Whether the instrument ever closes. Crypto is `continuous`. */
export type MarketKind = 'continuous' | 'fx_week'

export interface Instrument {
  provider: string
  symbol: string
  priceDecimals: number
  quoteCurrency: string
  sessionTz: string
  spreadMode: SpreadMode
  market: MarketKind
}

export interface ProviderInfo {
  id: string
  label: string
  needsKey: boolean
  free: boolean
  fromFile: boolean
  /** Whether a key is stored. The key itself never reaches the UI. */
  hasKey: boolean
}

export interface Coverage {
  from: number
  to: number
  fromLabel: string
  toLabel: string
  bars: number
}

export interface FetchSummary {
  fetched: number
  cached: number
  emptyDays: number
}

export interface SessionSummary {
  id: string
  provider: string
  symbol: string
  createdMs: number
  rangeFrom: number
  rangeTo: number
  rangeLabel: string
  balance: number
}

export interface Cursor {
  cursor: number
  /** Formatted in `timezone`, not UTC (spec §4). */
  cursorLabel: string
  timezone: string
  position: number
  bars: number
  atStart: boolean
  atEnd: boolean
}

export type GapKind = 'calendar' | 'data'

/** Spec §5.4: a closed market and a hole in the data are never the same thing. */
export interface Gap {
  from: number
  to: number
  kind: GapKind
  reason: string
  missingOpenMinutes: number
}

export type Side = 'buy' | 'sell'
export type OrderKind = 'market' | 'limit' | 'stop'
export type Role = 'entry' | 'exit'
export type Reason = 'market' | 'limit' | 'stop' | 'sl' | 'tp' | 'close'

/**
 * `sl_first` means the outcome rested on the pessimistic assumption because one
 * bar touched both the stop and the target and no finer data was available.
 * Spec §5.3 requires showing this wherever it affected a trade.
 */
export type Assumption = 'none' | 'sl_first' | 'resolved_by_ticks'

export interface PositionView {
  side: Side
  qty: number
  avgEntry: number
  sl: number | null
  tp: number | null
  openedLabel: string
  unrealised: number
}

export interface WorkingOrder {
  id: number
  side: Side
  kind: OrderKind
  qty: number
  price: number | null
  sl: number | null
  tp: number | null
}

export interface TradeRow {
  seq: number
  when: string
  side: Side
  qty: number
  price: number
  role: Role
  reason: Reason
  assumption: Assumption
  commission: number
  realised: number
}

export interface Trading {
  balance: number
  equity: number
  lastPrice: number | null
  position: PositionView | null
  working: WorkingOrder[]
  trades: TradeRow[]
}

export interface View {
  sessionId: string
  instrument: Instrument
  timeframe: string
  cursor: Cursor
  candles: Candle[]
  gaps: Gap[]
  trading: Trading
}

/** A forward step: only the candles that changed. */
export interface Step {
  cursor: Cursor
  tail: Candle[]
  trading: Trading
  /** Gaps uncovered by this step (spec §5.4). */
  newGaps: Gap[]
}

export const TIMEFRAMES = ['1m', '5m', '15m', '30m', '1h', '4h', '1d', '1w'] as const
export type Timeframe = (typeof TIMEFRAMES)[number]

export interface RoundTrip {
  opened: number
  closed: number
  side: Side
  qty: number
  entry: number
  exit: number
  realised: number
  /** Null when the trade was taken without a stop, so risk is undefined. */
  rMultiple: number | null
  assumption: Assumption
}

export interface EquityPoint {
  cursor: number
  balance: number
}

export interface JournalEntry {
  id: number
  cursor: number
  text: string
  tags: string[]
  trade: number | null
}

export interface Summary {
  trades: number
  wins: number
  losses: number
  winRate: number
  net: number
  grossProfit: number
  grossLoss: number
  /** Null rather than infinity when nothing was lost. */
  profitFactor: number | null
  averageWin: number
  averageLoss: number
  expectancy: number
  maxDrawdown: number
  averageR: number | null
  /** Trades whose outcome rested on the pessimistic assumption (spec §5.3). */
  flaggedTrades: number
}

export interface Review {
  summary: Summary
  roundTrips: RoundTrip[]
  equity: EquityPoint[]
  journal: JournalEntry[]
  tags: string[]
  priceDecimals: number
}
