import React from 'react'
import { createRoot } from 'react-dom/client'
import { App } from './App'
import { ErrorBoundary } from './ErrorBoundary'
import './styles.css'

const root = document.getElementById('root')
if (root === null) throw new Error('missing #root')

createRoot(root).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>,
)
