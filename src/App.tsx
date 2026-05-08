import React, { useEffect, useCallback, useRef, useState } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import { Paperclip, Camera, HeadCircuit, ClockCounterClockwise } from '@phosphor-icons/react'
import { TabStrip } from './components/TabStrip'
import { ConversationView } from './components/ConversationView'
import { InputBar } from './components/InputBar'
import { StatusBar } from './components/StatusBar'
import { MarketplacePanel } from './components/MarketplacePanel'
import { ConnectorsPanel } from './components/ConnectorsPanel'
import { PopoverLayerProvider } from './components/PopoverLayer'
import { useClaudeEvents } from './hooks/useClaudeEvents'
import { useHealthReconciliation } from './hooks/useHealthReconciliation'
import { useSessionStore } from './stores/sessionStore'
import { useColors, useThemeStore, spacing } from './theme'
import { useAgentStore } from './store/agent'
import { ApiKeyGate } from './components/ApiKeyGate'
import { PlanApprovalPanel } from './components/PlanApprovalPanel'
import { invoke } from '@tauri-apps/api/core'

const TRANSITION = { duration: 0.26, ease: [0.4, 0, 0.1, 1] as const }

export default function App() {
  useClaudeEvents()
  useHealthReconciliation()

  const activeTabStatus = useSessionStore((s) => s.tabs.find((t) => t.id === s.activeTabId)?.status)
  const addAttachments = useSessionStore((s) => s.addAttachments)
  const colors = useColors()
  const setSystemTheme = useThemeStore((s) => s.setSystemTheme)
  const expandedUI = useThemeStore((s) => s.expandedUI)

  // ─── Theme initialization ───
  useEffect(() => {
    // Get initial OS theme — setSystemTheme respects themeMode (system/light/dark)
    window.clui.getTheme().then(({ isDark }) => {
      setSystemTheme(isDark)
    }).catch(() => {})

    // Listen for OS theme changes
    const unsub = window.clui.onThemeChange((isDark) => {
      setSystemTheme(isDark)
    })
    return unsub
  }, [setSystemTheme])

  useEffect(() => {
    if (window.clui?.resetWindowPosition) {
      window.clui.resetWindowPosition();
    }
    useSessionStore.getState().initStaticInfo().then(() => {
      const homeDir = useSessionStore.getState().staticInfo?.homePath || '~'
      const tab = useSessionStore.getState().tabs[0]
      if (tab) {
        // Set working directory to home by default (user hasn't chosen yet)
        useSessionStore.setState((s) => ({
          tabs: s.tabs.map((t, i) => (i === 0 ? { ...t, workingDirectory: homeDir, hasChosenDirectory: false } : t)),
        }))
        window.clui.createTab().then(({ tabId }) => {
          useSessionStore.setState((s) => ({
            tabs: s.tabs.map((t, i) => (i === 0 ? { ...t, id: tabId } : t)),
            activeTabId: tabId,
          }))
        }).catch(() => {})
      }
    })
  }, [])

  // Shared drag ref — must be declared before the setIgnoreMouseEvents effect so both closures can read it
  const dragRef = useRef<{ startX: number; startY: number } | null>(null)

  // ─── Dynamic window resizing ───
  // Resize the native window to exactly fit the visible UI content.
  const isExpanded = useSessionStore((s) => s.isExpanded)
  const marketplaceOpen = useSessionStore((s) => s.marketplaceOpen)
  const connectorsOpen = useSessionStore((s) => s.connectorsOpen)
  const [isFilePickerCompact, setIsFilePickerCompact] = useState(false)
  const [isFilePickerWindowCompact, setIsFilePickerWindowCompact] = useState(false)
  const pendingPlan = useSessionStore((s) => s.pendingPlan)

  useEffect(() => {
    if (!window.clui?.resizeWindow) return
    if (isFilePickerWindowCompact) {
      // Keep only the input row visible while native file dialog is open.
      window.clui.resizeWindow(1200, 90)
      return
    }
    // Collapsed: just tab strip + input pill ≈ 148px
    // Expanded: tab strip + conversation + input ≈ 700px
    // Marketplace/connectors open: add panel space.
    let height = 148
    if (isExpanded && (marketplaceOpen || connectorsOpen)) {
      height = 820
    } else if (isExpanded) {
      height = 700
    } else if (marketplaceOpen || connectorsOpen) {
      height = 720
    }
    window.clui.resizeWindow(1200, height)
  }, [isExpanded, marketplaceOpen, connectorsOpen, isFilePickerWindowCompact])

  // NOTE: The OS-level click-through (setIgnoreMouseEvents) has been removed.
  // With the window dynamically sized to fit the UI content, there is no large
  // invisible region to click through. Tauri's setIgnoreCursorEvents(true) 
  // completely blocks ALL mouse events (unlike Electron's forward:true), making
  // it impossible for the window to recover once activated.

  // Native window drag — only via the dedicated drag handle [data-drag-handle]
  useEffect(() => {
    const onMouseDown = (e: MouseEvent) => {
      const el = e.target as HTMLElement
      // Only allow drag from the dedicated drag handle
      if (!el.closest('[data-drag-handle]')) return
      
      // Double-click: snap back to default position
      if (e.detail >= 2) {
        window.clui.resetWindowPosition?.()
        return
      }
      
      // Prevent setIgnoreMouseEvents from turning off interactability mid-drag request
      dragRef.current = { startX: e.clientX, startY: e.clientY }
      
      // Ensure full mouse capture for the duration of the drag, then rely on native Tauri drag
      window.clui.setIgnoreMouseEvents?.(false)
      window.clui.startDraggingNative?.()
    }

    const onMouseUp = () => {
      dragRef.current = null
    }

    document.addEventListener('mousedown', onMouseDown)
    window.addEventListener('mouseup', onMouseUp) // catch mouseups anywhere
    return () => {
      document.removeEventListener('mousedown', onMouseDown)
      window.removeEventListener('mouseup', onMouseUp)
    }
  }, [])

  const isRunning = activeTabStatus === 'running' || activeTabStatus === 'connecting'

  // Layout dimensions — expandedUI widens and heightens the panel
  const contentWidth = expandedUI ? 800 : spacing.contentWidth
  const cardExpandedWidth = expandedUI ? 800 : 560
  const cardCollapsedWidth = expandedUI ? 760 : 530
  const cardCollapsedMargin = expandedUI ? 15 : 15
  const bodyMaxHeight = expandedUI ? 620 : 480

  const handleScreenshot = useCallback(async () => {
    const result = await window.clui.takeScreenshot()
    if (!result) return
    addAttachments([result])
  }, [addAttachments])

  const handleAttachFile = useCallback(async () => {
    setIsFilePickerCompact(true)
    // Let the collapse animation play before shrinking the native window.
    await new Promise((r) => requestAnimationFrame(() => r(null)))
    await new Promise((r) => setTimeout(r, 140))
    setIsFilePickerWindowCompact(true)
    await new Promise((r) => setTimeout(r, 70))
    try {
      const files = await window.clui.attachFiles()
      if (!files || files.length === 0) return
      addAttachments(files)
    } finally {
      setIsFilePickerWindowCompact(false)
      await new Promise((r) => requestAnimationFrame(() => r(null)))
      setIsFilePickerCompact(false)
    }
  }, [addAttachments])
  
  const handleUndo = useCallback(async () => {
    const { addSystemMessage } = useSessionStore.getState()
    try {
      const results = await invoke<string[]>('undo_all')
      if (results.length === 0) {
        addSystemMessage('Nothing to undo.')
      } else {
        addSystemMessage('Undone:\n' + results.map((r) => `• ${r}`).join('\n'))
      }
    } catch (e: unknown) {
      addSystemMessage(`Undo failed: ${e instanceof Error ? e.message : String(e)}`)
    }
  }, [])
  
  const apiKey = useAgentStore((s) => s.apiKey)
  if (!apiKey) {
    return (
      <div className="w-screen h-screen flex items-center justify-center bg-transparent" data-tauri-drag-region>
        <div data-clui-ui className="w-[640px] bg-[#111111] rounded-2xl border border-white/[0.08] shadow-2xl flex flex-col overflow-hidden max-h-[500px]">
          <ApiKeyGate />
        </div>
      </div>
    )
  }

  return (
    <PopoverLayerProvider>
      <div className="flex flex-col justify-end h-full" style={{ background: 'transparent' }}>

        {/* ─── 460px content column, centered. Circles overflow left. ─── */}
        <div style={{ width: contentWidth, position: 'relative', margin: '0 auto', transition: 'width 0.26s cubic-bezier(0.4, 0, 0.1, 1)' }}>

          <AnimatePresence initial={false}>
            {marketplaceOpen && !isFilePickerCompact && (
              <div
                data-clui-ui
                style={{
                  width: 860,
                  maxWidth: 860,
                  marginLeft: '50%',
                  transform: 'translateX(-50%)',
                  marginBottom: 14,
                  position: 'relative',
                  zIndex: 30,
                }}
              >
                <motion.div
                  initial={{ opacity: 0, y: 14, scale: 0.98 }}
                  animate={{ opacity: 1, y: 0, scale: 1 }}
                  exit={{ opacity: 0, y: 10, scale: 0.985 }}
                  transition={TRANSITION}
                >
                  <div
                    data-clui-ui
                    className="glass-surface overflow-hidden no-drag"
                    style={{
                      borderRadius: 24,
                      maxHeight: 560,
                    }}
                  >
                    <MarketplacePanel />
                  </div>
                </motion.div>
              </div>
            )}
            {connectorsOpen && !isFilePickerCompact && (
              <div
                data-clui-ui
                style={{
                  width: 860,
                  maxWidth: 860,
                  marginLeft: '50%',
                  transform: 'translateX(-50%)',
                  marginBottom: 14,
                  position: 'relative',
                  zIndex: 30,
                }}
              >
                <motion.div
                  initial={{ opacity: 0, y: 14, scale: 0.98 }}
                  animate={{ opacity: 1, y: 0, scale: 1 }}
                  exit={{ opacity: 0, y: 10, scale: 0.985 }}
                  transition={TRANSITION}
                >
                  <div
                    data-clui-ui
                    className="glass-surface overflow-hidden no-drag"
                    style={{
                      borderRadius: 24,
                      maxHeight: 560,
                      overflowY: 'auto',
                    }}
                  >
                    <ConnectorsPanel />
                  </div>
                </motion.div>
              </div>
            )}
          </AnimatePresence>

          {/*
            ─── Tabs / message shell ───
            This always remains the chat shell. The marketplace is a separate
            panel rendered above it, never inside it.
          */}
          <motion.div
            data-clui-ui
            className="overflow-hidden flex flex-col"
            animate={{
              height: isFilePickerCompact ? 0 : 'auto',
              opacity: isFilePickerCompact ? 0 : 1,
              width: isExpanded ? cardExpandedWidth : cardCollapsedWidth,
              marginBottom: isExpanded ? 10 : -14,
              marginLeft: isExpanded ? 0 : cardCollapsedMargin,
              marginRight: isExpanded ? 0 : cardCollapsedMargin,
              background: isExpanded ? colors.containerBg : colors.containerBgCollapsed,
              borderColor: colors.containerBorder,
              boxShadow: isExpanded ? colors.cardShadow : colors.cardShadowCollapsed,
            }}
            transition={TRANSITION}
            style={{
              borderWidth: 1,
              borderStyle: 'solid',
              borderRadius: 20,
              position: 'relative',
              zIndex: isExpanded ? 20 : 10,
              pointerEvents: isFilePickerCompact ? 'none' : 'auto',
            }}
          >
            {/* Tab strip — always mounted */}
            <div className="no-drag">
              <TabStrip />
            </div>

            {/* Body — chat history only; the marketplace is a separate overlay above */}
            <motion.div
              initial={false}
              animate={{
                height: isExpanded ? 'auto' : 0,
                opacity: isExpanded ? 1 : 0,
              }}
              transition={TRANSITION}
              className="overflow-hidden no-drag"
            >
              <div style={{ maxHeight: bodyMaxHeight }}>
                <ConversationView />
                <StatusBar />
              </div>
            </motion.div>
          </motion.div>

          {/* ─── Input row — circles float outside left ─── */}
          {/* marginBottom: shadow buffer so the glass-surface drop shadow isn't clipped at the native window edge */}
          <div data-clui-ui className="relative" style={{ minHeight: 58, zIndex: 15, marginBottom: 10 }}>
            {/* Stacked circle buttons — expand on hover */}
            <div
              data-clui-ui
              className="circles-out"
            >
              <div className="btn-stack">
                {/* btn-1: Attach (front, rightmost) */}
                <button
                  className="stack-btn stack-btn-1 glass-surface"
                  title="Attach file"
                  onClick={handleAttachFile}
                  disabled={isRunning}
                >
                  <Paperclip size={20} />
                </button>
                {/* btn-2: Screenshot (middle) */}
                <button
                  className="stack-btn stack-btn-2 glass-surface"
                  title="Take screenshot"
                  onClick={handleScreenshot}
                  disabled={isRunning}
                >
                  <Camera size={20} />
                </button>
                {/* btn-3: Connectors */}
                <button
                  className="stack-btn stack-btn-3 glass-surface"
                  title="Connectors"
                  onClick={() => useSessionStore.getState().toggleConnectors()}
                  disabled={isRunning}
                >
                  <HeadCircuit size={20} />
                </button>
                {/* btn-4: Undo */}
                <button
                  className="stack-btn stack-btn-4 glass-surface"
                  title="Undo last action"
                  onClick={handleUndo}
                  disabled={isRunning}
                >
                  <ClockCounterClockwise size={20} />
                </button>
              </div>
            </div>

            {/* Plan approval panel — always mounted to receive events; renders only when a plan is pending */}
            <div style={{ marginBottom: pendingPlan ? 8 : 0 }}>
              <PlanApprovalPanel />
            </div>

            {/* Input pill */}
            <div
              data-clui-ui
              className="glass-surface w-full"
              style={{ minHeight: 62, borderRadius: 31, padding: '0 8px 0 20px', background: colors.inputPillBg }}
            >
              <InputBar />
            </div>
          </div>
        </div>
      </div>
    </PopoverLayerProvider>
  )
}
