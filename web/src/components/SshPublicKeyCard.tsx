import { useQuery } from '@tanstack/react-query'
import { Check, Copy, KeyRound } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import {
  errorMessage,
  getSshPublicKey,
  sshPublicKeyQueryKey,
} from '../lib/api.ts'

export function SshPublicKeyCard() {
  const [copied, setCopied] = useState(false)
  const publicKey = useQuery({
    queryKey: sshPublicKeyQueryKey,
    queryFn: getSshPublicKey,
    staleTime: Number.POSITIVE_INFINITY,
  })

  async function copyInstallCommand() {
    if (!publicKey.data) return
    const command = authorizedKeysCommand(publicKey.data.public_key)
    try {
      if (navigator.clipboard && window.isSecureContext) {
        await navigator.clipboard.writeText(command)
      } else if (!fallbackCopy(command)) {
        throw new Error('clipboard unavailable')
      }
      setCopied(true)
      toast.success('authorized_keys 安装命令已复制')
      window.setTimeout(() => setCopied(false), 1800)
    } catch {
      toast.error('浏览器无法访问剪贴板，请手动复制')
    }
  }

  return (
    <section className="mt-8 rounded-2xl border border-slate-200 bg-white p-4 shadow-sm lg:p-5">
      <div className="flex flex-col gap-4 lg:flex-row lg:items-center">
        <div className="flex min-w-52 items-center gap-3">
          <div className="grid size-10 shrink-0 place-items-center rounded-xl bg-emerald-50 text-emerald-600">
            <KeyRound size={18} />
          </div>
          <div>
            <h2 className="text-sm font-medium text-slate-900">SSH 公钥</h2>
            <p className="mt-1 text-xs text-slate-500">复制命令并在目标 root Shell 执行</p>
          </div>
        </div>

        {publicKey.isError ? (
          <p className="min-w-0 flex-1 text-xs text-rose-600">{errorMessage(publicKey.error)}</p>
        ) : <div className="min-w-0 flex-1" />}

        <button
          type="button"
          disabled={!publicKey.data}
          onClick={() => void copyInstallCommand()}
          className="inline-flex shrink-0 items-center justify-center gap-2 rounded-xl bg-slate-900 px-3.5 py-2.5 text-xs font-medium text-white transition hover:bg-slate-800 disabled:opacity-40"
        >
          {copied ? <Check size={15} className="text-emerald-600" /> : <Copy size={15} />}
          {copied ? '已复制' : '复制命令'}
        </button>
      </div>
    </section>
  )
}

function authorizedKeysCommand(publicKey: string): string {
  return `install -d -m 700 ~/.ssh
touch ~/.ssh/authorized_keys
chmod 600 ~/.ssh/authorized_keys
tmp="$(mktemp ~/.ssh/authorized_keys.nwm.XXXXXX)"
awk '!/[[:space:]]nut-web-manager([[:space:]]|$)/' ~/.ssh/authorized_keys > "$tmp"
printf '%s\\n' '${publicKey.trim()}' >> "$tmp"
chmod 600 "$tmp"
mv "$tmp" ~/.ssh/authorized_keys`
}

function fallbackCopy(value: string): boolean {
  const textarea = document.createElement('textarea')
  textarea.value = value
  textarea.style.position = 'fixed'
  textarea.style.opacity = '0'
  document.body.appendChild(textarea)
  textarea.select()
  const copied = document.execCommand('copy')
  textarea.remove()
  return copied
}
