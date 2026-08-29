import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Clock3, LoaderCircle, X } from 'lucide-react'
import * as Dialog from 'radix-ui/dialog'
import { useState } from 'react'
import { toast } from 'sonner'
import { errorMessage, serversQueryKey, updateServerShutdown } from '../lib/api.ts'
import type { NutServerRecord } from '../lib/types.ts'

export function ShutdownConfigDialog({ server }: { server: NutServerRecord }) {
  const [open, setOpen] = useState(false)
  const [triggerMode, setTriggerMode] = useState(server.shutdown.trigger_mode)
  const [batteryLevel, setBatteryLevel] = useState(server.shutdown.battery_level_percent)
  const [onBatterySeconds, setOnBatterySeconds] = useState(server.shutdown.on_battery_seconds)
  const [hostSync, setHostSync] = useState(server.shutdown.host_sync_seconds)
  const [finalDelay, setFinalDelay] = useState(server.shutdown.final_delay_seconds)
  const [powerdown, setPowerdown] = useState(server.shutdown.powerdown_enabled)
  const queryClient = useQueryClient()
  const invalid = finalDelay > hostSync
    || batteryLevel < 5
    || batteryLevel > 50
    || onBatterySeconds < 60
    || onBatterySeconds > 7200
    || !Number.isInteger(onBatterySeconds)
  const save = useMutation({
    mutationFn: () => updateServerShutdown(server.id, {
      trigger_mode: triggerMode,
      battery_level_percent: batteryLevel,
      on_battery_seconds: Math.round(onBatterySeconds),
      host_sync_seconds: hostSync,
      final_delay_seconds: finalDelay,
      powerdown_enabled: powerdown,
    }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: serversQueryKey })
      setOpen(false)
      toast.success('关机配置已保存，请打开“应用配置”将变更写入远端主机')
    },
    onError: (error) => toast.error(errorMessage(error)),
  })

  const reset = () => {
    setTriggerMode(server.shutdown.trigger_mode)
    setBatteryLevel(server.shutdown.battery_level_percent)
    setOnBatterySeconds(server.shutdown.on_battery_seconds)
    setHostSync(server.shutdown.host_sync_seconds)
    setFinalDelay(server.shutdown.final_delay_seconds)
    setPowerdown(server.shutdown.powerdown_enabled)
  }

  return (
    <Dialog.Root open={open} onOpenChange={(nextOpen) => { if (nextOpen) reset(); setOpen(nextOpen) }}>
      <Dialog.Trigger asChild>
        <button type="button" className="inline-flex items-center gap-1.5 rounded-lg border border-cyan-300/15 px-2.5 py-2 text-xs text-cyan-700 transition hover:bg-cyan-300/[0.05]">
          <Clock3 size={14} /> 关机配置
        </button>
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/65 backdrop-blur-sm" />
        <Dialog.Content className="dialog-content fixed top-1/2 left-1/2 z-50 max-h-[92vh] w-[calc(100%-2rem)] max-w-3xl -translate-x-1/2 -translate-y-1/2 overflow-y-auto rounded-2xl border border-slate-200 bg-white p-6 shadow-2xl focus:outline-none">
          <div className="flex items-start justify-between gap-4">
            <div>
              <Dialog.Title className="text-lg font-semibold text-slate-900">关机配置</Dialog.Title>
              <Dialog.Description className="mt-1.5 text-xs text-slate-500">{server.ups_name} · 保存后需要在“应用配置”中确认写入远端主机</Dialog.Description>
            </div>
            <Dialog.Close asChild><button type="button" aria-label="关闭" className="rounded-lg p-1.5 text-slate-500 hover:bg-slate-100"><X size={18} /></button></Dialog.Close>
          </div>

          <section className="mt-6 rounded-xl border border-slate-200 bg-slate-100 p-4">
            <div className="flex items-start justify-between gap-4">
              <div>
                <h3 className="text-sm font-medium text-slate-800">自动关机触发条件</h3>
                <p className="mt-1 text-xs leading-5 text-slate-500">由 Server 本地 NUT 独立判断并通知全部 Client，Web 服务无需保持在线。</p>
              </div>
              <span className="rounded-full bg-emerald-300/10 px-2.5 py-1 text-[10px] font-medium text-emerald-600">已启用</span>
            </div>
            <div className="mt-4 grid gap-3 sm:grid-cols-2">
              <label className={`cursor-pointer rounded-xl border p-4 transition ${triggerMode === 'battery_level' ? 'border-cyan-300/30 bg-cyan-300/[0.06]' : 'border-slate-200 bg-slate-50'}`}>
                <span className="flex items-center gap-2 text-sm text-slate-800"><input type="radio" name={`shutdown-${server.id}`} checked={triggerMode === 'battery_level'} onChange={() => setTriggerMode('battery_level')} className="accent-cyan-300" />按电池电量</span>
                <span className="mt-3 flex items-center gap-2 text-xs text-slate-600">电量低于
                  <input type="number" min={5} max={50} value={batteryLevel} onChange={(event) => setBatteryLevel(Number(event.target.value))} className="w-20 rounded-lg border border-slate-200 bg-slate-50 px-2.5 py-2 text-center text-sm text-slate-800 outline-none" /> %
                </span>
                <span className="mt-2 block text-[11px] leading-5 text-slate-500">默认明确写入 20%，不沿用 UPS 固件默认值。</span>
              </label>
              <label className={`cursor-pointer rounded-xl border p-4 transition ${triggerMode === 'on_battery_timer' ? 'border-cyan-300/30 bg-cyan-300/[0.06]' : 'border-slate-200 bg-slate-50'}`}>
                <span className="flex items-center gap-2 text-sm text-slate-800"><input type="radio" name={`shutdown-${server.id}`} checked={triggerMode === 'on_battery_timer'} onChange={() => setTriggerMode('on_battery_timer')} className="accent-cyan-300" />按断电时长</span>
                <span className="mt-3 flex items-center gap-2 text-xs text-slate-600">市电断电持续
                  <input type="number" min={60} max={7200} step={1} value={onBatterySeconds} disabled={triggerMode !== 'on_battery_timer'} onChange={(event) => setOnBatterySeconds(Number(event.target.value))} className="w-24 rounded-lg border border-slate-200 bg-slate-50 px-2.5 py-2 text-center text-sm text-slate-800 outline-none disabled:opacity-40" /> 秒
                </span>
                <span className="mt-2 block text-[11px] leading-5 text-slate-500">允许 60–7200 秒；恢复市电会取消计时。电量先降到 {batteryLevel}% 时会提前紧急关机。</span>
              </label>
            </div>

            <details className="mt-4 rounded-lg border border-slate-200 bg-slate-50 p-3">
              <summary className="cursor-pointer text-xs font-medium text-slate-600">高级关机流程参数</summary>
              <div className="mt-4 grid gap-4 sm:grid-cols-2">
                <label className="text-xs text-slate-600">等待 Client 退出（秒）
                  <input type="number" min={5} max={300} value={hostSync} onChange={(event) => setHostSync(Number(event.target.value))} className="mt-2 w-full rounded-lg border border-slate-200 bg-slate-50 px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-cyan-300/40" />
                </label>
                <label className="text-xs text-slate-600">执行关机前延迟（秒）
                  <input type="number" min={0} max={120} value={finalDelay} onChange={(event) => setFinalDelay(Number(event.target.value))} className="mt-2 w-full rounded-lg border border-slate-200 bg-slate-50 px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-cyan-300/40" />
                </label>
              </div>
              <label className={`mt-4 flex cursor-pointer items-start gap-3 rounded-lg border p-3 text-xs ${powerdown ? 'border-rose-400/30 bg-rose-400/[0.07] text-rose-800' : 'border-slate-200 bg-slate-50 text-slate-600'}`}>
                <input type="checkbox" checked={powerdown} onChange={(event) => setPowerdown(event.target.checked)} className="mt-0.5 accent-cyan-300" />
                <span><span className={`block font-medium ${powerdown ? 'text-rose-700' : 'text-slate-700'}`}>高风险：关机末期切断 UPS 全部输出</span><span className={`mt-1 block leading-5 ${powerdown ? 'text-rose-600/80' : 'text-slate-500'}`}>这会让所有连接到该 UPS 的物理设备同时断电。NUT Server 运行在 VM/容器中时不要启用；关闭此项不影响 Server 和 Client 自身的安全关机。</span></span>
              </label>
            </details>

            {finalDelay > hostSync && <p className="mt-3 text-xs text-rose-600">最终延迟不能大于 Client 等待时间。</p>}
            <button type="button" disabled={save.isPending || invalid} onClick={() => save.mutate()} className="mt-5 inline-flex w-full items-center justify-center gap-2 rounded-xl bg-cyan-300 px-4 py-3 text-sm font-semibold text-cyan-950 hover:bg-cyan-200 disabled:opacity-60">
              {save.isPending && <LoaderCircle size={17} className="animate-spin" />} 保存关机配置
            </button>
          </section>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
