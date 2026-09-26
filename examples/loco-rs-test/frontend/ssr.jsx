import { createInertiaApp } from '@inertiajs/react'
import createServer from '@inertiajs/react/server'
import { renderToString } from 'react-dom/server'

const pages = import.meta.glob('./pages/**/*.jsx', { eager: true })

createServer(page => createInertiaApp({
  page,
  render: renderToString,
  resolve: name => pages[`./pages/${name}.jsx`],
  setup: ({ App, props }) => <App {...props} />,
}))
