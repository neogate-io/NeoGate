type AxisFormatter = (value: number) => string

export function chartGrid(bottom = 28, top = 28) {
  return {
    left: 12,
    right: 18,
    top,
    bottom,
    outerBoundsMode: 'same' as const,
    outerBoundsContain: 'axisLabel' as const
  }
}

export function categoryAxis(data: string[], options: { rotate?: number; width?: number } = {}) {
  return {
    type: 'category' as const,
    data,
    axisLabel: {
      color: '#667085',
      hideOverlap: options.rotate == null,
      ...(options.rotate == null
        ? {}
        : { rotate: options.rotate, width: options.width ?? 92, overflow: 'truncate' as const })
    }
  }
}

export function metricAxis(formatter?: AxisFormatter) {
  return {
    type: 'value' as const,
    axisLabel: { color: '#667085', ...(formatter ? { formatter } : {}) },
    splitLine: { lineStyle: { color: '#edf2f7' } }
  }
}

export function axisTooltip(formatter: (params: unknown) => string, shadow = false) {
  return {
    trigger: 'axis' as const,
    ...(shadow ? { axisPointer: { type: 'shadow' as const } } : {}),
    formatter
  }
}
