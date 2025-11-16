export function Logomark(props: React.ComponentPropsWithoutRef<'svg'>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 36 36" fill="none" {...props}>
      <rect x="3" y="3" width="30" height="30" rx="6" fill="#0ea5e9" />
      <text
        x="18"
        y="24"
        textAnchor="middle"
        fill="white"
        fontSize="20"
        fontWeight="bold"
        fontFamily="system-ui, sans-serif"
      >
        A
      </text>
    </svg>
  )
}

export function Logo(props: React.ComponentPropsWithoutRef<'svg'>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 280 36" fill="none" {...props}>
      <rect x="0" y="3" width="30" height="30" rx="6" fill="#0ea5e9" />
      <text
        x="15"
        y="24"
        textAnchor="middle"
        fill="white"
        fontSize="20"
        fontWeight="bold"
        fontFamily="system-ui, sans-serif"
      >
        A
      </text>
      <text
        x="45"
        y="24"
        fill="currentColor"
        fontSize="20"
        fontWeight="600"
        fontFamily="system-ui, sans-serif"
      >
        acton-service
      </text>
    </svg>
  )
}
