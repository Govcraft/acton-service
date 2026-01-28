import { type Metadata } from 'next'
import { Inter } from 'next/font/google'
import localFont from 'next/font/local'
import clsx from 'clsx'

import { Providers } from '@/app/providers'
import { Layout } from '@/components/Layout'

import '@/styles/tailwind.css'

const inter = Inter({
  subsets: ['latin'],
  display: 'swap',
  variable: '--font-inter',
})

// Use local version of Lexend so that we can use OpenType features
const lexend = localFont({
  src: '../fonts/lexend.woff2',
  display: 'swap',
  variable: '--font-lexend',
})

const siteUrl = 'https://govcraft.github.io/acton-service'

export const metadata: Metadata = {
  title: {
    template: '%s - acton-service',
    default: 'acton-service - Production-ready Rust backend framework',
  },
  description:
    'Build production backends with enforced best practices, dual HTTP+gRPC support, and comprehensive observability. Scales from monolith to microservices.',
  openGraph: {
    title: 'acton-service - Production-ready Rust backend framework',
    description:
      'Build production backends with enforced best practices, dual HTTP+gRPC support, and comprehensive observability. Scales from monolith to microservices.',
    url: siteUrl,
    siteName: 'acton-service',
    images: [
      {
        url: `${siteUrl}/og-image.png`,
        width: 722,
        height: 298,
        alt: 'acton-service - Production-ready Rust backends',
      },
    ],
    locale: 'en_US',
    type: 'website',
  },
  twitter: {
    card: 'summary_large_image',
    title: 'acton-service - Production-ready Rust backend framework',
    description:
      'Build production backends with enforced best practices, dual HTTP+gRPC support, and comprehensive observability. Scales from monolith to microservices.',
    images: [`${siteUrl}/og-image.png`],
  },
}

export default function RootLayout({
  children,
}: {
  children: React.ReactNode
}) {
  return (
    <html
      lang="en"
      className={clsx('h-full antialiased', inter.variable, lexend.variable)}
      suppressHydrationWarning
    >
      <body className="flex min-h-full bg-white dark:bg-slate-900">
        <Providers>
          <Layout>{children}</Layout>
        </Providers>
      </body>
    </html>
  )
}
