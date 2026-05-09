import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'
import { listen, UnlistenFn } from '@tauri-apps/api/event'

export interface AgentEvent {
  kind: 'tool_call' | 'tool_result' | 'message' | 'done' | 'error'
  content: string
  timestamp: number
}

interface AgentStore {
  status: 'idle' | 'running' | 'done' | 'error'
  events: AgentEvent[]
  currentPrompt: string

  runAgent: (prompt: string) => Promise<void>
  reset: () => void
}

export const useAgentStore = create<AgentStore>((set) => ({
  status: 'idle',
  events: [],
  currentPrompt: '',

  runAgent: async (prompt) => {
    set({ status: 'running', events: [], currentPrompt: prompt })

    let unlisten: UnlistenFn | null = null

    unlisten = await listen<AgentEvent>('agent_event', (e) => {
      const event = { ...e.payload, timestamp: Date.now() }
      set((s) => ({ events: [...s.events, event] }))
      if (event.kind === 'done') {
        set({ status: 'done' })
        unlisten?.()
      }
      if (event.kind === 'error') {
        set({ status: 'error' })
        unlisten?.()
      }
    })

    try {
      await invoke('run_agent_command', { prompt, history: [] })
    } catch (err: any) {
      set((s) => ({
        status: 'error',
        events: [...s.events, { kind: 'error' as const, content: String(err), timestamp: Date.now() }]
      }))
      unlisten?.()
    }
  },

  reset: () => set({ status: 'idle', events: [], currentPrompt: '' }),
}))
