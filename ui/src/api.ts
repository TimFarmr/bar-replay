/** Typed wrappers over the Tauri commands. The only place `invoke` is called. */
import { invoke } from '@tauri-apps/api/core'
import type {
  Coverage,
  ProviderInfo,
  Review,
  FetchSummary,
  Instrument,
  SessionSummary,
  Step,
  View,
} from './types'

export const api = {
  instruments: () => invoke<Instrument[]>('instruments'),

  coverage: (provider: string, symbol: string) =>
    invoke<Coverage | null>('coverage', { provider, symbol }),

  /** Downloads straight from the provider; can take a while (ADR 0007). */
  fetchRange: (provider: string, symbol: string, from: number, to: number) =>
    invoke<FetchSummary>('fetch_range', { provider, symbol, from, to }),

  listSessions: () => invoke<SessionSummary[]>('list_sessions'),

  createSession: (
    provider: string,
    symbol: string,
    from: number,
    to: number,
    balance: number,
    spreadPoints: number,
    commissionPerUnit: number,
  ) =>
    invoke<View>('create_session', {
      request: {
        provider,
        symbol,
        from,
        to,
        balance,
        spreadPoints,
        commissionPerUnit,
      },
    }),

  openSession: (id: string) => invoke<View>('open_session', { id }),
  setTimeframe: (timeframe: string) => invoke<View>('set_timeframe', { timeframe }),
  view: () => invoke<View>('view'),
  stepForward: (count: number) => invoke<Step>('step_forward', { count }),
  stepBack: (count: number) => invoke<View>('step_back', { count }),
  jumpTo: (timestamp: number) => invoke<View>('jump_to', { timestamp }),

  placeOrder: (
    side: string,
    kind: string,
    qty: number,
    price: number | null,
    sl: number | null,
    tp: number | null,
  ) => invoke<View>('place_order', { side, kind, qty, price, sl, tp }),

  /** `order` of null targets the open position - that is a break-even move. */
  modify: (order: number | null, sl: number | null, tp: number | null) =>
    invoke<View>('modify', { order, sl, tp }),

  cancelOrder: (order: number) => invoke<View>('cancel_order', { order }),

  /** `qty` of null closes the whole position. */
  closePosition: (qty: number | null) => invoke<View>('close_position', { qty }),

  review: () => invoke<Review>('review'),

  /** Passing an existing `note` id edits that note. */
  addNote: (note: number | null, text: string, tags: string[], trade: number | null) =>
    invoke<View>('add_note', { note, text, tags, trade }),

  /** Writes CSV and JSON next to the session; returns the folder. */
  exportSession: () => invoke<string>('export_session'),

  /** Every adapter, and whether it has a key. Never the key itself. */
  providers: () => invoke<ProviderInfo[]>('providers'),

  hasApiKey: (provider: string) => invoke<boolean>('has_api_key', { provider }),
  setApiKey: (provider: string, key: string) =>
    invoke<void>('set_api_key', { provider, key }),
  clearApiKey: (provider: string) => invoke<void>('clear_api_key', { provider }),

  /** Native file picker; null if the user cancelled. */
  pickCsv: () => invoke<string | null>('pick_csv'),
  importCsv: (path: string, symbol: string, priceDecimals: number) =>
    invoke<Coverage>('import_csv', { path, symbol, priceDecimals }),

  clearCache: (provider: string, symbol: string) =>
    invoke<void>('clear_cache', { provider, symbol }),
}

/** `YYYY-MM-DD` (UTC) to epoch ms, the format every date input here uses. */
export function dateToMs(date: string): number {
  return Date.parse(`${date}T00:00:00Z`)
}

export function msToDate(ms: number): string {
  return new Date(ms).toISOString().slice(0, 10)
}
