import { FileCode2 } from 'lucide-react'
import type { ConfigPreview } from '../lib/types.ts'

export function ConfigPreviewPanel({ title, preview }: { title: string; preview: ConfigPreview }) {
  const conflicts = new Map(preview.conflicts.map((conflict) => [conflict.path, conflict.reason]))
  return (
    <section>
      <div className="flex items-center justify-between gap-3">
        <h3 className="text-xs font-semibold text-slate-700">{title}</h3>
        <span className="rounded-full border border-slate-200 px-2 py-1 text-[10px] text-slate-500">
          {ownershipLabel(preview.ownership)}
        </span>
      </div>
      <div className="mt-3 space-y-2">
        {preview.files.map((file) => (
          <details key={file.path} className="group rounded-xl border border-slate-200 bg-slate-50">
            <summary className="flex cursor-pointer list-none items-center gap-2 px-3.5 py-3 text-xs text-slate-700">
              <FileCode2 size={14} className="text-cyan-700" />
              <code className="font-mono">{file.path}</code>
              {conflicts.has(file.path) && <span className="ml-auto rounded-full bg-amber-300/10 px-2 py-0.5 text-[10px] text-amber-700">需要接管</span>}
            </summary>
            {conflicts.has(file.path) && <p className="border-t border-amber-300/10 bg-amber-300/[0.04] px-3.5 py-2 text-[10px] text-amber-700/80">{conflicts.get(file.path)}</p>}
            <div className="grid border-t border-slate-200 lg:grid-cols-2">
              <ConfigBlock label="当前" value={file.current || '(文件为空或不存在)'} />
              <ConfigBlock label="应用后" value={file.candidate} candidate />
            </div>
          </details>
        ))}
      </div>
      <p className="mt-3 text-[11px] text-slate-600">预览包含实际 NUT 凭据 · 将重启：{preview.services.join('、')}</p>
    </section>
  )
}

function ownershipLabel(ownership: ConfigPreview['ownership']) {
  return ({
    distribution_default: '发行版未配置',
    managed_unchanged: 'NWM 配置未变化',
    unmanaged_existing: '已有 NUT 配置',
    managed_modified: 'NWM 配置已被修改',
  } as const)[ownership]
}

function ConfigBlock({ label, value, candidate = false }: { label: string; value: string; candidate?: boolean }) {
  return (
    <div className={`min-w-0 p-3 ${candidate ? 'border-t border-slate-200 bg-cyan-300/[0.025] lg:border-t-0 lg:border-l' : ''}`}>
      <p className="mb-2 text-[10px] font-medium tracking-wider text-slate-600 uppercase">{label}</p>
      <pre className="max-h-52 overflow-auto whitespace-pre-wrap font-mono text-[10px] leading-4 text-slate-600">{value}</pre>
    </div>
  )
}
