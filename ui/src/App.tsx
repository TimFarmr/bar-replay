import { useState } from 'react'
import { Setup } from './Setup'
import { Replay } from './Replay'
import type { View } from './types'

/** One session is open at a time (ADR 0008), so this is the whole router. */
export function App() {
  const [view, setView] = useState<View | null>(null)

  return view === null ? (
    <Setup onOpen={setView} />
  ) : (
    <Replay initial={view} onExit={() => setView(null)} />
  )
}
