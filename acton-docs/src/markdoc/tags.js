import { Callout } from '@/components/Callout'
import { QuickLink, QuickLinks } from '@/components/QuickLinks'

const tags = {
  link: {
    attributes: {
      href: { type: String },
      baseUrl: { type: String },
      path: { type: String },
      title: { type: String },
    },
    render: ({ href, baseUrl, path, title, children }) => {
      // Support either direct href or baseUrl + path combination
      const finalHref = href || (baseUrl && path ? baseUrl + path : baseUrl || path)

      return (
        <a href={finalHref} title={title}>
          {children}
        </a>
      )
    },
  },
  callout: {
    attributes: {
      title: { type: String },
      type: {
        type: String,
        default: 'note',
        matches: ['note', 'warning'],
        errorLevel: 'critical',
      },
    },
    render: Callout,
  },
  figure: {
    selfClosing: true,
    attributes: {
      src: { type: String },
      alt: { type: String },
      caption: { type: String },
    },
    render: ({ src, alt = '', caption }) => (
      <figure>
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img src={src} alt={alt} />
        <figcaption>{caption}</figcaption>
      </figure>
    ),
  },
  'quick-links': {
    render: QuickLinks,
  },
  'quick-link': {
    selfClosing: true,
    render: QuickLink,
    attributes: {
      title: { type: String },
      description: { type: String },
      icon: { type: String },
      href: { type: String },
    },
  },
}

export default tags
