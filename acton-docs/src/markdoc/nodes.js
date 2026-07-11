import { nodes as defaultNodes, Tag } from '@markdoc/markdoc'
import { slugifyWithCounter } from '@sindresorhus/slugify'
import yaml from 'js-yaml'

import { DocsLayout } from '@/components/DocsLayout'
import { Fence } from '@/components/Fence'

let documentSlugifyMap = new Map()

const nodes = {
  document: {
    ...defaultNodes.document,
    render: DocsLayout,
    transform(node, config) {
      documentSlugifyMap.set(config, slugifyWithCounter())

      return new Tag(
        this.render,
        {
          frontmatter: yaml.load(node.attributes.frontmatter),
          nodes: node.children,
        },
        node.transformChildren(config),
      )
    },
  },
  heading: {
    ...defaultNodes.heading,
    transform(node, config) {
      let slugify = documentSlugifyMap.get(config)
      let attributes = node.transformAttributes(config)
      let children = node.transformChildren(config)
      let text = children.filter((child) => typeof child === 'string').join(' ')
      let id = attributes.id ?? slugify(text)

      return new Tag(
        `h${node.attributes.level}`,
        { ...attributes, id },
        children,
      )
    },
  },
  th: {
    ...defaultNodes.th,
    attributes: {
      ...defaultNodes.th.attributes,
      scope: {
        type: String,
        default: 'col',
      },
    },
  },
  fence: {
    render: Fence,
    attributes: {
      language: {
        type: String,
      },
      content: {
        type: String,
      },
    },
    transform(node, config) {
      const attributes = node.transformAttributes(config)

      // Use the fence's full raw body. Markdoc splits fence content into
      // several children as soon as it encounters a `{% ... %}` tag, so
      // reading `node.children[0]` would silently drop everything from the
      // first tag onward -- e.g. a ```toml fence containing
      // `acton-service = { version = "{% version() %}", features = [...] }`
      // used to render truncated at the opening quote.
      let content = node.attributes?.content ?? ''

      // Markdoc does not evaluate tags, variables, or functions inside fenced
      // code blocks -- their content is literal text. Code samples still need
      // the current crate version and dependency snippets, so interpolate the
      // supported forms here by hand.
      const version = config.variables?.version?.acton
      const deps = config.variables?.dep ?? {}

      if (version) {
        // Legacy handlebars form, kept for backwards compatibility.
        content = content.replace(/\{\{version\}\}/g, version)
        // {% version() %} and {% $version.acton %}
        content = content.replace(/\{%\s*version\(\)\s*%\}/g, version)
        content = content.replace(/\{%\s*\$version\.acton\s*%\}/g, version)
      }

      // {% $dep.<alias> %} -- variable form
      content = content.replace(
        /\{%\s*\$dep\.([A-Za-z0-9_]+)\s*%\}/g,
        (match, alias) => deps[alias] ?? match,
      )

      // {% dep("<alias>") %} -- function form, single alias
      content = content.replace(
        /\{%\s*dep\(\s*["']([A-Za-z0-9_-]+)["']\s*\)\s*%\}/g,
        (match, alias) => deps[alias] ?? match,
      )

      // {% dep(["<feature>", "<feature>"]) %} -- function form, literal
      // feature list (mirrors the array branch of `dep` in config.js).
      content = content.replace(
        /\{%\s*dep\(\s*\[([^\]]*)\]\s*\)\s*%\}/g,
        (match, list) => {
          if (!version) return match
          const features = list
            .split(',')
            .map((f) => f.trim().replace(/^["']|["']$/g, ''))
            .filter(Boolean)
          if (!features.length) return match
          const rendered = features.map((f) => `"${f}"`).join(', ')
          return `acton-service = { version = "${version}", features = [${rendered}] }`
        },
      )

      return new Tag(this.render, attributes, [content])
    },
  },
  link: {
    ...defaultNodes.link,
    transform(node, config) {
      const children = node.transformChildren(config)

      // Process href before transformAttributes to handle variable interpolation
      let href = node.attributes.href

      if (href && typeof href === 'string') {
        // Replace {% $variable.path %} with actual values from config.variables
        href = href.replace(/\{%\s*\$([a-zA-Z0-9._]+)\s*%\}/g, (match, path) => {
          const parts = path.split('.')
          let value = config.variables

          for (const part of parts) {
            if (value && typeof value === 'object') {
              value = value[part]
            } else {
              return match // Return original if path not found
            }
          }

          return value !== undefined ? value : match
        })

        // Add basePath to internal links when in GitHub Actions
        if (href.startsWith('/') && !href.startsWith('//')) {
          const basePath = process.env.GITHUB_ACTIONS ? '/acton-service' : ''
          href = basePath + href
        }
      }

      // Now transform other attributes normally
      const attributes = node.transformAttributes(config)

      // Override href with our processed version
      attributes.href = href

      return new Tag('a', attributes, children)
    },
  },
}

export default nodes
