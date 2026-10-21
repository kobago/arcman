import { useId } from 'react'

const LETTERS: [number, string][] = [
  [0, 'M0 100L35 0H55L90 100H68L61 78H29L22 100Z M35 60H55L45 30Z'],
  [100, 'M0 0H58L78 20V40L62 54L78 100H56L42 60H20V100H0Z M20 18H52L58 24V36L52 42H20Z'],
  [188, 'M20 0H76V20H26L20 26V74L26 80H76V100H20L0 80V20Z'],
  [274, 'M0 100V0H24L52 50L80 0H104V100H84V36L58 82H46L20 36V100Z'],
  [388, 'M0 100L35 0H55L90 100H68L61 78H29L22 100Z M35 60H55L45 30Z'],
  [488, 'M0 100V0H22L64 64V0H84V100H62L20 36V100Z'],
]

export function Monogram({ size = 26, ground = 'var(--panel)', metal = false }: { size?: number; ground?: string; metal?: boolean }) {
  const id = useId()
  const fill = metal ? `url(#${CSS.escape(id)})` : 'currentColor'
  const knockout = size < 40 ? 12 : 10
  return (
    <svg width={size} height={(size * 220) / 240} viewBox="0 0 240 220" aria-hidden="true" className="monogram">
      {metal && (
        <defs>
          <linearGradient id={id} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" style={{ stopColor: 'var(--metal-hi)' }} />
            <stop offset="0.5" style={{ stopColor: 'var(--metal-hi)' }} />
            <stop offset="0.5" style={{ stopColor: 'var(--metal-lo)' }} />
            <stop offset="1" style={{ stopColor: 'var(--metal-mid)' }} />
          </linearGradient>
        </defs>
      )}
      <path fillRule="evenodd" fill={fill} d="M30 40H200V210H30Z M46 56V194H184V56Z" />
      <path
        fillRule="evenodd"
        fill={fill}
        stroke={ground}
        strokeWidth={knockout}
        paintOrder="stroke"
        d="M54 210L104 10H128L178 210H149L139 170H93L83 210Z M99 144H133L116 74Z"
      />
      <path fill="var(--accent)" stroke={ground} strokeWidth={knockout} paintOrder="stroke" d="M84 144H236L221 160L236 176H84Z" />
    </svg>
  )
}

export function Wordmark({ height = 15, metal = false }: { height?: number; metal?: boolean }) {
  const id = useId()
  return (
    <svg width={(height * 572) / 100} height={height} viewBox="0 0 572 100" aria-hidden="true">
      {metal && (
        <defs>
          <linearGradient id={id} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" style={{ stopColor: 'var(--metal-hi)' }} />
            <stop offset="0.5" style={{ stopColor: 'var(--metal-hi)' }} />
            <stop offset="0.5" style={{ stopColor: 'var(--metal-lo)' }} />
            <stop offset="1" style={{ stopColor: 'var(--metal-mid)' }} />
          </linearGradient>
        </defs>
      )}
      <g fill={metal ? `url(#${CSS.escape(id)})` : 'currentColor'} fillRule="evenodd">
        {LETTERS.map(([x, d]) => (
          <path key={x} transform={`translate(${x} 0)`} d={d} />
        ))}
      </g>
    </svg>
  )
}

export function Logo() {
  return (
    <span className="logo" role="img" aria-label="ARCMAN">
      <Monogram />
      <Wordmark />
    </span>
  )
}
