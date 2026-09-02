import { LineChart, type LineSeriesOption } from 'echarts/charts'
import {
  DataZoomComponent,
  GridComponent,
  LegendComponent,
  TooltipComponent,
  type DataZoomComponentOption,
  type GridComponentOption,
  type LegendComponentOption,
  type TooltipComponentOption,
} from 'echarts/components'
import * as echarts from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { useEffect, useRef } from 'react'

echarts.use([LineChart, GridComponent, TooltipComponent, LegendComponent, DataZoomComponent, CanvasRenderer])

type ChartOption = echarts.ComposeOption<
  LineSeriesOption | GridComponentOption | TooltipComponentOption | LegendComponentOption | DataZoomComponentOption
>

export interface TimeSeries {
  name: string
  color: string
  points: Array<[string, number | null]>
}

interface TimeSeriesChartProps {
  title: string
  series: TimeSeries[]
  unit: string
  valueFormatter?: (value: number) => string
  emptyLabel?: string
}

export function TimeSeriesChart({
  title,
  series,
  unit,
  valueFormatter = (value) => `${Math.round(value * 10) / 10}${unit}`,
  emptyLabel = '暂无历史数据',
}: TimeSeriesChartProps) {
  const container = useRef<HTMLDivElement>(null)
  const chart = useRef<echarts.ECharts | null>(null)
  const hasData = series.some((item) => item.points.some(([, value]) => value !== null))

  useEffect(() => {
    if (!container.current) return
    const instance = echarts.init(container.current, undefined, { renderer: 'canvas' })
    chart.current = instance
    const observer = new ResizeObserver(() => instance.resize())
    observer.observe(container.current)
    return () => {
      observer.disconnect()
      instance.dispose()
      chart.current = null
    }
  }, [hasData])

  useEffect(() => {
    const instance = chart.current
    if (!instance || !hasData) return
    const option: ChartOption = {
      animation: false,
      color: series.map((item) => item.color),
      grid: { top: 42, right: 18, bottom: 38, left: 54 },
      legend: {
        top: 0,
        right: 0,
        itemWidth: 10,
        itemHeight: 6,
        textStyle: { color: '#64748b', fontSize: 10 },
      },
      tooltip: {
        trigger: 'axis',
        confine: true,
        valueFormatter: (value) => typeof value === 'number' ? valueFormatter(value) : '—',
      },
      xAxis: {
        type: 'time',
        axisLine: { lineStyle: { color: '#cbd5e1' } },
        axisLabel: { color: '#64748b', fontSize: 10, hideOverlap: true },
        splitLine: { show: false },
      },
      yAxis: {
        type: 'value',
        scale: true,
        axisLabel: { color: '#64748b', fontSize: 10, formatter: `{value}${unit}` },
        splitLine: { lineStyle: { color: '#e2e8f0' } },
      },
      dataZoom: [{ type: 'inside', filterMode: 'none', zoomOnMouseWheel: 'shift' }],
      series: series.map((item) => ({
        name: item.name,
        type: 'line',
        showSymbol: false,
        connectNulls: false,
        sampling: 'lttb',
        lineStyle: { width: 2 },
        emphasis: { focus: 'series' },
        data: item.points
          .map(([timestamp, value]) => [Date.parse(timestamp), value] as [number, number | null])
          .sort((left, right) => left[0] - right[0]),
      })),
    }
    instance.setOption(option, { notMerge: true })
  }, [hasData, series, unit, valueFormatter])

  return (
    <article className="rounded-xl border border-slate-200 p-4">
      <h4 className="text-xs font-semibold text-slate-700">{title}</h4>
      {hasData
        ? <div ref={container} className="mt-3 h-52 w-full" role="img" aria-label={`${title}趋势图`} />
        : <div className="grid h-52 place-items-center text-xs text-slate-500">{emptyLabel}</div>}
    </article>
  )
}
