import { useEffect, useMemo, useState } from 'react'
import type { ReactNode } from 'react'
import { motion } from 'framer-motion'
import { invoke } from '@tauri-apps/api/core'
import {
  ArrowRight,
  Browser,
  Check,
  Folder,
  FolderOpen,
  HardDrives,
  MagicWand,
  Plus,
  ShieldCheck,
  Sparkle,
  Trash,
  WarningCircle,
  X,
} from '@phosphor-icons/react'
import { useColors } from '../theme'
import type { ScopeConfig } from '../types'

type Step = 'scope' | 'missions'

interface Mission {
  id: string
  title: string
  caption: string
  icon: ReactNode
  accent: string
  prompt: string
  expected: string
  preview: string[]
}

interface OnboardingExperienceProps {
  homePath?: string
  onComplete: () => void
}

const TRANSITION = { duration: 0.24, ease: [0.4, 0, 0.1, 1] as const }
const STARTER_PROMPT_KEY = 'ritual_onboarding_suggested_prompt'

const MISSIONS: Mission[] = [
  {
    id: 'downloads',
    title: 'Clean Downloads',
    caption: 'Turn messy folders into something usable.',
    icon: <Trash size={20} weight="duotone" />,
    accent: '#38bdf8',
    prompt: 'Clean my Downloads folder and organize files by type.',
    expected: 'A cleanup plan with file groups, proposed moves, risky items held for approval, and undo available after execution.',
    preview: ['Scan Downloads', 'Group by file type and age', 'Preview moves and deletes', 'Wait for approval'],
  },
  {
    id: 'mac',
    title: 'Fix My Mac',
    caption: 'Understand what is slowing the system down.',
    icon: <HardDrives size={20} weight="duotone" />,
    accent: '#f59e0b',
    prompt: 'Find why my Mac feels slow and tell me what to fix.',
    expected: 'A readable diagnosis of CPU, memory, disk, login items, and safe next actions before anything changes.',
    preview: ['Read safe diagnostics', 'Spot pressure points', 'Explain likely causes', 'Suggest fixes'],
  },
  {
    id: 'browser',
    title: 'Browser + Email',
    caption: 'Use browser context to create finished work.',
    icon: <Browser size={20} weight="duotone" />,
    accent: '#a78bfa',
    prompt: 'Open Gmail and draft a reply using the current webpage.',
    expected: 'A browser session with page context gathered and an email draft left ready for your review.',
    preview: ['Open browser', 'Read page context', 'Draft the email', 'Leave it reviewable'],
  },
  {
    id: 'project',
    title: 'Maintain a Folder',
    caption: 'Keep project folders clean without guessing.',
    icon: <FolderOpen size={20} weight="duotone" />,
    accent: '#34d399',
    prompt: 'Find large unused files and duplicate screenshots in this folder.',
    expected: 'A findings report, duplicate/large-file candidates, proposed cleanup, and approval before changes.',
    preview: ['Inspect folder', 'Find clutter', 'Show evidence', 'Run approved cleanup'],
  },
]

function basename(path: string): string {
  const trimmed = path.replace(/\/+$/, '')
  return trimmed.split('/').pop() || trimmed
}

function uniquePaths(paths: string[]): string[] {
  return Array.from(new Set(paths.filter(Boolean)))
}

function StepButton({
  active,
  done,
  index,
  title,
  subtitle,
  disabled,
  onClick,
}: {
  active: boolean
  done?: boolean
  index: number
  title: string
  subtitle: string
  disabled?: boolean
  onClick: () => void
}) {
  const colors = useColors()
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className="w-full flex items-center gap-3 rounded-2xl px-3 py-3 text-left transition-transform hover:scale-[1.01]"
      style={{
        background: active ? colors.accentLight : 'rgba(255,255,255,0.035)',
        border: `1px solid ${active ? colors.accentBorderMedium : colors.containerBorder}`,
        opacity: disabled ? 0.55 : 1,
      }}
    >
      <div
        className="w-8 h-8 rounded-xl flex items-center justify-center text-[12px] font-bold"
        style={{
          background: active || done ? colors.accent : colors.surfaceSecondary,
          color: active || done ? colors.textOnAccent : colors.textSecondary,
        }}
      >
        {done ? <Check size={14} weight="bold" /> : index}
      </div>
      <div className="min-w-0">
        <div className="text-[13px] font-semibold" style={{ color: colors.textPrimary }}>
          {title}
        </div>
        <div className="text-[11px] truncate" style={{ color: colors.textTertiary }}>
          {subtitle}
        </div>
      </div>
    </button>
  )
}

function TrustItem({ icon, label }: { icon: ReactNode; label: string }) {
  const colors = useColors()
  return (
    <div
      className="flex items-center gap-2 rounded-xl px-3 py-2 text-[12px]"
      style={{ background: 'rgba(255,255,255,0.035)', color: colors.textSecondary }}
    >
      <span style={{ color: colors.accent }}>{icon}</span>
      {label}
    </div>
  )
}

export function OnboardingExperience({ homePath = '~', onComplete }: OnboardingExperienceProps) {
  const colors = useColors()
  const [step, setStep] = useState<Step>('scope')
  const [scopeConfig, setScopeConfig] = useState<ScopeConfig | null>(null)
  const [selectedMissionId, setSelectedMissionId] = useState(MISSIONS[0].id)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const quickFolders = useMemo(() => {
    if (!homePath || homePath === '~') return ['~/Downloads', '~/Desktop', '~/Documents']
    return [`${homePath}/Downloads`, `${homePath}/Desktop`, `${homePath}/Documents`]
  }, [homePath])

  const selectedMission = MISSIONS.find((m) => m.id === selectedMissionId) || MISSIONS[0]
  const allowedPaths = scopeConfig?.allowed_paths || []

  useEffect(() => {
    invoke<ScopeConfig>('get_scope_config')
      .then(setScopeConfig)
      .catch((e) => setError(e instanceof Error ? e.message : String(e)))
  }, [])

  const saveScope = async (nextConfig: ScopeConfig) => {
    setScopeConfig(nextConfig)
    setSaving(true)
    setError(null)
    try {
      await invoke('set_scope_config', { config: nextConfig })
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  const togglePath = (path: string) => {
    if (!scopeConfig) return
    const exists = scopeConfig.allowed_paths.includes(path)
    const nextPaths = exists
      ? scopeConfig.allowed_paths.filter((p) => p !== path)
      : [...scopeConfig.allowed_paths, path]
    saveScope({
      ...scopeConfig,
      allowed_paths: nextPaths.length > 0 ? uniquePaths(nextPaths) : [path],
    })
  }

  const addFolder = async () => {
    if (!scopeConfig) return
    try {
      const dir = await invoke<string | null>('select_directory_command')
      if (!dir) return
      saveScope({
        ...scopeConfig,
        allowed_paths: uniquePaths([...scopeConfig.allowed_paths, dir]),
      })
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const removePath = (path: string) => {
    if (!scopeConfig || scopeConfig.allowed_paths.length <= 1) return
    saveScope({
      ...scopeConfig,
      allowed_paths: scopeConfig.allowed_paths.filter((p) => p !== path),
    })
  }

  const complete = async (starterPrompt: string = selectedMission.prompt) => {
    setSaving(true)
    setError(null)
    try {
      try {
        localStorage.setItem(STARTER_PROMPT_KEY, starterPrompt)
      } catch {}
      await invoke('complete_onboarding')
      onComplete()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
      setSaving(false)
    }
  }

  const statusText = error
    ? error
    : saving
      ? step === 'scope' ? 'Saving scope...' : 'Saving onboarding...'
      : step === 'scope' ? 'You can change this later from Settings.' : 'Ritual still asks before changing files or running risky actions.'

  return (
    <div
      data-clui-ui
      className="h-full w-full flex items-center justify-center px-5 py-3"
      style={{ background: 'transparent' }}
    >
      <motion.div
        initial={{ opacity: 0, y: 18, scale: 0.985 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        transition={TRANSITION}
        className="overflow-hidden"
        style={{
          width: 1100,
          maxWidth: 'calc(100vw - 40px)',
          height: 640,
          maxHeight: 'calc(100vh - 24px)',
          borderRadius: 30,
          border: `1px solid ${colors.containerBorder}`,
          background: colors.containerBg,
          boxShadow: '0 26px 80px rgba(0,0,0,0.36), 0 2px 12px rgba(0,0,0,0.18)',
          backdropFilter: 'blur(28px)',
          WebkitBackdropFilter: 'blur(28px)',
        }}
      >
        <div className="h-full grid" style={{ gridTemplateColumns: '340px 1fr' }}>
          <aside
            className="h-full flex flex-col p-6"
            style={{
              borderRight: `1px solid ${colors.containerBorder}`,
              background: colors.surfaceSecondary,
            }}
          >
            <div
              className="inline-flex items-center gap-2 rounded-full px-3 py-1.5 text-[11px] font-semibold self-start"
              style={{ background: colors.accentLight, color: colors.accent }}
            >
              <Sparkle size={13} weight="fill" />
              desktop actions, reviewed by you
            </div>

            <div className="mt-5 text-[30px] font-semibold leading-[1.04]" style={{ color: colors.textPrimary }}>
              Tell Ritual the outcome. It figures out the work.
            </div>
            <div className="text-[13px] leading-[1.55] mt-3" style={{ color: colors.textSecondary }}>
              Ritual can clean files, diagnose your Mac, operate a browser, and draft useful output. First, define where it is allowed to make file changes.
            </div>

            <div className="grid grid-cols-2 gap-2 mt-5">
              <TrustItem icon={<ShieldCheck size={14} weight="duotone" />} label="Scoped folders" />
              <TrustItem icon={<Check size={14} weight="bold" />} label="Plan approval" />
              <TrustItem icon={<MagicWand size={14} weight="duotone" />} label="Browser work" />
              <TrustItem icon={<Folder size={14} weight="duotone" />} label="File cleanup" />
            </div>

            <div className="mt-auto flex flex-col gap-2">
              <StepButton
                index={1}
                title="Set safe scope"
                subtitle={`${allowedPaths.length || 0} folder${allowedPaths.length === 1 ? '' : 's'} allowed`}
                active={step === 'scope'}
                done={allowedPaths.length > 0}
                onClick={() => setStep('scope')}
              />
              <StepButton
                index={2}
                title="Pick a mission"
                subtitle="See prompt and result"
                active={step === 'missions'}
                disabled={!scopeConfig}
                onClick={() => setStep('missions')}
              />
            </div>
          </aside>

          <main className="h-full min-w-0 p-6 flex flex-col">
            <div className="flex items-center justify-between gap-4">
              <div className="min-w-0">
                <div className="text-[11px] font-semibold uppercase" style={{ color: colors.accent, letterSpacing: '0.08em' }}>
                  {step === 'scope' ? 'Step 1 of 2' : 'Step 2 of 2'}
                </div>
                <div className="text-[24px] font-semibold mt-1" style={{ color: colors.textPrimary }}>
                  {step === 'scope' ? 'Choose where Ritual can change files' : 'Start from a real mission'}
                </div>
              </div>
              <div
                className="hidden sm:flex items-center gap-2 rounded-full px-3 py-2 text-[12px]"
                style={{ background: colors.surfaceSecondary, color: colors.textSecondary }}
              >
                <ShieldCheck size={15} style={{ color: '#34d399' }} />
                approval before action
              </div>
            </div>

            {step === 'scope' ? (
              <div className="flex-1 min-h-0 grid gap-4 mt-5" style={{ gridTemplateColumns: '1.1fr 0.9fr' }}>
                <section className="min-w-0 flex flex-col">
                  <div className="text-[13px] leading-[1.55]" style={{ color: colors.textSecondary }}>
                    File cleanup and maintenance stay inside these folders. Ritual can still explain problems, browse, and draft text elsewhere.
                  </div>

                  <div className="grid grid-cols-3 gap-3 mt-4">
                    {quickFolders.map((path) => {
                      const checked = allowedPaths.includes(path)
                      return (
                        <button
                          key={path}
                          type="button"
                          onClick={() => togglePath(path)}
                          disabled={!scopeConfig}
                          className="h-[104px] rounded-2xl px-4 py-3.5 text-left flex flex-col justify-between transition-transform hover:scale-[1.01]"
                          style={{
                            background: checked ? colors.accentLight : colors.surfaceSecondary,
                            border: `1px solid ${checked ? colors.accentBorderMedium : colors.containerBorder}`,
                            color: checked ? colors.accent : colors.textPrimary,
                          }}
                        >
                          <div className="flex items-center justify-between">
                            <Folder size={23} weight="duotone" />
                            <span
                              className="w-6 h-6 rounded-full flex items-center justify-center"
                              style={{ background: checked ? colors.accent : colors.containerBg, color: checked ? colors.textOnAccent : colors.textTertiary }}
                            >
                              {checked && <Check size={13} weight="bold" />}
                            </span>
                          </div>
                          <div className="min-w-0">
                            <div className="text-[13px] font-semibold truncate">{basename(path)}</div>
                            <div className="text-[11px] truncate mt-1" style={{ color: colors.textTertiary }}>
                              {path}
                            </div>
                          </div>
                        </button>
                      )
                    })}
                  </div>

                  <div className="mt-5 flex items-center justify-between">
                    <div className="text-[12px] font-semibold" style={{ color: colors.textPrimary }}>
                      Allowed folders
                    </div>
                    <button
                      type="button"
                      onClick={addFolder}
                      disabled={!scopeConfig || saving}
                      className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-[12px] font-semibold"
                      style={{ background: colors.accent, color: colors.textOnAccent, opacity: !scopeConfig || saving ? 0.6 : 1 }}
                    >
                      <Plus size={13} weight="bold" />
                      Add folder
                    </button>
                  </div>

                  <div className="flex flex-col gap-2 mt-3 overflow-y-auto pr-1" style={{ maxHeight: 155 }}>
                    {allowedPaths.map((path) => (
                      <div
                        key={path}
                        className="flex items-center gap-2 rounded-2xl px-3 py-2.5"
                        style={{ background: colors.surfaceSecondary, color: colors.textSecondary, border: `1px solid ${colors.containerBorder}` }}
                      >
                        <FolderOpen size={15} style={{ flexShrink: 0, color: colors.accent }} />
                        <span className="text-[12px] font-mono truncate flex-1">{path}</span>
                        {allowedPaths.length > 1 && (
                          <button
                            type="button"
                            onClick={() => removePath(path)}
                            className="w-6 h-6 rounded-full flex items-center justify-center"
                            style={{ color: colors.textTertiary }}
                            title="Remove folder"
                          >
                            <X size={12} />
                          </button>
                        )}
                      </div>
                    ))}
                  </div>
                </section>

                <section
                  className="rounded-3xl p-4 flex flex-col"
                  style={{ background: colors.surfaceSecondary, border: `1px solid ${colors.containerBorder}` }}
                >
                  <div className="flex items-center gap-3">
                    <div
                      className="w-10 h-10 rounded-2xl flex items-center justify-center"
                      style={{ background: colors.accentLight, color: colors.accent }}
                    >
                      <ShieldCheck size={21} weight="duotone" />
                    </div>
                    <div>
                      <div className="text-[15px] font-semibold" style={{ color: colors.textPrimary }}>
                        How safety feels
                      </div>
                      <div className="text-[11px]" style={{ color: colors.textTertiary }}>
                        transparent before execution
                      </div>
                    </div>
                  </div>

                  <div className="mt-4 flex flex-col gap-2.5">
                    {['Ritual understands your request', 'It prepares a visible plan', 'You approve or cancel', 'Undo is kept when available'].map((item, index) => (
                      <div key={item} className="flex items-center gap-3">
                        <div
                          className="w-7 h-7 rounded-full flex items-center justify-center text-[11px] font-bold"
                          style={{ background: colors.containerBg, color: colors.accent }}
                        >
                          {index + 1}
                        </div>
                        <div className="text-[12px]" style={{ color: colors.textSecondary }}>
                          {item}
                        </div>
                      </div>
                    ))}
                  </div>

                  <div className="mt-auto rounded-2xl p-4" style={{ background: colors.containerBg, border: `1px solid ${colors.containerBorder}` }}>
                    <div className="text-[12px] font-semibold" style={{ color: colors.textPrimary }}>
                      Example
                    </div>
                    <div className="text-[13px] leading-[1.45] mt-2" style={{ color: colors.textSecondary }}>
                      “Delete old installers from Downloads” becomes a reviewable plan, not a silent delete.
                    </div>
                  </div>
                </section>
              </div>
            ) : (
              <div className="flex-1 min-h-0 grid gap-4 mt-5" style={{ gridTemplateColumns: '280px 1fr' }}>
                <section className="flex flex-col gap-2 overflow-y-auto pr-1">
                  {MISSIONS.map((mission) => {
                    const active = mission.id === selectedMissionId
                    return (
                      <button
                        key={mission.id}
                        type="button"
                        onClick={() => setSelectedMissionId(mission.id)}
                        className="rounded-2xl px-4 py-3.5 text-left transition-transform hover:scale-[1.01]"
                        style={{
                          background: active ? `${mission.accent}18` : colors.surfaceSecondary,
                          border: `1px solid ${active ? `${mission.accent}66` : colors.containerBorder}`,
                          color: active ? mission.accent : colors.textPrimary,
                        }}
                      >
                        <div className="flex items-center gap-2.5">
                          {mission.icon}
                          <span className="text-[13px] font-semibold">{mission.title}</span>
                        </div>
                        <div className="text-[11px] leading-[1.45] mt-2" style={{ color: colors.textTertiary }}>
                          {mission.caption}
                        </div>
                      </button>
                    )
                  })}
                </section>

                <motion.section
                  key={selectedMission.id}
                  initial={{ opacity: 0, y: 8 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={TRANSITION}
                  className="rounded-3xl p-5 min-w-0 flex flex-col"
                  style={{
                    background: colors.surfaceSecondary,
                    border: `1px solid ${colors.containerBorder}`,
                  }}
                >
                  <div className="flex items-start justify-between gap-5">
                    <div className="flex items-center gap-3 min-w-0">
                      <div
                        className="w-12 h-12 rounded-2xl flex items-center justify-center flex-shrink-0"
                        style={{ background: `${selectedMission.accent}1f`, color: selectedMission.accent }}
                      >
                        {selectedMission.icon}
                      </div>
                      <div className="min-w-0">
                        <div className="text-[20px] font-semibold" style={{ color: colors.textPrimary }}>
                          {selectedMission.title}
                        </div>
                        <div className="text-[12px] mt-1" style={{ color: colors.textTertiary }}>
                          {selectedMission.caption}
                        </div>
                      </div>
                    </div>
                    <button
                      type="button"
                      onClick={() => complete(selectedMission.prompt)}
                      disabled={saving}
                      className="flex-shrink-0 flex items-center gap-2 px-4 py-2 rounded-xl text-[12px] font-semibold"
                      style={{ background: selectedMission.accent, color: '#ffffff', opacity: saving ? 0.65 : 1 }}
                    >
                      Start here
                      <ArrowRight size={14} weight="bold" />
                    </button>
                  </div>

                  <div className="grid gap-3 mt-4" style={{ gridTemplateColumns: '1fr 1fr' }}>
                    <div
                      className="rounded-2xl p-4 min-h-[132px]"
                      style={{ background: colors.containerBg, border: `1px solid ${colors.containerBorder}` }}
                    >
                      <div className="text-[11px] font-semibold uppercase" style={{ color: selectedMission.accent, letterSpacing: '0.08em' }}>
                        Try saying
                      </div>
                      <div className="text-[18px] leading-[1.3] mt-3" style={{ color: colors.textPrimary }}>
                        {selectedMission.prompt}
                      </div>
                    </div>
                    <div
                      className="rounded-2xl p-4 min-h-[132px]"
                      style={{ background: colors.containerBg, border: `1px solid ${colors.containerBorder}` }}
                    >
                      <div className="text-[11px] font-semibold uppercase" style={{ color: '#34d399', letterSpacing: '0.08em' }}>
                        You get
                      </div>
                      <div className="text-[13px] leading-[1.5] mt-3" style={{ color: colors.textSecondary }}>
                        {selectedMission.expected}
                      </div>
                    </div>
                  </div>

                  <div className="mt-3 rounded-2xl p-4" style={{ background: colors.containerBg, border: `1px solid ${colors.containerBorder}` }}>
                    <div className="flex items-center justify-between gap-3">
                      <div>
                        <div className="text-[13px] font-semibold" style={{ color: colors.textPrimary }}>
                          Plan preview
                        </div>
                        <div className="text-[11px] mt-1" style={{ color: colors.textTertiary }}>
                          This is the mental model users should expect.
                        </div>
                      </div>
                      <div className="rounded-full px-2.5 py-1 text-[11px] font-semibold" style={{ background: `${selectedMission.accent}18`, color: selectedMission.accent }}>
                        review first
                      </div>
                    </div>

                    <div className="grid grid-cols-4 gap-3 mt-4">
                      {selectedMission.preview.map((item, index) => (
                        <div key={item} className="min-w-0">
                          <div
                            className="h-[84px] rounded-2xl p-3 flex flex-col justify-between"
                            style={{ background: colors.surfaceSecondary, border: `1px solid ${index === selectedMission.preview.length - 1 ? `${selectedMission.accent}66` : colors.containerBorder}` }}
                          >
                            <div
                              className="w-6 h-6 rounded-full flex items-center justify-center text-[11px] font-bold"
                              style={{ background: `${selectedMission.accent}22`, color: selectedMission.accent }}
                            >
                              {index + 1}
                            </div>
                            <div className="text-[12px] leading-[1.28] font-medium" style={{ color: colors.textPrimary }}>
                              {item}
                            </div>
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>

                  <div className="mt-auto flex items-center justify-between gap-4 pt-3">
                    <div className="min-w-0 text-[12px]" style={{ color: error ? '#ef4444' : colors.textTertiary }}>
                      {error ? (
                        <span className="inline-flex items-center gap-1.5">
                          <WarningCircle size={14} />
                          {error}
                        </span>
                      ) : statusText}
                    </div>
                    <button
                      type="button"
                      onClick={() => complete()}
                      disabled={saving}
                      className="flex items-center gap-2 px-4 py-2 rounded-xl text-[12px] font-semibold"
                      style={{ background: colors.accent, color: colors.textOnAccent, opacity: saving ? 0.65 : 1 }}
                    >
                      Enter Ritual
                      <ArrowRight size={14} weight="bold" />
                    </button>
                  </div>
                </motion.section>
              </div>
            )}

            {step === 'scope' && (
              <div className="mt-auto pt-4 flex items-center justify-between gap-3">
                <div className="min-h-[18px] text-[12px]" style={{ color: error ? '#ef4444' : colors.textTertiary }}>
                  {error ? (
                    <span className="inline-flex items-center gap-1.5">
                      <WarningCircle size={14} />
                      {statusText}
                    </span>
                  ) : statusText}
                </div>
                <button
                  type="button"
                  onClick={() => setStep('missions')}
                  disabled={!scopeConfig || saving}
                  className="flex items-center gap-2 px-5 py-2.5 rounded-xl text-[13px] font-semibold"
                  style={{
                    background: colors.accent,
                    color: colors.textOnAccent,
                    opacity: !scopeConfig || saving ? 0.6 : 1,
                  }}
                >
                  Continue
                  <ArrowRight size={15} weight="bold" />
                </button>
              </div>
            )}
          </main>
        </div>
      </motion.div>
    </div>
  )
}
