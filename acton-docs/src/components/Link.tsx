export function Link({
  href,
  baseUrl,
  path,
  title,
  children,
}: {
  href?: string
  baseUrl?: string
  path?: string
  title?: string
  children: React.ReactNode
}) {
  // Support either direct href or baseUrl + path combination
  const finalHref = href || (baseUrl && path ? baseUrl + path : baseUrl || path)

  return (
    <a href={finalHref} title={title} target="_blank" rel="noopener noreferrer">
      {children}
    </a>
  )
}
