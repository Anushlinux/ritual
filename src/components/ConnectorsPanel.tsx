import React, { useEffect } from 'react'
import { ArrowClockwise, Check, GithubLogo, GoogleLogo, Plug, PlugsConnected, ShieldCheck, X } from '@phosphor-icons/react'
import { useSessionStore } from '../stores/sessionStore'
import { useColors } from '../theme'
import type { ConnectorInfo, ConnectorProvider } from '../types'

const providerIcon: Record<ConnectorProvider, React.ReactNode> = {
  google: <GoogleLogo size={16} />,
  github: <GithubLogo size={16} />,
}

function ConnectorCard({ connector }: { connector: ConnectorInfo }) {
  const colors = useColors()
  const connect = useSessionStore((s) => s.connectConnector)
  const disconnect = useSessionStore((s) => s.disconnectConnector)
  const loading = useSessionStore((s) => s.connectorsLoading)
  const tools = useSessionStore((s) => s.connectorTools.filter((t) => t.provider === connector.provider))
  const connected = connector.status === 'connected'

  return (
    <div
      className="rounded-xl p-3"
      style={{
        background: colors.surfacePrimary,
        border: `1px solid ${connected ? colors.accentBorderMedium : colors.containerBorder}`,
      }}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex items-start gap-2 min-w-0">
          <div
            className="w-8 h-8 rounded-full flex items-center justify-center flex-shrink-0"
            style={{
              color: connected ? colors.accent : colors.textTertiary,
              background: connected ? colors.accentLight : colors.surfaceSecondary,
            }}
          >
            {providerIcon[connector.provider]}
          </div>
          <div className="min-w-0">
            <div className="flex items-center gap-1.5">
              <div className="text-[13px] font-semibold truncate" style={{ color: colors.textPrimary }}>
                {connector.name}
              </div>
              <span
                className="text-[9px] rounded-full px-1.5 py-[1px]"
                style={{
                  color: connected ? colors.statusComplete : colors.textTertiary,
                  background: connected ? colors.statusCompleteBg : colors.surfaceSecondary,
                  border: `1px solid ${connected ? colors.permissionAllowBorder : colors.containerBorder}`,
                }}
              >
                {connected ? 'Connected' : 'Disconnected'}
              </span>
            </div>
            <div className="text-[11px] leading-[1.4] mt-1" style={{ color: colors.textSecondary }}>
              {connector.message || (connected ? 'Ready for actions.' : 'Connect to enable actions.')}
            </div>
          </div>
        </div>

        <button
          type="button"
          disabled={loading}
          onClick={() => connected ? disconnect(connector.provider) : connect(connector.provider)}
          className="text-[11px] font-medium rounded-full px-2.5 py-1 transition-colors flex items-center gap-1 disabled:opacity-50"
          style={{
            color: connected ? colors.textSecondary : colors.textOnAccent,
            background: connected ? colors.surfaceSecondary : colors.accent,
            border: `1px solid ${connected ? colors.containerBorder : colors.accent}`,
          }}
        >
          {connected ? <X size={10} /> : <Plug size={10} />}
          {connected ? 'Disconnect' : 'Connect'}
        </button>
      </div>

      <div className="mt-3 grid gap-1.5">
        <div className="flex items-center gap-1.5 text-[10px] uppercase tracking-wider" style={{ color: colors.textTertiary }}>
          <ShieldCheck size={11} />
          Scopes
        </div>
        <div className="flex flex-wrap gap-1">
          {connector.scopes.map((scope) => (
            <span
              key={scope}
              className="text-[10px] rounded-full px-2 py-0.5"
              style={{ color: colors.textSecondary, background: colors.surfaceSecondary }}
            >
              {scope}
            </span>
          ))}
        </div>
      </div>

      <div className="mt-3 grid gap-1.5">
        <div className="flex items-center gap-1.5 text-[10px] uppercase tracking-wider" style={{ color: colors.textTertiary }}>
          <PlugsConnected size={11} />
          Tools
        </div>
        <div className="flex flex-wrap gap-1">
          {tools.slice(0, 6).map((tool) => (
            <span
              key={tool.id}
              className="text-[10px] rounded-full px-2 py-0.5"
              style={{
                color: tool.risk === 'dangerous' ? colors.statusPermission : colors.textSecondary,
                background: tool.risk === 'dangerous' ? colors.permissionHeaderBg : colors.surfaceSecondary,
              }}
              title={tool.description}
            >
              {tool.name.replace(`${connector.provider}_`, '')}
            </span>
          ))}
          {tools.length > 6 && (
            <span className="text-[10px] rounded-full px-2 py-0.5" style={{ color: colors.textTertiary, background: colors.surfaceSecondary }}>
              +{tools.length - 6}
            </span>
          )}
        </div>
      </div>
    </div>
  )
}

export function ConnectorsPanel() {
  const colors = useColors()
  const connectors = useSessionStore((s) => s.connectors)
  const loading = useSessionStore((s) => s.connectorsLoading)
  const error = useSessionStore((s) => s.connectorsError)
  const notice = useSessionStore((s) => s.connectorsNotice)
  const loadConnectors = useSessionStore((s) => s.loadConnectors)

  useEffect(() => {
    loadConnectors()
  }, [loadConnectors])

  return (
    <div className="p-4" style={{ color: colors.textPrimary }}>
      <div className="flex items-center justify-between gap-3 mb-3">
        <div className="flex items-center gap-2 min-w-0">
          <div
            className="w-8 h-8 rounded-full flex items-center justify-center"
            style={{ background: colors.accentLight, color: colors.accent }}
          >
            <PlugsConnected size={16} />
          </div>
          <div className="min-w-0">
            <div className="text-[14px] font-semibold">Connectors</div>
            <div className="text-[11px]" style={{ color: colors.textSecondary }}>
              Google and GitHub actions through MCP-style tools
            </div>
          </div>
        </div>
        <button
          type="button"
          onClick={() => loadConnectors()}
          disabled={loading}
          className="w-7 h-7 rounded-full flex items-center justify-center transition-colors disabled:opacity-50"
          style={{ color: colors.textTertiary, background: colors.surfacePrimary }}
          title="Refresh connectors"
        >
          {loading ? <ArrowClockwise size={14} className="animate-spin" /> : <ArrowClockwise size={14} />}
        </button>
      </div>

      {error && (
        <div className="text-[11px] rounded-lg px-3 py-2 mb-3" style={{ color: colors.statusError, background: colors.statusErrorBg }}>
          {error}
        </div>
      )}

      {notice && !error && (
        <div className="text-[11px] rounded-lg px-3 py-2 mb-3" style={{ color: colors.textSecondary, background: colors.surfacePrimary, border: `1px solid ${colors.containerBorder}` }}>
          {notice}
        </div>
      )}

      <div className="grid gap-3">
        {connectors.map((connector) => (
          <ConnectorCard key={connector.provider} connector={connector} />
        ))}
      </div>

      <div className="mt-3 flex items-start gap-1.5 text-[11px] leading-[1.45]" style={{ color: colors.textTertiary }}>
        <Check size={12} className="flex-shrink-0 mt-[2px]" />
        Writes such as sending email, creating events, and GitHub changes require preview approval.
      </div>
    </div>
  )
}
